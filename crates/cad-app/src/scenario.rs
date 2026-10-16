//! Scripted scenarios: `cad360 --scenario steps.json --screenshot-dir DIR`.
//!
//! Steps run one per frame through the same command registry, tool events and AppState calls
//! that user input uses, so a scenario exercises the real application, and `screenshot` steps
//! capture the whole window (eframe `ViewportCommand::Screenshot`). Used to produce the
//! implementation screenshots placed next to the Fusion reference screenshots.
//!
//! ```json
//! { "steps": [
//!   {"op": "select_plane", "plane": "XY"}, {"op": "cmd", "id": "CreateSketch"},
//!   {"op": "cmd", "id": "Rectangle2Pt"}, {"op": "sketch_click", "at": [0, 0]},
//!   {"op": "sketch_click", "at": [50, 30]}, {"op": "screenshot", "name": "sketch"} ] }
//! ```

use crate::app::CadApp;
use crate::state::command::{CommandId, COMMANDS};
use crate::state::sketch::{PointerKind, SketchKey, SketchPointer};
use crate::state::Pick;
use crate::viewport::camera::StandardView;
use crate::viewport::manipulator::PointerInput;
use cad_core::OriginPlane;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Step {
    /// Run a registry command by its name (e.g. "Extrude", "Rectangle2Pt").
    Cmd { id: String },
    New,
    Open { path: String },
    Save { path: String },
    ExportStep { path: String },
    ExportStl { path: String },
    ImportStep { path: String },
    SelectPlane { plane: String },
    /// Select a profile of the n-th sketch feature in the timeline.
    SelectProfile { sketch: usize, profile: usize },
    /// Select edges of the first body whose direction is mostly along `axis` ("x" / "y" / "z").
    SelectEdges { axis: String, #[serde(default)] add: bool },
    /// Select the planar face of the first body whose outward normal is closest to `normal`.
    SelectFace { normal: [f64; 3], #[serde(default)] body: usize },
    ClearSelection,
    /// Pointer click (move + press + release + click) at sketch coordinates.
    SketchClick { at: [f64; 2] },
    SketchMove { at: [f64; 2] },
    SketchDoubleClick { at: [f64; 2] },
    Key { key: String },
    SetParam { name: String, expr: String },
    /// Type a value into the open dialog's primary field (distance, radius, ...).
    SetDialogParam { expr: String },
    /// Type into the sketch dimension value box and press Enter.
    SketchValue { text: String },
    /// Accept "create as driven dimension" after an over-constraint warning.
    AcceptDriven,
    /// Extrude dialog options: operation "New"/"Join"/"Cut"/"Intersect", direction "Normal"/"Reverse"/"Symmetric".
    ExtrudeOptions { #[serde(default)] operation: Option<String>, #[serde(default)] direction: Option<String>, #[serde(default)] through_all: Option<bool> },
    /// Press on the open dialog's on-canvas manipulator (arrow) at its current value.
    ManipPress,
    /// Move the pressed manipulator to where its value would be `value` (mm).
    ManipDrag { value: f64, #[serde(default)] shift: bool },
    /// Release the manipulator (commits the value into the dialog).
    ManipRelease,
    DialogOk,
    DialogCancel,
    EditFeature { index: usize },
    Marker { index: usize },
    Undo,
    Redo,
    View { view: String },
    Fit,
    Orbit { dx: f32, dy: f32 },
    Wait { frames: u32 },
    Screenshot { name: String },
    Log { text: String },
    Close,
}

#[derive(Debug, Deserialize)]
struct File {
    steps: Vec<Step>,
}

pub struct Scenario {
    steps: Vec<Step>,
    next: usize,
    wait: u32,
    out: Option<PathBuf>,
    pending_shot: Option<Arc<AtomicBool>>,
    seen_messages: usize,
    pub done: bool,
}

pub fn command_by_name(name: &str) -> Option<CommandId> {
    COMMANDS.iter().map(|c| c.id).find(|id| format!("{id:?}") == name)
}

impl Scenario {
    pub fn load(path: &std::path::Path, out: Option<PathBuf>) -> Result<Scenario, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let f: File = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        if let Some(o) = &out {
            std::fs::create_dir_all(o).map_err(|e| e.to_string())?;
        }
        Ok(Scenario { steps: f.steps, next: 0, wait: 8, out, pending_shot: None, seen_messages: 0, done: false })
    }

    fn pointer(app: &mut CadApp, at: [f64; 2], kind: PointerKind) {
        app.state.sketch_pointer(SketchPointer { pos: at, tolerance: 0.5, kind, shift: false, ctrl: false });
    }

    /// Called at the start of every frame; executes at most one step.
    pub fn step(&mut self, app: &mut CadApp, ctx: &egui::Context) {
        ctx.request_repaint();
        if let Some(flag) = &self.pending_shot {
            if !flag.load(Ordering::SeqCst) {
                return;
            }
            self.pending_shot = None;
        }
        if self.wait > 0 {
            self.wait -= 1;
            return;
        }
        if self.done {
            return;
        }
        let Some(step) = self.steps.get(self.next) else {
            self.done = true;
            eprintln!("CAD360 scenario: finished");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        };
        self.next += 1;
        self.wait = 2;
        let st = &mut app.state;
        match step {
            Step::Cmd { id } => match command_by_name(id) {
                Some(c) => st.execute(c),
                None => eprintln!("CAD360 scenario: unknown command {id}"),
            },
            Step::New => st.new_design(),
            Step::Open { path } => {
                st.open(std::path::Path::new(path));
            }
            Step::Save { path } => {
                st.save_to(std::path::Path::new(path));
            }
            Step::ExportStep { path } => {
                st.export_step(std::path::Path::new(path));
            }
            Step::ExportStl { path } => {
                st.export_stl(std::path::Path::new(path));
            }
            Step::ImportStep { path } => {
                st.import_step(std::path::Path::new(path));
            }
            Step::SelectPlane { plane } => {
                let p = match plane.as_str() {
                    "XZ" => OriginPlane::XZ,
                    "YZ" => OriginPlane::YZ,
                    _ => OriginPlane::XY,
                };
                st.select(Pick::OriginPlane(p), false);
            }
            Step::SelectProfile { sketch, profile } => {
                let sk = st.doc.features.iter().filter(|f| f.sketch().is_some()).nth(*sketch).map(|f| f.id.clone());
                match sk {
                    Some(id) => st.select(Pick::Profile { sketch: id, index: *profile }, false),
                    None => eprintln!("CAD360 scenario: no sketch #{sketch}"),
                }
            }
            Step::SelectEdges { axis, add } => {
                let k = match axis.as_str() {
                    "x" => 0,
                    "y" => 1,
                    _ => 2,
                };
                if !add {
                    st.selection.clear();
                }
                if let Some(b) = st.model.bodies.first().cloned() {
                    let t = b.topology();
                    for (i, e) in t.edge_info.iter().enumerate() {
                        if e.curve == cad_core::CurveKind::Line && e.dir[k].abs() > 0.999 {
                            st.select(Pick::Edge { body: b.id.clone(), edge: i }, true);
                        }
                    }
                }
            }
            Step::SelectFace { normal, body } => {
                if let Some(b) = st.model.bodies.get(*body).cloned() {
                    let t = b.topology();
                    let best = t
                        .face_info
                        .iter()
                        .enumerate()
                        .filter(|(_, f)| f.surface == cad_core::SurfaceKind::Plane)
                        .max_by(|a, b2| {
                            let d = |n: [f64; 3]| n[0] * normal[0] + n[1] * normal[1] + n[2] * normal[2];
                            d(a.1.normal).partial_cmp(&d(b2.1.normal)).unwrap()
                        })
                        .map(|(i, _)| i);
                    if let Some(i) = best {
                        st.select(Pick::Face { body: b.id.clone(), face: i }, false);
                    }
                }
            }
            Step::ClearSelection => st.clear_selection(),
            Step::SketchClick { at } => {
                for k in [PointerKind::Move, PointerKind::Press, PointerKind::Release, PointerKind::Click] {
                    Self::pointer(app, *at, k);
                }
            }
            Step::SketchMove { at } => Self::pointer(app, *at, PointerKind::Move),
            Step::SketchDoubleClick { at } => {
                for k in [PointerKind::Move, PointerKind::Press, PointerKind::Release, PointerKind::DoubleClick] {
                    Self::pointer(app, *at, k);
                }
            }
            Step::Key { key } => match key.as_str() {
                "Escape" => st.sketch_key(SketchKey::Escape),
                "Enter" => st.sketch_key(SketchKey::Enter),
                "Delete" => st.execute(CommandId::Delete),
                other => eprintln!("CAD360 scenario: unknown key {other}"),
            },
            Step::SetParam { name, expr } => {
                st.set_parameter(name, expr);
            }
            Step::SetDialogParam { expr } => {
                if !st.dialog_set_primary(expr) {
                    eprintln!("CAD360 scenario: dialog rejected {expr}");
                }
            }
            Step::SketchValue { text } => {
                st.sketch_set_value_text(text);
                st.sketch_value_commit();
            }
            Step::AcceptDriven => st.sketch_accept_driven(),
            Step::ExtrudeOptions { operation, direction, through_all } => {
                use cad_core::{ExtentDirection, Operation};
                let op = operation.as_deref().map(|o| match o {
                    "Join" => Operation::Join,
                    "Cut" => Operation::Cut,
                    "Intersect" => Operation::Intersect,
                    _ => Operation::New,
                });
                let dir = direction.as_deref().map(|d| match d {
                    "Reverse" => ExtentDirection::Reverse,
                    "Symmetric" => ExtentDirection::Symmetric,
                    _ => ExtentDirection::Normal,
                });
                let all = *through_all;
                st.dialog_edit(|d| {
                    if let crate::state::Dialog::Extrude(x) = d {
                        if let Some(o) = op {
                            x.operation = o;
                        }
                        if let Some(v) = dir {
                            x.direction = v;
                        }
                        if let Some(a) = all {
                            x.through_all = a;
                        }
                    }
                });
            }
            Step::ManipPress => {
                let view = app.view();
                let time = ctx.input(|i| i.time);
                match app.state.manipulator_handle() {
                    Some(h) => {
                        let at = PointerInput { pos: view.to_screen(h.point_at(h.value)), moved: true, time, ..Default::default() };
                        app.manip.input(&mut app.state, &view, &at);
                        app.manip.input(&mut app.state, &view, &PointerInput { pressed: true, ..at });
                        if !app.manip.dragging() {
                            eprintln!("CAD360 scenario: manipulator press missed the handle");
                        }
                    }
                    None => eprintln!("CAD360 scenario: the open dialog has no manipulator"),
                }
            }
            Step::ManipDrag { value, shift } => {
                let view = app.view();
                let time = ctx.input(|i| i.time);
                match app.manip.drag_handle().cloned() {
                    Some(h) => {
                        let at = PointerInput { pos: view.to_screen(h.point_at(*value)), moved: true, shift: *shift, time, ..Default::default() };
                        app.manip.input(&mut app.state, &view, &at);
                        eprintln!("CAD360 scenario: manipulator value {:?}", app.manip.drag_value());
                    }
                    None => eprintln!("CAD360 scenario: manip_drag without manip_press"),
                }
            }
            Step::ManipRelease => {
                let view = app.view();
                let time = ctx.input(|i| i.time);
                app.manip.input(&mut app.state, &view, &PointerInput { released: true, time, ..Default::default() });
            }
            Step::DialogOk => st.dialog_ok(),
            Step::DialogCancel => st.dialog_cancel(),
            Step::EditFeature { index } => {
                if let Some(id) = st.doc.features.get(*index).map(|f| f.id.clone()) {
                    st.edit_feature(&id);
                }
            }
            Step::Marker { index } => st.set_marker(*index),
            Step::Undo => st.undo(),
            Step::Redo => st.redo(),
            Step::View { view } => {
                let v = match view.as_str() {
                    "Top" => StandardView::Top,
                    "Front" => StandardView::Front,
                    "Right" => StandardView::Right,
                    "Back" => StandardView::Back,
                    "Left" => StandardView::Left,
                    "Bottom" => StandardView::Bottom,
                    _ => StandardView::Home,
                };
                st.view_requests.push(crate::state::ViewRequest::Standard(v));
            }
            Step::Fit => st.view_requests.push(crate::state::ViewRequest::Fit),
            Step::Orbit { dx, dy } => app.cam.orbit(glam::Vec2::new(*dx, *dy)),
            Step::Wait { frames } => self.wait = *frames,
            Step::Log { text } => eprintln!("CAD360 scenario: {text}"),
            Step::Screenshot { name } => {
                if let Some(dir) = &self.out {
                    let path = dir.join(format!("{name}.png"));
                    let flag = Arc::new(AtomicBool::new(false));
                    let f2 = flag.clone();
                    let ctx2 = ctx.clone();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                        if let Err(e) = save_png(&path, &img) {
                            eprintln!("CAD360 scenario: screenshot {} failed: {e}", path.display());
                        } else {
                            eprintln!("CAD360 scenario: wrote {}", path.display());
                        }
                        f2.store(true, Ordering::SeqCst);
                        ctx2.request_repaint();
                    })));
                    self.pending_shot = Some(flag);
                    self.wait = 6;
                }
            }
            Step::Close => {
                self.done = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        // report new messages so scenario logs show errors
        for m in app.state.messages.iter().skip(self.seen_messages) {
            eprintln!("CAD360 scenario: [{:?}] {}", m.level, m.text);
        }
        self.seen_messages = app.state.messages.len();
    }
}

pub fn save_png(path: &std::path::Path, img: &egui::ColorImage) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.size[0] as u32, img.size[1] as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
    w.write_image_data(&bytes).map_err(|e| e.to_string())
}
