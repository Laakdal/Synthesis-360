// CAD360 viewport shaders: background gradient, lit B-rep meshes, screen-space thick lines/points.

struct Uniforms {
    view_proj: mat4x4<f32>,
    // xy: viewport size in pixels
    viewport: vec4<f32>,
    // xyz: eye position, w: 1 = perspective, 0 = orthographic
    eye: vec4<f32>,
    // xyz: direction from the target towards the eye (orthographic view direction)
    back: vec4<f32>,
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
    // section analysis: xyz plane normal, w offset; keep points with dot(n, p) <= w
    clip: vec4<f32>,
    // x: 1 = section active; yzw: cap colour
    clip_on: vec4<f32>,
};

fn clipped(p: vec3<f32>) -> bool {
    return u.clip_on.x > 0.5 && dot(u.clip.xyz, p) > u.clip.w;
}

@group(0) @binding(0) var<uniform> u: Uniforms;

// ---------------------------------------------------------------- background
struct BgOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) t: f32,
};

@vertex
fn vs_bg(@builtin(vertex_index) vi: u32) -> BgOut {
    var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    var o: BgOut;
    o.pos = vec4<f32>(p[vi], 1.0, 1.0);
    o.t = p[vi].y * 0.5 + 0.5;
    return o;
}

@fragment
fn fs_bg(i: BgOut) -> @location(0) vec4<f32> {
    return mix(u.bg_bottom, u.bg_top, clamp(i.t, 0.0, 1.0));
}

// ---------------------------------------------------------------- meshes
struct MeshIn {
    @location(0) pos: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) color: vec4<f32>,
};
struct MeshOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) wpos: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_mesh(v: MeshIn) -> MeshOut {
    var o: MeshOut;
    o.pos = u.view_proj * vec4<f32>(v.pos, 1.0);
    o.wpos = v.pos;
    o.nrm = v.nrm;
    o.color = v.color;
    return o;
}

@fragment
fn fs_mesh(i: MeshOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if (clipped(i.wpos)) {
        discard;
    }
    let to_eye = mix(u.back.xyz, normalize(u.eye.xyz - i.wpos), u.eye.w);
    var n = normalize(i.nrm);
    if (dot(n, to_eye) < 0.0) {
        n = -n;
    }
    // headlight slightly above and to the left of the eye, plus a soft fill
    let l = normalize(to_eye + vec3<f32>(0.0, 0.0, 0.35));
    let diff = max(dot(n, l), 0.0);
    let h = normalize(l + to_eye);
    let spec = pow(max(dot(n, h), 0.0), 48.0);
    let c = i.color.rgb * (0.32 + 0.68 * diff) + vec3<f32>(0.22) * spec;
    return vec4<f32>(c, i.color.a);
}

// ---------------------------------------------------------------- section caps
// Stencil-marking pass: only the clip test matters (colour writes are off).
@fragment
fn fs_clip_only(i: MeshOut) -> @location(0) vec4<f32> {
    if (clipped(i.wpos)) {
        discard;
    }
    return vec4<f32>(0.0);
}

// The cap quad lies exactly on the plane, so it must not run the clip test (dot(n, p) == w
// would speckle). Solid cap colour with a screen-space 45° hatch, like Fusion's section fill.
@fragment
fn fs_cap(i: MeshOut) -> @location(0) vec4<f32> {
    // distance to the nearest hatch line, 0 (on a line) .. 1 (between lines)
    let d = 1.0 - abs(fract((i.pos.x + i.pos.y) / 9.0) - 0.5) * 2.0;
    let on_line = 1.0 - smoothstep(0.12, 0.30, d);
    let c = mix(i.color.rgb, i.color.rgb * 0.60, on_line);
    return vec4<f32>(c, 1.0);
}

// ---------------------------------------------------------------- thick lines and points
struct LineIn {
    @location(0) p0: vec3<f32>,
    @location(1) p1: vec3<f32>,
    @location(2) color: vec4<f32>,
    // x: width in pixels, y: depth offset towards the viewer (NDC), z: 1 = draw on top, w: dash period px
    @location(3) style: vec4<f32>,
};
struct LineOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) along_px: f32,
    @location(2) dash: f32,
    @location(3) wpos: vec3<f32>,
};

@vertex
fn vs_line(@builtin(vertex_index) vi: u32, l: LineIn) -> LineOut {
    let c0 = u.view_proj * vec4<f32>(l.p0, 1.0);
    let c1 = u.view_proj * vec4<f32>(l.p1, 1.0);
    let half = u.viewport.xy * 0.5;
    let s0 = c0.xy / c0.w * half;
    let s1 = c1.xy / c1.w * half;
    var d = s1 - s0;
    let seg_len = length(d);
    let is_point = seg_len < 1e-3;
    if (is_point) {
        d = vec2<f32>(1.0, 0.0);
    } else {
        d = d / seg_len;
    }
    let n = vec2<f32>(-d.y, d.x);
    // two triangles: corners (along, side)
    var along = array<f32, 6>(0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    var side = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    let a = along[vi];
    let s = side[vi];
    let w = l.style.x * 0.5;
    var base = c0;
    if (a > 0.5) {
        base = c1;
    }
    var off = n * s * w;
    if (is_point) {
        off = vec2<f32>((a * 2.0 - 1.0) * w, s * w);
    }
    var o: LineOut;
    let z = select(base.z / base.w - l.style.y, 0.0, l.style.z > 0.5);
    o.pos = vec4<f32>((base.xy / base.w + off / half) * base.w, clamp(z, 0.0, 1.0) * base.w, base.w);
    o.color = l.color;
    o.along_px = a * seg_len;
    o.dash = l.style.w;
    o.wpos = select(l.p0, l.p1, a > 0.5);
    return o;
}

@fragment
fn fs_line(i: LineOut) -> @location(0) vec4<f32> {
    if (clipped(i.wpos)) {
        discard;
    }
    if (i.dash > 0.5) {
        if (fract(i.along_px / i.dash) > 0.55) {
            discard;
        }
    }
    return i.color;
}
