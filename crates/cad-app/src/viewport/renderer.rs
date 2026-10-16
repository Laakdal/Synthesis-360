//! Offscreen wgpu renderer for the 3D viewport. Renders into its own MSAA colour + depth targets
//! and resolves into a texture that egui displays as an image, so the 3D pass is independent of
//! egui's render pass formats.

use super::camera::{Camera, Projection};
use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use wgpu::util::DeviceExt;

pub const SAMPLES: u32 = 4;
// egui samples this texture without re-encoding, so colours are written in display (sRGB) space
pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Depth + stencil: the stencil drives section-analysis capping. Depth24PlusStencil8 is
/// guaranteed by WebGPU (also with MSAA 4); Depth32FloatStencil8 would need a device feature.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    viewport: [f32; 4],
    eye: [f32; 4],
    back: [f32; 4],
    bg_top: [f32; 4],
    bg_bottom: [f32; 4],
    clip: [f32; 4],
    clip_on: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct LineInst {
    pub p0: [f32; 3],
    pub p1: [f32; 3],
    pub color: [f32; 4],
    /// width px, depth offset (NDC), on-top flag, dash period px
    pub style: [f32; 4],
}

impl LineInst {
    pub fn new(p0: [f32; 3], p1: [f32; 3], color: [f32; 4], width: f32) -> Self {
        LineInst { p0, p1, color, style: [width, 2e-5, 0.0, 0.0] }
    }
    pub fn on_top(mut self) -> Self {
        self.style[2] = 1.0;
        self
    }
    pub fn dashed(mut self, period: f32) -> Self {
        self.style[3] = period;
        self
    }
    pub fn point(p: [f32; 3], color: [f32; 4], size: f32) -> Self {
        LineInst { p0: p, p1: p, color, style: [size, 1e-4, 0.0, 0.0] }
    }
}

/// CPU-side mesh to upload: positions/normals (xyz triples), per-vertex RGBA colours, indices.
pub struct MeshData {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    pub transparent: bool,
    /// Colour of the section-analysis cap of this body (opaque meshes only).
    pub cap_color: [f32; 4],
}

impl MeshData {
    pub fn new(transparent: bool) -> Self {
        MeshData { positions: Vec::new(), normals: Vec::new(), colors: Vec::new(), indices: Vec::new(), transparent, cap_color: CAP_COLOR }
    }
}

/// Default section cap colour (warm, so a cut reads clearly against grey bodies).
pub const CAP_COLOR: [f32; 4] = [0.93, 0.66, 0.42, 1.0];

/// Cap quad for a body: the section plane (normal `n`, keep `dot(n, p) <= w`) clipped to the
/// body's padded bounding box `[min xyz, max xyz]`; `None` when the plane misses the box. The
/// plane is origin-aligned, so the quad spans the box in the two in-plane axes.
pub fn cap_quad(bbox: [f32; 6], n: [f32; 3], w: f32) -> Option<[[f32; 3]; 4]> {
    let k = (0..3).max_by(|&a, &b| n[a].abs().total_cmp(&n[b].abs()))?;
    if n[k].abs() < 0.5 {
        return None;
    }
    let pk = w / n[k];
    if pk < bbox[k] - 1e-4 || pk > bbox[k + 3] + 1e-4 {
        return None;
    }
    let diag = ((bbox[3] - bbox[0]).powi(2) + (bbox[4] - bbox[1]).powi(2) + (bbox[5] - bbox[2]).powi(2)).sqrt();
    let pad = diag * 0.02 + 0.5;
    let (i, j) = ((k + 1) % 3, (k + 2) % 3);
    let corner = |a: f32, b: f32| {
        let mut p = [0.0; 3];
        p[k] = pk;
        p[i] = a;
        p[j] = b;
        p
    };
    let (i0, i1, j0, j1) = (bbox[i] - pad, bbox[i + 3] + pad, bbox[j] - pad, bbox[j + 3] + pad);
    Some([corner(i0, j0), corner(i1, j0), corner(i1, j1), corner(i0, j1)])
}

struct GpuMesh {
    vbuf: wgpu::Buffer,
    cbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    count: u32,
    transparent: bool,
    /// Axis-aligned bounds [min xyz, max xyz] (cap quad extent).
    bbox: [f32; 6],
    /// Section cap quad (4 vertices: position + normal, colours), rewritten every frame.
    cap_v: wgpu::Buffer,
    cap_c: wgpu::Buffer,
}

struct Targets {
    size: [u32; 2],
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    resolve_tex: wgpu::Texture,
    resolve: wgpu::TextureView,
}

pub struct Renderer {
    bg: wgpu::RenderPipeline,
    mesh_opaque: wgpu::RenderPipeline,
    mesh_transparent: wgpu::RenderPipeline,
    /// Section capping: stencil parity of the clipped mesh, cap quad where odd, stencil reset.
    stencil_mark: wgpu::RenderPipeline,
    cap: wgpu::RenderPipeline,
    stencil_clear: wgpu::RenderPipeline,
    cap_ibuf: wgpu::Buffer,
    lines: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    targets: Option<Targets>,
    meshes: Vec<GpuMesh>,
    fills: Vec<GpuMesh>,
    line_buf: Option<wgpu::Buffer>,
    line_count: u32,
    pub texture_id: Option<egui::TextureId>,
    pub bg_top: [f32; 4],
    pub bg_bottom: [f32; 4],
    /// Section analysis plane (normal, offset); `None` = off.
    pub section: Option<([f32; 3], f32)>,
}

/// Colours are authored and stored in display space (see COLOR_FORMAT).
fn srgb(c: [f32; 4]) -> [f32; 4] {
    c
}

impl Renderer {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("cad360-viewport"), source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()) });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cad360-uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("cad360-layout"), bind_group_layouts: &[Some(&bgl)], immediate_size: 0 });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor { label: Some("cad360-uniform"), size: std::mem::size_of::<Uniforms>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("cad360-bind"), layout: &bgl, entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }] });

        let blend = Some(wgpu::BlendState::ALPHA_BLENDING);
        let color_target = |b: Option<wgpu::BlendState>| [Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend: b, write_mask: wgpu::ColorWrites::ALL })];
        let ms = wgpu::MultisampleState { count: SAMPLES, mask: !0, alpha_to_coverage_enabled: false };
        let depth = |write: bool, compare: wgpu::CompareFunction, bias: wgpu::DepthBiasState| {
            Some(wgpu::DepthStencilState { format: DEPTH_FORMAT, depth_write_enabled: Some(write), depth_compare: Some(compare), stencil: wgpu::StencilState::default(), bias })
        };

        let bg_targets = color_target(None);
        let bg = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cad360-bg"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_bg"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth(false, wgpu::CompareFunction::Always, Default::default()),
            multisample: ms,
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_bg"), compilation_options: Default::default(), targets: &bg_targets }),
            multiview_mask: None,
            cache: None,
        });

        let mesh_buffers = [
            Some(wgpu::VertexBufferLayout { array_stride: 24, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3] }),
            Some(wgpu::VertexBufferLayout { array_stride: 16, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![2 => Float32x4] }),
        ];
        // push faces slightly back so edges drawn at the same depth stay visible
        let face_bias = wgpu::DepthBiasState { constant: 2, slope_scale: 1.5, clamp: 0.0 };
        let make_mesh = |label: &str, transparent: bool| {
            let targets = color_target(if transparent { blend } else { None });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_mesh"), compilation_options: Default::default(), buffers: &mesh_buffers },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
                depth_stencil: depth(!transparent, wgpu::CompareFunction::LessEqual, if transparent { Default::default() } else { face_bias }),
                multisample: ms,
                fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_mesh"), compilation_options: Default::default(), targets: &targets }),
                multiview_mask: None,
                cache: None,
            })
        };
        let mesh_opaque = make_mesh("cad360-mesh", false);
        let mesh_transparent = make_mesh("cad360-mesh-transparent", true);

        // ---- section capping (stencil parity per body)
        use wgpu::{CompareFunction as Cf, StencilOperation as So};
        let no_color = [Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend: None, write_mask: wgpu::ColorWrites::empty() })];
        let face = |compare, fail_op, depth_fail_op, pass_op| wgpu::StencilFaceState { compare, fail_op, depth_fail_op, pass_op };
        let stencil_depth = |write: bool, compare: Cf, stencil: wgpu::StencilState, bias: wgpu::DepthBiasState| {
            Some(wgpu::DepthStencilState { format: DEPTH_FORMAT, depth_write_enabled: Some(write), depth_compare: Some(compare), stencil, bias })
        };
        let tri = wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() };
        // (b) every surviving fragment of the body toggles bit 0 (both faces, no depth test)
        let invert = face(Cf::Always, So::Keep, So::Invert, So::Invert);
        let stencil_mark = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cad360-stencil-mark"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_mesh"), compilation_options: Default::default(), buffers: &mesh_buffers },
            primitive: tri,
            depth_stencil: stencil_depth(false, Cf::Always, wgpu::StencilState { front: invert, back: invert, read_mask: 0x01, write_mask: 0x01 }, Default::default()),
            multisample: ms,
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_clip_only"), compilation_options: Default::default(), targets: &no_color }),
            multiview_mask: None,
            cache: None,
        });
        // (c) the cap quad on the plane where the parity is odd, depth-tested and depth-writing
        let odd = face(Cf::NotEqual, So::Keep, So::Keep, So::Keep);
        let cap_targets = color_target(None);
        let cap = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cad360-cap"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_mesh"), compilation_options: Default::default(), buffers: &mesh_buffers },
            primitive: tri,
            depth_stencil: stencil_depth(true, Cf::LessEqual, wgpu::StencilState { front: odd, back: odd, read_mask: 0x01, write_mask: 0 }, face_bias),
            multisample: ms,
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_cap"), compilation_options: Default::default(), targets: &cap_targets }),
            multiview_mask: None,
            cache: None,
        });
        // (d) full-screen stencil reset before the next body
        let zero = face(Cf::Always, So::Zero, So::Zero, So::Zero);
        let stencil_clear = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cad360-stencil-clear"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_bg"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: stencil_depth(false, Cf::Always, wgpu::StencilState { front: zero, back: zero, read_mask: 0xff, write_mask: 0xff }, Default::default()),
            multisample: ms,
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_bg"), compilation_options: Default::default(), targets: &no_color }),
            multiview_mask: None,
            cache: None,
        });
        let cap_ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("cad360-cap-i"), contents: bytemuck::cast_slice(&[0u32, 1, 2, 0, 2, 3]), usage: wgpu::BufferUsages::INDEX });

        let line_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineInst>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x4],
        })];
        let line_targets = color_target(blend);
        let lines = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cad360-lines"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_line"), compilation_options: Default::default(), buffers: &line_buffers },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth(false, wgpu::CompareFunction::LessEqual, Default::default()),
            multisample: ms,
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_line"), compilation_options: Default::default(), targets: &line_targets }),
            multiview_mask: None,
            cache: None,
        });

        Renderer {
            bg,
            mesh_opaque,
            mesh_transparent,
            stencil_mark,
            cap,
            stencil_clear,
            cap_ibuf,
            lines,
            uniform,
            bind,
            targets: None,
            meshes: Vec::new(),
            fills: Vec::new(),
            line_buf: None,
            line_count: 0,
            texture_id: None,
            bg_top: [0.20, 0.22, 0.26, 1.0],
            bg_bottom: [0.12, 0.13, 0.15, 1.0],
            section: None,
        }
    }

    /// Ensures targets of `size`; returns true when the texture was (re)created.
    pub fn resize(&mut self, device: &wgpu::Device, size: [u32; 2]) -> bool {
        let size = [size[0].max(1), size[1].max(1)];
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return false;
        }
        let ext = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
        let tex = |label: &str, format, samples, usage| {
            device.create_texture(&wgpu::TextureDescriptor { label: Some(label), size: ext, mip_level_count: 1, sample_count: samples, dimension: wgpu::TextureDimension::D2, format, usage, view_formats: &[] })
        };
        let msaa = tex("cad360-msaa", COLOR_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&Default::default());
        let depth = tex("cad360-depth", DEPTH_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&Default::default());
        let resolve_tex = tex("cad360-resolve", COLOR_FORMAT, 1, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC);
        let resolve = resolve_tex.create_view(&Default::default());
        self.targets = Some(Targets { size, msaa, depth, resolve_tex, resolve });
        true
    }

    pub fn resolve_view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.resolve)
    }
    pub fn resolve_texture(&self) -> Option<&wgpu::Texture> {
        self.targets.as_ref().map(|t| &t.resolve_tex)
    }

    pub fn set_meshes(&mut self, device: &wgpu::Device, meshes: &[MeshData]) {
        self.meshes = Self::upload(device, meshes);
    }

    /// Transparent overlays (sketch profiles, plane squares), drawn after lines.
    pub fn set_fills(&mut self, device: &wgpu::Device, fills: &[MeshData]) {
        self.fills = Self::upload(device, fills);
    }

    fn upload(device: &wgpu::Device, meshes: &[MeshData]) -> Vec<GpuMesh> {
        meshes
            .iter()
            .filter(|m| !m.indices.is_empty())
            .map(|m| {
                let verts: Vec<f32> = m.positions.chunks(3).zip(m.normals.chunks(3)).flat_map(|(p, n)| [p[0], p[1], p[2], n[0], n[1], n[2]]).collect();
                let colors: Vec<[f32; 4]> = m.colors.iter().map(|c| srgb(*c)).collect();
                let mut bbox = [f32::MAX, f32::MAX, f32::MAX, f32::MIN, f32::MIN, f32::MIN];
                for p in m.positions.chunks(3) {
                    for k in 0..3 {
                        bbox[k] = bbox[k].min(p[k]);
                        bbox[k + 3] = bbox[k + 3].max(p[k]);
                    }
                }
                let cap_colors = [srgb(m.cap_color); 4];
                GpuMesh {
                    bbox,
                    cap_v: device.create_buffer(&wgpu::BufferDescriptor { label: Some("cad360-cap-v"), size: 4 * 24, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }),
                    cap_c: device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("cad360-cap-c"), contents: bytemuck::cast_slice(&cap_colors), usage: wgpu::BufferUsages::VERTEX }),
                    vbuf: device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("cad360-v"), contents: bytemuck::cast_slice(&verts), usage: wgpu::BufferUsages::VERTEX }),
                    cbuf: device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("cad360-c"), contents: bytemuck::cast_slice(&colors), usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST }),
                    ibuf: device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("cad360-i"), contents: bytemuck::cast_slice(&m.indices), usage: wgpu::BufferUsages::INDEX }),
                    count: m.indices.len() as u32,
                    transparent: m.transparent,
                }
            })
            .collect()
    }

    /// Replaces the per-vertex colours of mesh `i` (hover/selection highlighting).
    pub fn update_colors(&self, queue: &wgpu::Queue, i: usize, colors: &[[f32; 4]]) {
        if let Some(m) = self.meshes.get(i) {
            let c: Vec<[f32; 4]> = colors.iter().map(|c| srgb(*c)).collect();
            queue.write_buffer(&m.cbuf, 0, bytemuck::cast_slice(&c));
        }
    }

    pub fn set_lines(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, lines: &[LineInst]) {
        let lines: Vec<LineInst> = lines.iter().map(|l| LineInst { color: srgb(l.color), ..*l }).collect();
        let bytes: &[u8] = bytemuck::cast_slice(&lines);
        self.line_count = lines.len() as u32;
        if bytes.is_empty() {
            return;
        }
        let need = bytes.len() as u64;
        if self.line_buf.as_ref().is_none_or(|b| b.size() < need) {
            self.line_buf = Some(device.create_buffer(&wgpu::BufferDescriptor { label: Some("cad360-lines"), size: need.next_power_of_two(), usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }));
        }
        queue.write_buffer(self.line_buf.as_ref().unwrap(), 0, bytes);
    }

    pub fn render(&self, device: &wgpu::Device, queue: &wgpu::Queue, cam: &Camera) {
        let Some(t) = &self.targets else { return };
        let aspect = t.size[0] as f32 / t.size[1] as f32;
        let eye = cam.eye();
        let back = cam.back();
        let u = Uniforms {
            view_proj: cam.view_proj(aspect).to_cols_array_2d(),
            viewport: [t.size[0] as f32, t.size[1] as f32, 0.0, 0.0],
            eye: [eye.x, eye.y, eye.z, if cam.projection == Projection::Perspective { 1.0 } else { 0.0 }],
            back: [back.x, back.y, back.z, 0.0],
            bg_top: srgb(self.bg_top),
            bg_bottom: srgb(self.bg_bottom),
            clip: self.section.map(|(n, d)| [n[0], n[1], n[2], d]).unwrap_or([0.0; 4]),
            clip_on: if self.section.is_some() { [1.0, 0.0, 0.0, 0.0] } else { [0.0; 4] },
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&u));
        // section caps: one quad per opaque body crossed by the plane, rebuilt every frame so the
        // cap follows the offset slider live
        let caps: Vec<bool> = self
            .meshes
            .iter()
            .map(|m| {
                let Some((n, w)) = self.section.filter(|_| !m.transparent) else { return false };
                let Some(q) = cap_quad(m.bbox, n, w) else { return false };
                let v: Vec<f32> = q.iter().flat_map(|p| [p[0], p[1], p[2], n[0], n[1], n[2]]).collect();
                queue.write_buffer(&m.cap_v, 0, bytemuck::cast_slice(&v));
                true
            })
            .collect();
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("cad360-frame") });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cad360-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.msaa,
                    depth_slice: None,
                    resolve_target: Some(&t.resolve),
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Discard },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Discard }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.bind, &[]);
            pass.set_pipeline(&self.bg);
            pass.draw(0..3, 0..1);
            pass.set_stencil_reference(0);
            for (m, &capped) in self.meshes.iter().zip(&caps).filter(|(m, _)| !m.transparent) {
                pass.set_pipeline(&self.mesh_opaque);
                pass.set_vertex_buffer(0, m.vbuf.slice(..));
                pass.set_vertex_buffer(1, m.cbuf.slice(..));
                pass.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..m.count, 0, 0..1);
                if !capped {
                    continue;
                }
                // Per-body capping: an odd number of surviving surface fragments along the view
                // ray means the plane point is inside the solid. Resetting the stencil after each
                // body keeps overlapping bodies from cancelling each other's caps.
                pass.set_pipeline(&self.stencil_mark);
                pass.draw_indexed(0..m.count, 0, 0..1);
                pass.set_pipeline(&self.cap);
                pass.set_vertex_buffer(0, m.cap_v.slice(..));
                pass.set_vertex_buffer(1, m.cap_c.slice(..));
                pass.set_index_buffer(self.cap_ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..6, 0, 0..1);
                pass.set_pipeline(&self.stencil_clear);
                pass.draw(0..3, 0..1);
            }
            if let (Some(b), n) = (&self.line_buf, self.line_count) {
                if n > 0 {
                    pass.set_pipeline(&self.lines);
                    pass.set_vertex_buffer(0, b.slice(..));
                    pass.draw(0..6, 0..n);
                }
            }
            pass.set_pipeline(&self.mesh_transparent);
            for m in self.meshes.iter().filter(|m| m.transparent).chain(self.fills.iter()) {
                pass.set_vertex_buffer(0, m.vbuf.slice(..));
                pass.set_vertex_buffer(1, m.cbuf.slice(..));
                pass.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..m.count, 0, 0..1);
            }
        }
        queue.submit(Some(enc.finish()));
    }
}

#[cfg(test)]
mod tests {
    use super::cap_quad;

    #[test]
    fn cap_quad_spans_the_box_on_the_plane() {
        let bb = [0.0, 0.0, 0.0, 60.0, 30.0, 20.0];
        // XZ plane at y = 15 (normal +Y)
        let q = cap_quad(bb, [0.0, 1.0, 0.0], 15.0).unwrap();
        assert!(q.iter().all(|p| (p[1] - 15.0).abs() < 1e-6));
        let (xs, zs): (Vec<f32>, Vec<f32>) = q.iter().map(|p| (p[0], p[2])).unzip();
        assert!(xs.iter().cloned().fold(f32::MAX, f32::min) < 0.0 && xs.iter().cloned().fold(f32::MIN, f32::max) > 60.0);
        assert!(zs.iter().cloned().fold(f32::MAX, f32::min) < 0.0 && zs.iter().cloned().fold(f32::MIN, f32::max) > 20.0);
        // flipped plane: keep dot(-n, p) <= -15 -> the same plane y = 15
        let q = cap_quad(bb, [0.0, -1.0, 0.0], -15.0).unwrap();
        assert!(q.iter().all(|p| (p[1] - 15.0).abs() < 1e-6));
        // plane outside the box
        assert!(cap_quad(bb, [0.0, 0.0, 1.0], 25.0).is_none());
        assert!(cap_quad(bb, [1.0, 0.0, 0.0], -1.0).is_none());
    }
}
