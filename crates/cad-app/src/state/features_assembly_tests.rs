//! Headless tests of components (New Component, activation, visibility, rename, delete) and
//! the Joint dialog (live preview, OK = one undo step, Cancel restores, edit reopens it).

use super::*;
use crate::state::command::spec;
use crate::state::{AppState, Pick};
use cad_core::{ops, JointKind, OriginPlane, PlaneRef, SurfaceKind};

/// `AppState::new()` touches OCC's global messenger; serialise construction across test threads.
static OCC_INIT: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn new_state() -> AppState {
    let _guard = OCC_INIT.lock().unwrap_or_else(|e| e.into_inner());
    AppState::new()
}

/// Adds an axis-aligned box [x0,x1] x [y0,y1] x [0,d] as a new body in the active component.
fn add_box(st: &mut AppState, a: [f64; 2], b: [f64; 2], d: &str) -> String {
    let sk = ops::new_sketch(&mut st.doc, PlaneRef::Origin { plane: OriginPlane::XY });
    ops::sketch_mut(&mut st.doc, &sk).unwrap().add_rectangle(a, b);
    st.recompute();
    let p = &st.model.sketches[&sk].profiles[0];
    let prof = vec![ProfileRef { curves: p.curves.clone(), sample: p.interior_point() }];
    let ext = ops::new_extrude(&mut st.doc, &sk, prof, d, ExtentDirection::Normal, Operation::New);
    st.recompute();
    assert_eq!(st.model.health_of(&ext), Health::Healthy);
    ext
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

/// Canvas click on the planar face of `body` whose normal is `n`.
fn click_face(st: &mut AppState, body: &str, n: [f64; 3]) {
    let p = face_with_normal(st, body, n);
    click(st, p);
}

fn bbox(st: &AppState, body: &str) -> [f64; 6] {
    st.model.body(body).unwrap().topology().bbox
}

fn assert_bbox(st: &AppState, body: &str, expect: [f64; 6]) {
    let b = bbox(st, body);
    for (x, y) in b.iter().zip(expect) {
        assert!((x - y).abs() <= 1e-6, "{body}: bbox {b:?}, expected {expect:?}");
    }
}

fn visible_ids(st: &AppState) -> Vec<String> {
    st.visible_bodies().iter().map(|b| b.id.clone()).collect()
}

/// A (50 x 30 x 20) in Component1 and B (20 x 10 x 10) in Component2, created through the
/// New Component command.
fn two_components(st: &mut AppState) -> (String, String, String, String) {
    st.execute(CommandId::NewComponent);
    let c1 = st.doc.active_component.clone().expect("New Component activates the component");
    let a = add_box(st, [0.0, 0.0], [50.0, 30.0], "20 mm");
    st.execute(CommandId::NewComponent);
    let c2 = st.doc.active_component.clone().unwrap();
    let b = add_box(st, [100.0, 50.0], [120.0, 60.0], "10 mm");
    (c1, c2, a, b)
}

#[test]
fn assemble_commands_are_supported_and_gated_by_sketch_mode() {
    let st = new_state();
    for id in [CommandId::NewComponent, CommandId::Joint] {
        assert!(spec(id).supported, "{id:?}");
        assert!(st.is_enabled(id), "{id:?}");
    }
    assert_eq!(spec(CommandId::Joint).shortcut, Some("J"));
}

#[test]
fn new_component_activates_and_owns_new_features() {
    let mut st = new_state();
    let root_box = add_box(&mut st, [-40.0, 0.0], [-20.0, 10.0], "5 mm");
    st.execute(CommandId::NewComponent);
    assert_eq!(st.doc.components.len(), 1);
    let c1 = st.doc.components[0].id.clone();
    assert_eq!(st.doc.components[0].name, "Component1");
    assert_eq!(st.doc.active_component.as_deref(), Some(c1.as_str()));
    assert_eq!(st.history.undo_label(), Some("New Component"));
    let a = add_box(&mut st, [0.0, 0.0], [10.0, 10.0], "10 mm");
    assert_eq!(st.doc.feature(&a).unwrap().component.as_deref(), Some(c1.as_str()), "features go into the active component");
    assert_eq!(st.component_of_body(&a).as_deref(), Some(c1.as_str()));
    assert_eq!(st.component_of_body(&root_box), None);

    // activating the root again: new features go to the root
    st.activate_component(None);
    assert_eq!(st.doc.active_component, None);
    let r2 = add_box(&mut st, [0.0, 40.0], [10.0, 50.0], "10 mm");
    assert_eq!(st.component_of_body(&r2), None);
    st.activate_component(Some(&c1));
    assert_eq!(st.doc.active_component.as_deref(), Some(c1.as_str()));

    // New Component with a body selected moves it into the new component
    st.select(Pick::Body { body: root_box.clone() }, false);
    st.execute(CommandId::NewComponent);
    let c2 = st.doc.active_component.clone().unwrap();
    assert_ne!(c2, c1);
    assert_eq!(st.component_of_body(&root_box).as_deref(), Some(c2.as_str()));
    assert_eq!(st.doc.component(&c2).unwrap().name, "Component2");
    // ... in one undo step
    st.undo();
    assert_eq!(st.component_of_body(&root_box), None);
    assert_eq!(st.doc.components.len(), 1);
    assert_eq!(st.doc.active_component.as_deref(), Some(c1.as_str()));

    // rename
    st.rename_component(&c1, "Bracket");
    assert_eq!(st.doc.component(&c1).unwrap().name, "Bracket");

    // delete: refused while the component owns features, with a message
    assert!(!st.delete_component(&c1));
    assert!(st.doc.component(&c1).is_some());
    let msg = st.last_message().unwrap();
    assert_eq!(msg.level, crate::state::Level::Error);
    assert!(msg.text.contains("cannot be deleted"), "{}", msg.text);
    // an empty one can be deleted (the active component falls back to the root)
    st.execute(CommandId::NewComponent);
    let empty = st.doc.active_component.clone().unwrap();
    assert!(st.delete_component(&empty));
    assert!(st.doc.component(&empty).is_none());
    assert_eq!(st.doc.active_component, None);
}

#[test]
fn hiding_a_component_hides_its_bodies() {
    let mut st = new_state();
    let root_box = add_box(&mut st, [-40.0, 0.0], [-20.0, 10.0], "5 mm");
    let (c1, _c2, a, b) = two_components(&mut st);
    assert_eq!(visible_ids(&st).len(), 3);
    st.set_component_visible(&c1, false);
    assert_eq!(visible_ids(&st), vec![root_box.clone(), b.clone()], "{a} is hidden with its component");
    // the body itself keeps its own visibility flag
    assert!(st.doc.bodies.get(&a).is_none_or(|m| m.visible));
    st.undo();
    assert_eq!(visible_ids(&st).len(), 3);
    // bodies hidden individually stay hidden when the component is shown
    st.set_body_visible(&b, false);
    assert_eq!(visible_ids(&st), vec![root_box, a]);
}

#[test]
fn joint_dialog_preview_ok_cancel_and_edit() {
    let mut st = new_state();
    let (c1, c2, a, b) = two_components(&mut st);
    let n_features = st.doc.features.len();
    let undo_before = st.history.undo_label().map(String::from);

    // ---- cancel restores
    st.execute(CommandId::Joint);
    assert!(matches!(st.dialog, Some(Dialog::Joint(_))));
    assert_eq!(st.dialog.as_ref().unwrap().title(), "JOINT");
    assert!(st.dialog_blocker().is_some(), "no inputs yet");
    click_face(&mut st, &a, [0.0, 0.0, 1.0]);
    match &st.dialog {
        Some(Dialog::Joint(x)) => {
            assert_eq!(x.a.as_ref().and_then(|o| o.component.as_deref()), Some(c1.as_str()));
            assert_eq!(x.active, BodyInput::Second, "advances to the second input");
            assert!(x.s.feature.is_none(), "no preview with one input");
        }
        _ => panic!("joint dialog"),
    }
    click_face(&mut st, &b, [0.0, 0.0, -1.0]);
    match &st.dialog {
        Some(Dialog::Joint(x)) => {
            assert_eq!(x.b.as_ref().and_then(|o| o.component.as_deref()), Some(c2.as_str()));
            assert!(x.s.feature.is_some());
        }
        _ => panic!("joint dialog"),
    }
    assert_eq!(st.dialog_blocker(), None);
    // live preview (F35): the joint Z axes align, B goes just below A's top face
    assert_bbox(&st, &b, [15.0, 10.0, 10.0, 35.0, 20.0, 20.0]);
    assert_eq!(st.dialog_status().as_deref(), Some("Component1 (fixed) <- Component2 (moves)"));
    st.dialog_cancel();
    assert!(st.dialog.is_none());
    assert_eq!(st.doc.features.len(), n_features);
    assert_bbox(&st, &b, [100.0, 50.0, 0.0, 120.0, 60.0, 10.0]);
    assert_eq!(st.history.undo_label().map(String::from), undo_before, "cancel records nothing");

    // ---- OK = one undo step
    st.execute(CommandId::Joint);
    click_face(&mut st, &a, [0.0, 0.0, 1.0]);
    click_face(&mut st, &b, [0.0, 0.0, -1.0]);
    st.dialog_edit(|d| {
        if let Dialog::Joint(x) = d {
            x.kind = JointKind::Slider;
            x.offset.text = "15 mm".into();
        }
    });
    assert_bbox(&st, &b, [15.0, 10.0, 25.0, 35.0, 20.0, 35.0]);
    // an invalid expression blocks OK and keeps the last valid value
    assert!(!st.dialog_set_primary("15 deg"), "a slider's primary value is a length");
    assert!(st.dialog_blocker().is_some());
    assert!(st.dialog_set_primary("15 mm"));
    st.dialog_ok();
    assert!(st.dialog.is_none());
    let j = st.doc.features.iter().find(|f| f.type_label() == "Joint").unwrap().id.clone();
    assert_eq!(st.doc.feature(&j).unwrap().name, "Joint1");
    assert!(!st.doc.feature(&j).unwrap().suppressed);
    assert_eq!(st.history.undo_label(), Some("Joint"));
    assert_eq!(st.health(&j), Health::Healthy);
    assert_bbox(&st, &b, [15.0, 10.0, 25.0, 35.0, 20.0, 35.0]);

    // ---- the joint's values are model parameters (Change Parameters drives the motion)
    let offset = st.doc.feature(&j).unwrap().parameter_names()[1].to_string();
    assert!(st.set_parameter(&offset, "5 mm"));
    assert_bbox(&st, &b, [15.0, 10.0, 15.0, 35.0, 20.0, 25.0]);
    st.undo();
    assert_bbox(&st, &b, [15.0, 10.0, 25.0, 35.0, 20.0, 35.0]);

    // ---- edit reopens the dialog with the inputs
    st.edit_feature(&j);
    match &st.dialog {
        Some(Dialog::Joint(x)) => {
            assert!(x.a.is_some() && x.b.is_some());
            assert_eq!(x.kind, JointKind::Slider);
            assert_eq!(x.offset.text, "15 mm");
        }
        _ => panic!("edit opens the joint dialog"),
    }
    assert_eq!(st.dialog.as_ref().unwrap().title(), "EDIT FEATURE");
    st.dialog_edit(|d| {
        if let Dialog::Joint(x) = d {
            x.kind = JointKind::Revolute;
            x.offset.text = "0 mm".into();
            x.angle.text = "90 deg".into();
        }
    });
    st.dialog_ok();
    assert_eq!(st.history.undo_label(), Some("Edit Joint1"));
    // B (20 x 10) rotated 90 deg about A's top-face normal through its centre
    assert_bbox(&st, &b, [20.0, 5.0, 10.0, 30.0, 25.0, 20.0]);
    st.undo();
    assert_bbox(&st, &b, [15.0, 10.0, 25.0, 35.0, 20.0, 35.0]);
}

#[test]
fn joint_dialog_refuses_two_origins_on_one_component() {
    let mut st = new_state();
    let (c1, _c2, a, _b) = two_components(&mut st);
    st.activate_component(Some(&c1));
    let a2 = add_box(&mut st, [0.0, 50.0], [10.0, 60.0], "5 mm");
    st.execute(CommandId::Joint);
    click_face(&mut st, &a, [0.0, 0.0, 1.0]);
    click_face(&mut st, &a2, [0.0, 0.0, -1.0]);
    match &st.dialog {
        Some(Dialog::Joint(x)) => {
            assert!(x.b.is_none(), "same component refused");
            assert!(x.s.notice.as_deref().is_some_and(|n| n.contains("different component")), "{:?}", x.s.notice);
        }
        _ => panic!("joint dialog"),
    }
    // clicking the selected origin again clears it
    click_face(&mut st, &a, [0.0, 0.0, 1.0]);
    match &st.dialog {
        Some(Dialog::Joint(x)) => assert!(x.a.is_none() && x.active == BodyInput::First),
        _ => panic!("joint dialog"),
    }
    st.dialog_cancel();
}

#[test]
fn joint_preview_guards_repicks_and_keeps_user_suppression() {
    let mut st = new_state();
    let (_c1, _c2, a, b) = two_components(&mut st);
    st.execute(CommandId::Joint);
    click_face(&mut st, &a, [0.0, 0.0, 1.0]);
    click_face(&mut st, &b, [0.0, 0.0, -1.0]);
    let before = match &st.dialog {
        Some(Dialog::Joint(x)) => x.b.clone(),
        _ => panic!("joint dialog"),
    };
    // B is moved by the preview: picking another of its faces is refused with a hint
    click_face(&mut st, &b, [1.0, 0.0, 0.0]);
    match &st.dialog {
        Some(Dialog::Joint(x)) => {
            assert_eq!(x.b, before, "the moved component is not re-picked");
            assert!(x.s.notice.as_deref().is_some_and(|n| n.contains("moved by the joint preview")), "{:?}", x.s.notice);
        }
        _ => panic!("joint dialog"),
    }
    st.dialog_ok();
    let j = st.doc.features.iter().find(|f| f.type_label() == "Joint").unwrap().id.clone();
    // a suppressed joint stays suppressed through Edit + OK
    st.toggle_suppress(&j);
    assert_bbox(&st, &b, [100.0, 50.0, 0.0, 120.0, 60.0, 10.0]);
    st.edit_feature(&j);
    st.dialog_ok();
    assert!(st.doc.feature(&j).unwrap().suppressed);
    assert_bbox(&st, &b, [100.0, 50.0, 0.0, 120.0, 60.0, 10.0]);
}

#[test]
fn joint_dialog_refuses_an_over_constraining_second_joint() {
    let mut st = new_state();
    let (_c1, _c2, a, b) = two_components(&mut st);
    st.execute(CommandId::Joint);
    click_face(&mut st, &a, [0.0, 0.0, 1.0]);
    click_face(&mut st, &b, [0.0, 0.0, -1.0]);
    st.dialog_ok();
    let j1 = st.doc.features.iter().find(|f| f.type_label() == "Joint").unwrap().id.clone();
    let n = st.doc.features.len();
    // F37: a second joint between the same two components
    st.execute(CommandId::Joint);
    click_face(&mut st, &a, [1.0, 0.0, 0.0]);
    click_face(&mut st, &b, [1.0, 0.0, 0.0]);
    match &st.dialog {
        Some(Dialog::Joint(x)) => assert!(x.a.is_some() && x.b.is_some()),
        _ => panic!("joint dialog"),
    }
    assert_eq!(st.dialog_blocker().as_deref(), Some(cad_kernel::OVER_CONSTRAINED));
    assert_eq!(st.dialog_status().as_deref(), Some(cad_kernel::OVER_CONSTRAINED));
    st.dialog_ok();
    assert!(st.dialog.is_some(), "OK is refused");
    let msg = st.last_message().unwrap();
    assert_eq!((msg.level, msg.text.as_str()), (crate::state::Level::Error, cad_kernel::OVER_CONSTRAINED));
    st.dialog_cancel();
    assert_eq!(st.doc.features.len(), n);
    // the first joint stays healthy and in place
    assert_eq!(st.health(&j1), Health::Healthy);
    assert_bbox(&st, &b, [15.0, 10.0, 10.0, 35.0, 20.0, 20.0]);
}

#[test]
fn root_geometry_can_be_either_origin_and_grounding_decides_the_mover() {
    let mut st = new_state();
    let r = add_box(&mut st, [0.0, 0.0], [10.0, 10.0], "10 mm");
    st.execute(CommandId::NewComponent);
    let c = add_box(&mut st, [50.0, 0.0], [60.0, 10.0], "10 mm");
    // origin 1 on the component, origin 2 on the root: accepted, the component moves
    st.execute(CommandId::Joint);
    click_face(&mut st, &c, [0.0, 0.0, 1.0]);
    click_face(&mut st, &r, [0.0, 0.0, -1.0]);
    match &st.dialog {
        Some(Dialog::Joint(x)) => assert!(x.b.is_some(), "{:?}", x.s.notice),
        _ => panic!("joint dialog"),
    }
    let root = st.doc.component_name(None);
    assert_eq!(st.dialog_status(), Some(format!("{root} (fixed) <- Component1 (moves)")));
    assert_bbox(&st, &r, [0.0, 0.0, 0.0, 10.0, 10.0, 10.0]);
    assert_bbox(&st, &c, [0.0, 0.0, 0.0, 10.0, 10.0, 10.0]);
    st.dialog_ok();
    assert!(st.dialog.is_none());

    // two components: grounding the later one makes the earlier one move (one undo step)
    let mut st = new_state();
    let (_c1, c2, a, b) = two_components(&mut st);
    st.set_component_grounded(&c2, true);
    assert_eq!(st.history.undo_label(), Some("Ground"));
    assert!(st.doc.component(&c2).unwrap().grounded);
    st.execute(CommandId::Joint);
    click_face(&mut st, &a, [0.0, 0.0, 1.0]);
    click_face(&mut st, &b, [0.0, 0.0, -1.0]);
    assert_eq!(st.dialog_status().as_deref(), Some("Component2 (fixed) <- Component1 (moves)"));
    assert_bbox(&st, &b, [100.0, 50.0, 0.0, 120.0, 60.0, 10.0]);
    assert_bbox(&st, &a, [85.0, 40.0, 0.0, 135.0, 70.0, 20.0]);
    st.dialog_ok();
    // undo removes the joint, then the grounding
    st.undo();
    st.undo();
    assert!(!st.doc.component(&c2).unwrap().grounded);
    assert!(st.doc.features.iter().all(|f| f.type_label() != "Joint"));
}
