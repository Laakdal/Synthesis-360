//! Headless tests of the Shell, Combine, Mirror, Rectangular / Circular Pattern and Move/Copy
//! dialogs: live preview, OK = one undo step, Cancel restores, edit_feature reopens the dialog.

use super::*;
use crate::state::command::spec;
use crate::state::{AppState, Pick};
use cad_core::sketch::{ConstraintKind as K, DistanceOrientation as O, EntityKind, ORIGIN};
use cad_core::{ops, OriginAxis, OriginPlane, PlaneRef, SurfaceKind};

/// `AppState::new()` touches OCC's global messenger; serialise construction across test threads.
static OCC_INIT: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn new_state() -> AppState {
    let _guard = OCC_INIT.lock().unwrap_or_else(|e| e.into_inner());
    AppState::new()
}

/// Adds a w x h x d box as a new body (corner at `at`, dimensions fixed when `at` is the origin).
fn add_box(st: &mut AppState, at: [f64; 2], w: f64, h: f64, d: &str) -> String {
    let doc = &mut st.doc;
    let sk = ops::new_sketch(doc, PlaneRef::Origin { plane: OriginPlane::XY });
    let s = ops::sketch_mut(doc, &sk).unwrap();
    let lines = s.add_rectangle(at, [at[0] + w, at[1] + h]);
    if at == [0.0, 0.0] {
        let corner = match &s.entity(&lines[0]).unwrap().kind {
            EntityKind::Line { p1, .. } => p1.clone(),
            _ => unreachable!(),
        };
        s.add_constraint(K::Coincident { a: corner, b: ORIGIN.into() });
        ops::add_dimension(doc, &sk, K::Distance { a: lines[0].clone(), b: lines[0].clone(), orientation: O::Horizontal }, &format!("{w} mm"), [20.0, -8.0], false).unwrap();
        ops::add_dimension(doc, &sk, K::Distance { a: lines[1].clone(), b: lines[1].clone(), orientation: O::Vertical }, &format!("{h} mm"), [48.0, 10.0], false).unwrap();
    }
    st.recompute();
    let p = &st.model.sketches[&sk].profiles[0];
    let prof = vec![ProfileRef { curves: p.curves.clone(), sample: p.interior_point() }];
    let ext = ops::new_extrude(&mut st.doc, &sk, prof, d, ExtentDirection::Normal, Operation::New);
    st.recompute();
    assert_eq!(st.model.health_of(&ext), Health::Healthy);
    ext
}

fn volume(st: &AppState) -> f64 {
    st.model.bodies.iter().map(|b| b.volume()).sum()
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn undo_steps(st: &mut AppState) -> usize {
    let mut n = 0;
    while st.history.can_undo() {
        st.undo();
        n += 1;
    }
    n
}

/// A canvas click (the hovered item is the clicked one).
fn click(st: &mut AppState, p: Pick) {
    st.hover = Some(p.clone());
    st.select(p, false);
    st.hover = None;
}

fn face_with_normal(st: &AppState, body: &str, n: [f64; 3]) -> Pick {
    let b = st.model.body(body).unwrap();
    let t = b.topology();
    let i = t.face_info.iter().position(|f| f.surface == SurfaceKind::Plane && cad_kernel::frame::dot(f.normal, n) > 0.999).unwrap();
    Pick::Face { body: body.into(), face: i }
}

fn feature_of(st: &AppState, label: &str) -> String {
    st.doc.features.iter().find(|f| f.type_label() == label).unwrap_or_else(|| panic!("no {label} feature")).id.clone()
}

#[test]
fn new_commands_are_registered_and_supported() {
    for id in [CommandId::Shell, CommandId::Combine, CommandId::MirrorFeature, CommandId::RectangularPattern, CommandId::CircularPattern, CommandId::Move] {
        assert!(spec(id).supported, "{id:?}");
    }
    assert_eq!(spec(CommandId::CircularPattern).panel, crate::state::command::Panel::Create);
    let st = new_state();
    assert!(st.is_enabled(CommandId::Shell));
}

#[test]
fn shell_dialog_preview_ok_undo_and_edit() {
    let mut st = new_state();
    let b = add_box(&mut st, [0.0, 0.0], 50.0, 30.0, "20 mm");
    let params = st.doc.parameters.len();
    // pre-selected top face
    let top = face_with_normal(&st, &b, [0.0, 0.0, 1.0]);
    click(&mut st, top);
    st.execute(CommandId::Shell);
    assert_eq!(st.dialog.as_ref().unwrap().title(), "SHELL");
    assert_eq!(st.dialog.as_ref().unwrap().input_count(), 1);
    assert!(st.in_txn());
    // live preview with the default 2 mm
    assert!(close(volume(&st), 30000.0 - 46.0 * 26.0 * 18.0, 1e-3), "{}", volume(&st));
    assert_eq!(st.dialog_status().as_deref(), Some("1 Face"));
    assert!(st.dialog_set_primary("3 mm"));
    assert!(close(volume(&st), 30000.0 - 44.0 * 24.0 * 17.0, 1e-3));

    // too thick: the feature errors, OK is blocked, nothing crashes
    assert!(st.dialog_set_primary("20 mm"));
    assert!(st.dialog_blocker().is_some());
    st.dialog_ok();
    assert!(st.dialog.is_some(), "OK refused while the feature has an error");
    assert!(close(volume(&st), 30000.0, 1e-6));
    assert!(st.dialog_set_primary("2 mm"));
    st.dialog_ok();
    assert!(st.dialog.is_none() && !st.in_txn(), "{:?}", st.messages.last());
    assert_eq!(st.history.undo_label(), Some("Shell"));
    assert_eq!(st.doc.parameters.len(), params + 1, "one model parameter (thickness)");
    let sh = feature_of(&st, "Shell");

    // edit: the dialog reopens with the stored values
    st.edit_feature(&sh);
    match st.dialog.as_ref().unwrap() {
        Dialog::Shell(x) => {
            assert_eq!(x.s.editing.as_deref(), Some(sh.as_str()));
            assert_eq!(x.thickness.text, "2 mm");
            assert_eq!(x.faces.len(), 1);
        }
        d => panic!("{d:?}"),
    }
    assert_eq!(st.dialog.as_ref().unwrap().title(), "EDIT FEATURE");
    assert!(st.dialog_set_primary("1 mm"));
    st.dialog_ok();
    assert_eq!(st.history.undo_label(), Some("Edit Shell1"));
    assert!(close(volume(&st), 30000.0 - 48.0 * 28.0 * 19.0, 1e-3));
    assert_eq!(undo_steps(&mut st), 2);
    assert!(close(volume(&st), 30000.0, 1e-6));
}

#[test]
fn shell_cancel_restores_and_face_clicks_toggle() {
    let mut st = new_state();
    let b = add_box(&mut st, [0.0, 0.0], 50.0, 30.0, "20 mm");
    let (features, params) = (st.doc.features.len(), st.doc.parameters.len());
    st.execute(CommandId::Shell);
    assert_eq!(st.dialog_blocker().as_deref(), Some("Select the faces to remove in the canvas."));
    let top = face_with_normal(&st, &b, [0.0, 0.0, 1.0]);
    let side = face_with_normal(&st, &b, [1.0, 0.0, 0.0]);
    click(&mut st, top.clone());
    click(&mut st, side.clone());
    assert_eq!(st.dialog.as_ref().unwrap().input_count(), 2);
    assert!(close(volume(&st), 30000.0 - 48.0 * 26.0 * 18.0, 1e-3), "open top and +X side: {}", volume(&st));
    // with an invalid thickness the canvas shows the unshelled body: clicks toggle faces
    assert!(st.dialog_set_primary("20 mm"));
    assert!(close(volume(&st), 30000.0, 1e-6));
    let bottom = face_with_normal(&st, &b, [0.0, 0.0, -1.0]);
    click(&mut st, bottom.clone());
    assert_eq!(st.dialog.as_ref().unwrap().input_count(), 3);
    click(&mut st, bottom);
    assert_eq!(st.dialog.as_ref().unwrap().input_count(), 2);
    click(&mut st, top);
    assert_eq!(st.dialog.as_ref().unwrap().input_count(), 1, "clicking a selected face removes it");
    assert!(st.dialog_set_primary("2 mm"));
    assert!(close(volume(&st), 30000.0 - 48.0 * 26.0 * 16.0, 1e-3), "only +X open: {}", volume(&st));
    st.dialog_cancel();
    assert!(st.dialog.is_none() && !st.in_txn());
    assert_eq!((st.doc.features.len(), st.doc.parameters.len()), (features, params));
    assert!(close(volume(&st), 30000.0, 1e-6));
    assert!(!st.history.can_undo());
}

#[test]
fn combine_dialog_target_then_tools() {
    let mut st = new_state();
    let a = add_box(&mut st, [0.0, 0.0], 50.0, 30.0, "20 mm");
    let b = add_box(&mut st, [40.0, 10.0], 20.0, 10.0, "20 mm");
    st.execute(CommandId::Combine);
    assert_eq!(st.dialog_blocker().as_deref(), Some("Select a target body."));
    let p = face_with_normal(&st, &a, [0.0, 0.0, 1.0]);
    click(&mut st, p);
    assert_eq!(st.dialog_blocker().as_deref(), Some("Select tool bodies."));
    let p = face_with_normal(&st, &b, [0.0, 0.0, 1.0]);
    click(&mut st, p);
    match st.dialog.as_ref().unwrap() {
        Dialog::Combine(x) => {
            assert_eq!(x.target.as_deref(), Some(a.as_str()));
            assert_eq!(x.tools, vec![b.clone()]);
            assert_eq!(x.operation, Operation::Join);
        }
        d => panic!("{d:?}"),
    }
    assert_eq!(st.model.bodies.len(), 1, "join preview");
    assert!(close(volume(&st), 32000.0, 1e-6));
    st.dialog_edit(|d| {
        if let Dialog::Combine(x) = d {
            x.operation = Operation::Cut;
        }
    });
    assert!(close(volume(&st), 28000.0, 1e-6), "{}", volume(&st));
    st.dialog_ok();
    assert!(st.dialog.is_none(), "{:?}", st.messages.last());
    assert_eq!(st.history.undo_label(), Some("Combine"));
    let c = feature_of(&st, "Combine");

    st.edit_feature(&c);
    match st.dialog.as_mut().unwrap() {
        Dialog::Combine(x) => {
            assert_eq!((x.target.as_deref(), x.tools.len(), x.operation), (Some(a.as_str()), 1, Operation::Cut));
        }
        d => panic!("{d:?}"),
    }
    assert_eq!(st.model.bodies.len(), 1, "the edit previews the current result");
    st.dialog_edit(|d| {
        if let Dialog::Combine(x) = d {
            x.keep_tools = true;
        }
    });
    assert_eq!(st.model.bodies.len(), 2);
    st.dialog_cancel();
    assert_eq!(st.model.bodies.len(), 1, "cancel restores the committed combine");
    assert!(close(volume(&st), 28000.0, 1e-6));
    assert_eq!(undo_steps(&mut st), 1);
    assert_eq!(st.model.bodies.len(), 2);
}

#[test]
fn mirror_dialog_objects_then_plane() {
    let mut st = new_state();
    let a = add_box(&mut st, [0.0, 0.0], 50.0, 30.0, "10 mm");
    st.select(Pick::Body { body: a.clone() }, false);
    st.execute(CommandId::MirrorFeature);
    match st.dialog.as_ref().unwrap() {
        Dialog::Mirror(x) => {
            assert_eq!(x.bodies, vec![a.clone()]);
            assert_eq!(x.active, BodyInput::Second, "advances to the plane input");
        }
        d => panic!("{d:?}"),
    }
    assert_eq!(st.dialog_blocker().as_deref(), Some("Select a mirror plane."));
    // the +X face as mirror plane
    let p = face_with_normal(&st, &a, [1.0, 0.0, 0.0]);
    click(&mut st, p);
    assert_eq!(st.model.bodies.len(), 2, "preview copy");
    let mi_box = st.model.bodies[1].topology().bbox;
    assert!(close(mi_box[0], 50.0, 1e-6) && close(mi_box[3], 100.0, 1e-6), "{mi_box:?}");
    // an origin plane replaces it
    click(&mut st, Pick::OriginPlane(OriginPlane::YZ));
    let mi_box = st.model.bodies[1].topology().bbox;
    assert!(close(mi_box[0], -50.0, 1e-6) && close(mi_box[3], 0.0, 1e-6), "{mi_box:?}");
    st.dialog_ok();
    assert!(st.dialog.is_none(), "{:?}", st.messages.last());
    assert_eq!(st.history.undo_label(), Some("Mirror"));
    assert!(close(volume(&st), 30000.0, 1e-6));
    let mi = feature_of(&st, "Mirror");
    st.edit_feature(&mi);
    assert!(matches!(st.dialog.as_ref().unwrap(), Dialog::Mirror(x) if x.plane == Some(PlaneRef::Origin { plane: OriginPlane::YZ }) && x.bodies == vec![a.clone()]));
    st.dialog_cancel();
    assert_eq!(undo_steps(&mut st), 1);
    assert_eq!(st.model.bodies.len(), 1);
}

#[test]
fn rectangular_pattern_dialog_defaults_errors_and_edit() {
    let mut st = new_state();
    let a = add_box(&mut st, [0.0, 0.0], 50.0, 30.0, "10 mm");
    st.select(Pick::Body { body: a.clone() }, false);
    st.execute(CommandId::RectangularPattern);
    assert_eq!(st.dialog.as_ref().unwrap().title(), "RECTANGULAR PATTERN");
    // quantity 3 (counts the original), spacing = body length + 10 mm
    assert_eq!(st.model.bodies.len(), 3);
    match st.dialog.as_ref().unwrap() {
        Dialog::RectangularPattern(x) => assert_eq!(x.distance1.text, "60 mm"),
        d => panic!("{d:?}"),
    }
    assert!(close(volume(&st), 3.0 * 15000.0, 1e-6));
    // a quantity with a unit is rejected (unitless parameter) and blocks OK
    assert!(!st.dialog_set_primary("3 mm"));
    assert!(st.dialog_blocker().is_some_and(|b| b.starts_with("Quantity")));
    assert!(st.dialog_set_primary("4"));
    st.dialog_edit(|d| {
        if let Dialog::RectangularPattern(x) = d {
            x.axis2 = Some(OriginAxis::Y);
            x.quantity2.text = "2".into();
        }
    });
    assert_eq!(st.model.bodies.len(), 8, "4 x 2");
    st.dialog_ok();
    assert!(st.dialog.is_none(), "{:?}", st.messages.last());
    let rp = feature_of(&st, "RectangularPattern");
    assert_eq!(st.doc.feature(&rp).unwrap().name, "RectangularPattern1");
    st.edit_feature(&rp);
    match st.dialog.as_ref().unwrap() {
        Dialog::RectangularPattern(x) => {
            assert_eq!((x.quantity1.text.as_str(), x.distance1.text.as_str(), x.axis2), ("4", "60 mm", Some(OriginAxis::Y)));
            assert_eq!(x.distance2.text, "40 mm", "second spacing = body width + 10 mm");
        }
        d => panic!("{d:?}"),
    }
    st.dialog_edit(|d| {
        if let Dialog::RectangularPattern(x) = d {
            x.join = true;
            x.distance1.text = "25 mm".into();
            x.axis2 = None;
        }
    });
    assert_eq!(st.model.bodies.len(), 1);
    st.dialog_ok();
    assert_eq!(st.history.undo_label(), Some("Edit RectangularPattern1"));
    assert!(close(volume(&st), 125.0 * 30.0 * 10.0, 1e-6), "{}", volume(&st));
    assert_eq!(undo_steps(&mut st), 2);
    assert_eq!(st.model.bodies.len(), 1);
    assert!(close(volume(&st), 15000.0, 1e-6));
}

#[test]
fn circular_pattern_dialog_ok_and_cancel() {
    let mut st = new_state();
    let a = add_box(&mut st, [20.0, -5.0], 10.0, 10.0, "10 mm");
    st.select(Pick::Body { body: a.clone() }, false);
    st.execute(CommandId::CircularPattern);
    assert_eq!(st.model.bodies.len(), 3, "default 3 x 360 deg about Z");
    st.dialog_cancel();
    assert_eq!(st.model.bodies.len(), 1);
    assert!(!st.history.can_undo());

    st.select(Pick::Body { body: a.clone() }, false);
    st.execute(CommandId::CircularPattern);
    assert!(st.dialog_set_primary("6"));
    assert_eq!(st.model.bodies.len(), 6);
    st.dialog_ok();
    assert_eq!(st.history.undo_label(), Some("Circular Pattern"));
    assert!(close(volume(&st), 6000.0, 1e-6));
    let cp = feature_of(&st, "CircularPattern");
    st.edit_feature(&cp);
    assert!(matches!(st.dialog.as_ref().unwrap(), Dialog::CircularPattern(x) if x.quantity.text == "6" && x.angle.text == "360 deg"));
    st.dialog_cancel();
    assert_eq!(undo_steps(&mut st), 1);
}

#[test]
fn move_dialog_translate_rotate_copy() {
    let mut st = new_state();
    let a = add_box(&mut st, [0.0, 0.0], 50.0, 30.0, "10 mm");
    let p = face_with_normal(&st, &a, [0.0, 0.0, 1.0]);
    click(&mut st, p);
    st.execute(CommandId::Move);
    assert_eq!(st.dialog.as_ref().unwrap().title(), "MOVE/COPY");
    assert_eq!(st.dialog.as_ref().unwrap().input_count(), 1, "a face pick selects its body");
    assert!(st.dialog_set_primary("10 mm"));
    let bb = st.model.body(&a).unwrap().topology().bbox;
    assert!(close(bb[0], 10.0, 1e-6) && close(bb[3], 60.0, 1e-6), "{bb:?}");
    // clicking the moved body again deselects it (same body id in the preview)
    let p = face_with_normal(&st, &a, [0.0, 0.0, 1.0]);
    click(&mut st, p);
    assert_eq!(st.dialog.as_ref().unwrap().input_count(), 0);
    assert_eq!(st.dialog_blocker().as_deref(), Some("Select the bodies to move."));
    click(&mut st, Pick::Body { body: a.clone() });
    st.dialog_edit(|d| {
        if let Dialog::Move(x) = d {
            x.angle.text = "90 deg".into();
            x.copy = true;
        }
    });
    assert_eq!(st.model.bodies.len(), 2);
    st.dialog_ok();
    assert!(st.dialog.is_none(), "{:?}", st.messages.last());
    assert_eq!(st.history.undo_label(), Some("Move/Copy"));
    let mv = feature_of(&st, "Move");
    let bb = st.model.body(&mv).unwrap().topology().bbox;
    assert!(close(bb[0], -20.0, 1e-6) && close(bb[3], 10.0, 1e-6) && close(bb[4], 50.0, 1e-6), "{bb:?}");
    st.edit_feature(&mv);
    assert!(matches!(st.dialog.as_ref().unwrap(), Dialog::Move(x) if x.dx.text == "10 mm" && x.angle.text == "90 deg" && x.copy));
    st.dialog_cancel();
    assert_eq!(undo_steps(&mut st), 1);
    assert_eq!(st.model.bodies.len(), 1);
    let bb = st.model.body(&a).unwrap().topology().bbox;
    assert!(close(bb[0], 0.0, 1e-6));
}
