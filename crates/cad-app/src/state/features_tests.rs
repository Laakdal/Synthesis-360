//! Headless workflow tests of the feature dialogs (values from the Fusion 2705.1.30 probe).

use super::*;
use crate::state::{AppState, Pick};
use cad_core::sketch::{ConstraintKind as K, DistanceOrientation as O, EntityKind, ORIGIN};
use cad_core::{ops, CurveKind, OriginPlane, PlaneRef};
use cad_kernel::refs;

/// `AppState::new()` runs OCC's `cad_quiet()`, which mutates OCC's global messenger and is not
/// thread-safe (parallel test threads crashed with STATUS_HEAP_CORRUPTION); serialise it.
static OCC_INIT: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn new_state() -> AppState {
    let _guard = OCC_INIT.lock().unwrap_or_else(|e| e.into_inner());
    AppState::new()
}

/// Rectangle with H/V constraints, corner on the origin and width/height dimensions.
/// Returns (sketch id, width parameter, height parameter).
fn rectangle(st: &mut AppState, plane: PlaneRef, w: &str, h: &str) -> (String, String, String) {
    let doc = &mut st.doc;
    let sk = ops::new_sketch(doc, plane);
    let s = ops::sketch_mut(doc, &sk).unwrap();
    let lines = s.add_rectangle([1.0, 1.0], [40.0, 20.0]);
    let corner = match &s.entity(&lines[0]).unwrap().kind {
        EntityKind::Line { p1, .. } => p1.clone(),
        _ => unreachable!(),
    };
    s.add_constraint(K::Coincident { a: corner, b: ORIGIN.into() });
    let (_, pw) = ops::add_dimension(doc, &sk, K::Distance { a: lines[0].clone(), b: lines[0].clone(), orientation: O::Horizontal }, w, [20.0, -8.0], false).unwrap();
    let (_, ph) = ops::add_dimension(doc, &sk, K::Distance { a: lines[1].clone(), b: lines[1].clone(), orientation: O::Vertical }, h, [48.0, 10.0], false).unwrap();
    st.recompute();
    (sk, pw, ph)
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

/// Sketch + extrude through the dialog, like a user would. Returns (state, sketch, width param, extrude id).
fn box_via_dialog(w: &str, h: &str, d: &str) -> (AppState, String, String, String) {
    let mut st = new_state();
    let (sk, pw, _) = rectangle(&mut st, PlaneRef::Origin { plane: OriginPlane::XY }, w, h);
    st.select(Pick::Profile { sketch: sk.clone(), index: 0 }, false);
    st.execute(CommandId::Extrude);
    assert!(st.dialog_set_primary(d));
    st.dialog_ok();
    assert!(st.dialog.is_none(), "{:?}", st.messages.last());
    let ext = st.doc.features.iter().find(|f| matches!(f.kind, FeatureKind::Extrude { .. })).unwrap().id.clone();
    (st, sk, pw, ext)
}

fn vertical_edges(st: &AppState) -> Vec<Pick> {
    let b = &st.model.bodies[0];
    let t = b.topology();
    t.edge_info.iter().enumerate().filter(|(_, e)| e.curve == CurveKind::Line && e.dir[2].abs() > 0.999).map(|(i, _)| Pick::Edge { body: b.id.clone(), edge: i }).collect()
}

fn face_with_normal(st: &AppState, n: [f64; 3]) -> Pick {
    let b = &st.model.bodies[0];
    let t = b.topology();
    let i = t.face_info.iter().position(|f| f.surface == cad_core::SurfaceKind::Plane && cad_kernel::frame::dot(f.normal, n) > 0.999).unwrap();
    Pick::Face { body: b.id.clone(), face: i }
}

#[test]
fn extrude_dialog_creates_one_undo_step_and_fusion_volume() {
    let mut st = new_state();
    let (sk, _, _) = rectangle(&mut st, PlaneRef::Origin { plane: OriginPlane::XY }, "50 mm", "30 mm");
    st.select(Pick::Profile { sketch: sk.clone(), index: 0 }, false);
    st.execute(CommandId::Extrude);
    let d = st.dialog.as_ref().expect("extrude dialog");
    assert_eq!(d.title(), "EXTRUDE");
    match d {
        Dialog::Extrude(x) => {
            assert_eq!(x.profiles.len(), 1);
            assert_eq!(x.operation, Operation::New, "no bodies yet -> New Body");
        }
        _ => panic!(),
    }
    assert_eq!(st.dialog_status().as_deref(), Some("1 Profile | Area : 1500.00 mm^2"));
    assert!(st.in_txn());
    assert!(st.dialog_set_primary("10 mm"));
    // live preview: the body exists while the dialog is open
    assert!(close(volume(&st), 15000.0, 1e-6), "{}", volume(&st));
    st.dialog_ok();
    assert!(st.dialog.is_none());
    assert!(!st.in_txn());
    assert!(close(volume(&st), 15000.0, 1e-6));
    assert_eq!(st.history.undo_label(), Some("Extrude"));
    assert!(st.selection.is_empty());
    // auto-hidden sketch (Fusion default)
    assert!(matches!(st.doc.feature(&sk).unwrap().kind, FeatureKind::Sketch { visible: false, .. }));
    assert_eq!(undo_steps(&mut st), 1, "the whole dialog session is one undo step");
    assert!(st.model.bodies.is_empty());
    assert_eq!(st.doc.features.len(), 1, "only the sketch remains");
}

#[test]
fn extrude_cancel_leaves_no_feature() {
    let mut st = new_state();
    let (sk, _, _) = rectangle(&mut st, PlaneRef::Origin { plane: OriginPlane::XY }, "50 mm", "30 mm");
    let params = st.doc.parameters.len();
    st.select(Pick::Profile { sketch: sk, index: 0 }, false);
    st.execute(CommandId::Extrude);
    st.dialog_set_primary("25 mm");
    assert_eq!(st.model.bodies.len(), 1, "preview body");
    st.dialog_cancel();
    assert!(st.dialog.is_none() && !st.in_txn());
    assert_eq!(st.doc.features.len(), 1);
    assert_eq!(st.doc.parameters.len(), params, "no leftover model parameter");
    assert!(st.model.bodies.is_empty());
    assert!(!st.history.can_undo());
}

#[test]
fn profiles_toggle_by_clicking_while_open() {
    let mut st = new_state();
    let (sk, _, _) = rectangle(&mut st, PlaneRef::Origin { plane: OriginPlane::XY }, "50 mm", "30 mm");
    st.execute(CommandId::Extrude);
    assert_eq!(st.dialog_blocker().as_deref(), Some("Select profiles in the canvas."));
    assert!(st.model.bodies.is_empty());
    // a canvas click: hover is the clicked item
    let p = Pick::Profile { sketch: sk.clone(), index: 0 };
    st.hover = Some(p.clone());
    st.select(p.clone(), false);
    assert_eq!(st.dialog.as_ref().unwrap().input_count(), 1);
    assert_eq!(st.model.bodies.len(), 1);
    // clicking it again removes it
    st.select(p.clone(), false);
    assert_eq!(st.dialog.as_ref().unwrap().input_count(), 0);
    assert!(st.dialog_blocker().is_some());
    st.dialog_ok();
    assert!(st.dialog.is_some(), "OK is refused without profiles");
}

#[test]
fn invalid_expression_keeps_dialog_open_with_error() {
    let mut st = new_state();
    let (sk, _, _) = rectangle(&mut st, PlaneRef::Origin { plane: OriginPlane::XY }, "50 mm", "30 mm");
    st.select(Pick::Profile { sketch: sk, index: 0 }, false);
    st.execute(CommandId::Extrude);
    assert!(st.dialog_set_primary("10 mm"));
    assert!(!st.dialog_set_primary("foo * 2"));
    let Some(Dialog::Extrude(x)) = &st.dialog else { panic!() };
    assert!(x.distance.error.is_some());
    assert_eq!(x.distance.text, "foo * 2", "the invalid text stays in the box");
    assert!(close(volume(&st), 15000.0, 1e-6), "the last valid value stays in effect");
    st.dialog_ok();
    assert!(st.dialog.is_some(), "OK refused while the expression is invalid");
    // expressions referencing parameters are accepted
    let w = st.doc.parameters[0].name.clone();
    assert!(st.dialog_set_primary(&format!("{w} / 5")));
    st.dialog_ok();
    assert!(st.dialog.is_none());
    assert!(close(volume(&st), 15000.0, 1e-6));
}

#[test]
fn zero_distance_reports_kernel_error() {
    let mut st = new_state();
    let (sk, _, _) = rectangle(&mut st, PlaneRef::Origin { plane: OriginPlane::XY }, "50 mm", "30 mm");
    st.select(Pick::Profile { sketch: sk, index: 0 }, false);
    st.execute(CommandId::Extrude);
    st.dialog_set_primary("0 mm");
    assert_eq!(st.dialog_blocker().as_deref(), Some("Distance must not be zero."));
    st.dialog_ok();
    assert!(st.dialog.is_some());
    st.dialog_cancel();
}

#[test]
fn edit_extrude_distance_via_edit_feature() {
    let (mut st, _, _, ext) = box_via_dialog("50 mm", "30 mm", "10 mm");
    st.edit_feature(&ext);
    let d = st.dialog.as_ref().expect("edit dialog");
    assert_eq!(d.title(), "EDIT FEATURE");
    let Dialog::Extrude(x) = d else { panic!() };
    assert_eq!(x.distance.text, "10 mm");
    assert_eq!(x.profiles.len(), 1);
    assert!(st.dialog_set_primary("20 mm"));
    assert!(close(volume(&st), 30000.0, 1e-6));
    st.dialog_ok();
    assert!(close(volume(&st), 30000.0, 1e-6));
    assert_eq!(st.history.undo_label(), Some("Edit Extrude1"));
    st.undo();
    assert!(close(volume(&st), 15000.0, 1e-6));
    // cancel of an edit restores
    st.edit_feature(&ext);
    st.dialog_set_primary("40 mm");
    assert!(close(volume(&st), 60000.0, 1e-6));
    st.dialog_cancel();
    assert!(close(volume(&st), 15000.0, 1e-6));
}

#[test]
fn edit_rolls_back_downstream_features_and_restores_marker() {
    let (mut st, _, _, ext) = box_via_dialog("60 mm", "30 mm", "20 mm");
    for e in vertical_edges(&st) {
        st.select(e, true);
    }
    st.execute(CommandId::Fillet);
    st.dialog_set_primary("5 mm");
    st.dialog_ok();
    let n = st.doc.features.len();
    assert_eq!(st.doc.marker, n);
    st.edit_feature(&ext);
    assert_eq!(st.doc.marker, 2, "the fillet is rolled back while the extrude is edited");
    st.dialog_set_primary("30 mm");
    st.dialog_ok();
    assert_eq!(st.doc.marker, n);
    let v = volume(&st);
    assert!(close(v, 35570.8 / 20.0 * 30.0, 0.2), "fillet re-applies on the taller box: {v}");
}

#[test]
fn second_extrude_defaults_to_join_and_cut_through_all() {
    let (mut st, _, _, _) = box_via_dialog("50 mm", "30 mm", "10 mm");
    let top = match face_with_normal(&st, [0.0, 0.0, 1.0]) {
        Pick::Face { body, face } => refs::face_ref(st.model.body(&body).unwrap(), face).unwrap(),
        _ => unreachable!(),
    };
    let sk2 = ops::new_sketch(&mut st.doc, PlaneRef::Face { face: top });
    ops::sketch_mut(&mut st.doc, &sk2).unwrap().add_circle([25.0, 15.0], 6.0);
    st.recompute();
    let disc = st.model.sketches[&sk2].profiles.iter().position(|p| p.area < 200.0).unwrap();
    st.select(Pick::Profile { sketch: sk2, index: disc }, false);
    st.execute(CommandId::Extrude);
    let Some(Dialog::Extrude(x)) = &st.dialog else { panic!() };
    assert_eq!(x.operation, Operation::Join, "bodies exist -> Join");
    st.dialog_edit(|d| {
        if let Dialog::Extrude(x) = d {
            x.operation = Operation::Cut;
            x.through_all = true;
        }
    });
    // One Side points away from the body: Fusion's warning
    let s = st.dialog.as_ref().unwrap().session().unwrap().health.clone();
    assert!(matches!(&s, Health::Warning(w) if w.contains("No target body")), "{s:?}");
    st.dialog_edit(|d| {
        if let Dialog::Extrude(x) = d {
            x.direction = ExtentDirection::Reverse;
        }
    });
    assert_eq!(st.dialog.as_ref().unwrap().session().unwrap().health, Health::Healthy);
    st.dialog_ok();
    let expect = 15000.0 - std::f64::consts::PI * 36.0 * 10.0;
    assert!(close(volume(&st), expect, 0.2), "{} vs {expect}", volume(&st));
}

#[test]
fn fillet_four_vertical_edges_matches_fusion() {
    let (mut st, _, _, _) = box_via_dialog("60 mm", "30 mm", "20 mm");
    let edges = vertical_edges(&st);
    assert_eq!(edges.len(), 4);
    for e in &edges[..3] {
        st.select(e.clone(), true);
    }
    st.execute(CommandId::Fillet);
    assert_eq!(st.dialog.as_ref().unwrap().input_count(), 3);
    assert!(st.dialog_set_primary("5 mm"));
    // add the fourth edge by clicking it on the (filleted) preview
    let last = match &edges[3] {
        Pick::Edge { body, .. } => {
            // the same edge in the preview topology
            let base = st.dialog.as_ref().unwrap().session().unwrap().base.0.clone();
            let r = match &edges[3] {
                Pick::Edge { edge, .. } => refs::edge_ref(base.body(body).unwrap(), *edge).unwrap(),
                _ => unreachable!(),
            };
            let i = refs::resolve_edge(st.model.body(body).unwrap(), &r).unwrap();
            Pick::Edge { body: body.clone(), edge: i }
        }
        _ => unreachable!(),
    };
    st.hover = Some(last.clone());
    st.select(last, false);
    assert_eq!(st.dialog.as_ref().unwrap().input_count(), 4);
    assert_eq!(st.dialog_status().as_deref(), Some("4 Edges"));
    st.dialog_ok();
    assert!(st.dialog.is_none(), "{:?}", st.messages.last());
    assert!(close(volume(&st), 35570.8, 0.1), "{}", volume(&st));
    assert_eq!(st.history.undo_label(), Some("Fillet"));
}

#[test]
fn fillet_too_large_shows_kernel_error() {
    let (mut st, _, _, _) = box_via_dialog("50 mm", "30 mm", "20 mm");
    for e in vertical_edges(&st) {
        st.select(e, true);
    }
    st.execute(CommandId::Fillet);
    st.dialog_set_primary("40 mm");
    let h = st.dialog.as_ref().unwrap().session().unwrap().health.clone();
    assert!(h.is_error(), "{h:?}");
    assert!(st.dialog_blocker().is_some());
    st.dialog_ok();
    assert!(st.dialog.is_some());
    assert!(close(volume(&st), 30000.0, 1e-6), "the failed fillet leaves the body unchanged");
    st.dialog_cancel();
}

#[test]
fn chamfer_dialog() {
    let (mut st, _, _, _) = box_via_dialog("50 mm", "30 mm", "10 mm");
    for e in vertical_edges(&st) {
        st.select(e, true);
    }
    st.execute(CommandId::Chamfer);
    assert_eq!(st.dialog.as_ref().unwrap().title(), "CHAMFER");
    st.dialog_set_primary("2 mm");
    st.dialog_ok();
    assert!(close(volume(&st), 15000.0 - 4.0 * 2.0 * 10.0, 1e-3), "{}", volume(&st));
}

#[test]
fn hole_through_all_at_face_centre() {
    let (mut st, _, _, _) = box_via_dialog("50 mm", "30 mm", "10 mm");
    let top = face_with_normal(&st, [0.0, 0.0, 1.0]);
    st.select(top, false);
    st.execute(CommandId::Hole);
    let Some(Dialog::Hole(x)) = &st.dialog else { panic!() };
    assert!(close(x.centroid[0], 25.0, 1e-6) && close(x.centroid[1], 15.0, 1e-6), "{:?}", x.centroid);
    st.dialog_edit(|d| {
        if let Dialog::Hole(x) = d {
            x.diameter.text = "10 mm".into();
            x.through_all = true;
            x.offset_x.text = "5 mm".into();
        }
    });
    st.dialog_ok();
    assert!(st.dialog.is_none(), "{:?}", st.messages.last());
    let expect = 15000.0 - std::f64::consts::PI * 25.0 * 10.0;
    assert!(close(volume(&st), expect, 0.2), "{} vs {expect}", volume(&st));
    let hole = st.doc.features.last().unwrap();
    assert!(matches!(&hole.kind, FeatureKind::Hole { center, .. } if close(center[0], 30.0, 1e-6)));
}

#[test]
fn revolve_rectangle_about_x_axis() {
    let mut st = new_state();
    let (sk, _, _) = rectangle(&mut st, PlaneRef::Origin { plane: OriginPlane::XY }, "50 mm", "30 mm");
    st.select(Pick::Profile { sketch: sk.clone(), index: 0 }, false);
    st.execute(CommandId::Revolve);
    let Some(Dialog::Revolve(x)) = &st.dialog else { panic!() };
    assert_eq!(x.axis, Some(RevolveAxis::Origin { axis: cad_core::OriginAxis::X }));
    assert_eq!(x.angle.text, "360 deg");
    let choices = st.revolve_axis_choices(Some(&sk), &x.s.base.0);
    assert!(choices.len() >= 7, "3 origin axes + 4 lines");
    st.dialog_ok();
    let expect = std::f64::consts::PI * 900.0 * 50.0;
    assert!(close(volume(&st), expect, 0.5), "{} vs {expect}", volume(&st));
    assert_eq!(st.history.undo_label(), Some("Revolve"));
}

#[test]
fn offset_plane_from_origin_plane() {
    let mut st = new_state();
    st.select(Pick::OriginPlane(OriginPlane::XY), false);
    st.execute(CommandId::OffsetPlane);
    assert!(st.dialog_set_primary("15 mm"));
    st.dialog_ok();
    let id = st.doc.features.last().unwrap().id.clone();
    assert!(close(st.model.planes[&id].origin[2], 15.0, 1e-9));
    st.edit_feature(&id);
    assert!(st.dialog_set_primary("-5 mm"));
    st.dialog_ok();
    assert!(close(st.model.planes[&id].origin[2], -5.0, 1e-9));
}

#[test]
fn change_parameters_drives_the_body() {
    let (mut st, _, pw, _) = box_via_dialog("50 mm", "30 mm", "10 mm");
    st.execute(CommandId::ChangeParameters);
    assert!(matches!(st.dialog, Some(Dialog::Parameters(_))));
    assert!(!st.in_txn(), "Change Parameters applies edits immediately");
    assert!(st.parameters_set(&pw, "60 mm"));
    assert!(close(volume(&st), 18000.0, 1e-6));
    assert_eq!(st.history.undo_label(), Some(format!("Change Parameter {pw}").as_str()));
    // invalid -> per-row error, value kept
    assert!(!st.parameters_set(&pw, "15 deg"));
    let Some(Dialog::Parameters(p)) = &st.dialog else { panic!() };
    assert!(p.errors.contains_key(&pw));
    assert!(close(volume(&st), 18000.0, 1e-6));
    // user parameter driving the width
    st.dialog_edit(|d| {
        if let Dialog::Parameters(p) = d {
            p.new_name = "width".into();
            p.new_expression = "70 mm".into();
        }
    });
    assert!(st.parameters_add_user());
    assert!(st.parameters_set(&pw, "width"));
    assert!(close(volume(&st), 21000.0, 1e-6));
    assert!(!st.parameters_delete("width"), "in use");
    st.dialog_ok();
    assert!(st.dialog.is_none());
    st.undo();
    assert!(close(volume(&st), 18000.0, 1e-6));
}

#[test]
fn measure_reports_volume_area_length_and_distance() {
    let (mut st, _, _, ext) = box_via_dialog("50 mm", "30 mm", "10 mm");
    let body = st.model.bodies[0].id.clone();
    assert_eq!(body, ext);
    st.select(Pick::Body { body: body.clone() }, false);
    st.execute(CommandId::Measure);
    let r = st.measure_report();
    assert_eq!(r.items.len(), 1);
    assert!(close(r.items[0].volume.unwrap(), 15000.0, 1e-6));
    assert!(close(r.items[0].surface_area.unwrap(), 4600.0, 1e-6));
    // pick two faces by clicking: top and bottom are 10 mm apart
    let top = face_with_normal(&st, [0.0, 0.0, 1.0]);
    let bottom = face_with_normal(&st, [0.0, 0.0, -1.0]);
    st.hover = Some(top.clone());
    st.select(top, false);
    // a body + a face: the first two picks
    let r = st.measure_report();
    assert_eq!(r.items.len(), 2);
    st.hover = Some(bottom.clone());
    st.select(bottom, false);
    let r = st.measure_report();
    assert_eq!(r.items.len(), 1, "a third pick starts over");
    assert!(close(r.items[0].area.unwrap(), 1500.0, 1e-6));
    let top = face_with_normal(&st, [0.0, 0.0, 1.0]);
    st.hover = Some(top.clone());
    st.select(top, false);
    let r = st.measure_report();
    assert!(close(r.distance.unwrap(), 10.0, 1e-6), "{:?}", r.distance);
    st.dialog_ok();
    assert!(st.dialog.is_none());
}

#[test]
fn appearance_colours_selected_bodies() {
    let (mut st, _, _, ext) = box_via_dialog("50 mm", "30 mm", "10 mm");
    st.execute(CommandId::Appearance);
    assert!(!st.apply_appearance(Some(2)), "nothing selected");
    st.select(face_with_normal(&st, [0.0, 0.0, 1.0]), false);
    assert!(st.apply_appearance(Some(2)));
    let meta = &st.doc.bodies[&ext];
    assert_eq!(meta.color, Some(APPEARANCES[2].1));
    assert!(meta.visible, "colouring keeps the body visible");
    st.dialog_ok();
    st.undo();
    assert_eq!(st.doc.bodies.get(&ext).and_then(|m| m.color), None);
}

#[test]
fn tool_dialogs_do_not_touch_a_sketch_transaction() {
    let mut st = new_state();
    st.create_sketch(PlaneRef::Origin { plane: OriginPlane::XY });
    assert!(st.in_txn());
    st.execute(CommandId::Measure);
    let p = Pick::SketchEntity("l1".into());
    st.select(p.clone(), false);
    assert_eq!(st.selection, vec![p], "sketch selections survive an open Measure dialog");
    st.dialog_cancel();
    assert!(st.in_txn(), "closing Measure must not end the sketch session");
    st.execute(CommandId::Extrude);
    assert!(st.dialog.is_none(), "Extrude is unavailable while sketching");
}
