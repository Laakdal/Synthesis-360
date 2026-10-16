//! Headless sketch-environment workflow tests: they drive `sketch_pointer` / `sketch_key` /
//! `execute` exactly like the canvas does (Move + Click, Press + Drag + Release).

use super::*;
use crate::state::AppState;
use cad_core::sketch::{Constraint, SolveReport};
use cad_core::OriginPlane;

const TOL: f64 = 0.5;

fn ev(s: &mut AppState, kind: PointerKind, x: f64, y: f64) {
    s.sketch_pointer(SketchPointer { pos: [x, y], tolerance: TOL, kind, shift: false, ctrl: false });
}

fn click(s: &mut AppState, x: f64, y: f64) {
    ev(s, PointerKind::Move, x, y);
    ev(s, PointerKind::Click, x, y);
}

fn shift_click(s: &mut AppState, x: f64, y: f64) {
    ev(s, PointerKind::Move, x, y);
    s.sketch_pointer(SketchPointer { pos: [x, y], tolerance: TOL, kind: PointerKind::Click, shift: true, ctrl: false });
}

fn drag(s: &mut AppState, from: [f64; 2], to: [f64; 2]) {
    ev(s, PointerKind::Move, from[0], from[1]);
    ev(s, PointerKind::Press, from[0], from[1]);
    for i in 1..=4 {
        let t = i as f64 / 4.0;
        ev(s, PointerKind::Drag, from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t);
    }
    ev(s, PointerKind::Release, to[0], to[1]);
}

/// `AppState::new()` -> `Kernel::new()` -> OCC `cad_quiet()`, which mutates OCC's global
/// messenger and is not thread-safe; parallel tests must not run it concurrently.
static OCC_INIT: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn new_sketch() -> AppState {
    let mut s = {
        let _guard = OCC_INIT.lock().unwrap_or_else(|e| e.into_inner());
        AppState::new()
    };
    s.create_sketch(PlaneRef::Origin { plane: OriginPlane::XY });
    assert!(s.active_sketch().is_some());
    s
}

fn sk(s: &AppState) -> Sketch {
    s.doc_sketch().expect("active sketch").clone()
}

fn report(s: &AppState) -> SolveReport {
    s.sketch_result().expect("solved sketch").report.clone()
}

fn count(s: &AppState, f: impl Fn(&Constraint) -> bool) -> usize {
    sk(s).constraints.iter().filter(|c| f(c)).count()
}

fn lines(s: &AppState) -> Vec<String> {
    sk(s).entities.iter().filter(|e| matches!(e.kind, EntityKind::Line { .. })).map(|e| e.id.clone()).collect()
}

fn line_at(s: &AppState, p: [f64; 2]) -> String {
    let sk = sk(s);
    sk.entities
        .iter()
        .find(|e| matches!(e.kind, EntityKind::Line { .. }) && interact::closest_on_curve(&sk, e, p).is_some_and(|(_, d)| d < 0.2))
        .map(|e| e.id.clone())
        .expect("a line through the point")
}

fn point_at(s: &AppState, p: [f64; 2]) -> Option<String> {
    let sk = sk(s);
    sk.entities.iter().find(|e| matches!(e.kind, EntityKind::Point { x, y } if (x - p[0]).abs() < 1e-6 && (y - p[1]).abs() < 1e-6)).map(|e| e.id.clone())
}

fn param_value(s: &AppState, name: &str) -> Option<f64> {
    s.doc.parameter_values().values.get(name).copied()
}

fn last_msg(s: &AppState) -> String {
    s.last_message().map(|m| m.text.clone()).unwrap_or_default()
}

/// Places a dimension with the Sketch Dimension tool and enters `value` in its value box.
fn dimension(s: &mut AppState, pick: [f64; 2], label: [f64; 2], value: &str) {
    s.execute(CommandId::SketchDimension);
    click(s, pick[0], pick[1]);
    ev(s, PointerKind::Move, label[0], label[1]);
    click(s, label[0], label[1]);
    assert!(matches!(s.sketch_prompt(), Some(SketchPrompt::Value { .. })), "value box opens after placing: {:?}", s.sketch_prompt());
    s.sketch_set_value_text(value);
    s.sketch_key(SketchKey::Enter);
    assert!(s.sketch_prompt().is_none(), "value accepted: {}", last_msg(s));
}

/// Rectangle with a corner on the origin, width 50 and height 30: fully constrained (F17).
fn constrained_rectangle() -> AppState {
    let mut s = new_sketch();
    s.execute(CommandId::Rectangle2Pt);
    click(&mut s, 0.1, -0.1); // snaps to the origin -> coincident
    click(&mut s, 40.0, 20.0);
    dimension(&mut s, [20.0, 0.0], [20.0, -8.0], "50");
    dimension(&mut s, [50.0, 10.0], [58.0, 10.0], "30");
    s.set_sketch_tool(SketchTool::Select);
    s
}

#[test]
fn rectangle_tool_adds_two_horizontal_and_two_vertical() {
    let mut s = new_sketch();
    s.execute(CommandId::Rectangle2Pt);
    click(&mut s, 10.0, 10.0);
    click(&mut s, 40.0, 30.0);
    assert_eq!(lines(&s).len(), 4);
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Horizontal { .. })), 2);
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Vertical { .. })), 2);
    assert_eq!(s.sketch_result().unwrap().profiles.len(), 1);
    // the tool stays active for the next rectangle
    assert_eq!(s.active_sketch().unwrap().tool, SketchTool::Rectangle2Pt);
}

#[test]
fn center_rectangle_on_origin_is_centred() {
    let mut s = new_sketch();
    s.execute(CommandId::RectangleCenter);
    click(&mut s, 0.1, 0.1);
    click(&mut s, 20.0, 10.0);
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Horizontal { .. })), 2);
    assert_eq!(count(&s, |c| matches!(&c.kind, ConstraintKind::Midpoint { p, .. } if p == ORIGIN)), 1);
    assert!(point_at(&s, [-20.0, -10.0]).is_some());
    let construction = sk(&s).entities.iter().filter(|e| e.construction).count();
    assert_eq!(construction, 1, "construction diagonal");
}

#[test]
fn line_inference_adds_horizontal() {
    let mut s = new_sketch();
    s.settings.snap = false;
    s.execute(CommandId::Line);
    click(&mut s, 5.0, 5.0);
    click(&mut s, 30.0, 5.6); // ~1.4 deg off horizontal (F15)
    s.sketch_key(SketchKey::Escape);
    assert_eq!(lines(&s).len(), 1);
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Horizontal { .. })), 1);
    let (a, b) = sk(&s).line_points(&lines(&s)[0]).unwrap();
    assert!((a[1] - b[1]).abs() < 1e-9 && (b[0] - 30.0).abs() < 1e-9);
    // a clearly inclined line gets no inference
    click(&mut s, 5.0, 20.0);
    click(&mut s, 20.0, 30.0);
    s.sketch_key(SketchKey::Escape);
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Horizontal { .. } | ConstraintKind::Vertical { .. })), 1);
}

#[test]
fn line_chain_shares_points_and_closes_loop() {
    let mut s = new_sketch();
    s.execute(CommandId::Line);
    for p in [[10.0, 10.0], [30.0, 10.0], [30.0, 25.0], [10.0, 25.0], [10.0, 10.0]] {
        click(&mut s, p[0], p[1]);
    }
    assert!(s.active_sketch().unwrap().chain.is_none(), "clicking the start point closes the loop");
    assert_eq!(lines(&s).len(), 4);
    let points = sk(&s).entities.iter().filter(|e| matches!(e.kind, EntityKind::Point { .. })).count();
    assert_eq!(points, 4, "segments share their end points");
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Horizontal { .. } | ConstraintKind::Vertical { .. })), 4);
    assert_eq!(s.sketch_result().unwrap().profiles.len(), 1);
}

#[test]
fn double_click_ends_line_chain_without_extra_segment() {
    let mut s = new_sketch();
    s.execute(CommandId::Line);
    click(&mut s, 5.0, 5.0);
    click(&mut s, 20.0, 12.0);
    ev(&mut s, PointerKind::DoubleClick, 20.0, 12.0);
    assert!(s.active_sketch().unwrap().chain.is_none());
    assert_eq!(lines(&s).len(), 1);
    // ending after a single click leaves nothing behind
    click(&mut s, 40.0, 40.0);
    s.sketch_key(SketchKey::Enter);
    assert_eq!(sk(&s).entities.len(), 3);
}

#[test]
fn snapping_onto_existing_endpoint_shares_it_and_origin_is_coincident() {
    let mut s = new_sketch();
    s.execute(CommandId::Line);
    click(&mut s, 5.0, 5.0);
    click(&mut s, 25.0, 5.0);
    s.sketch_key(SketchKey::Escape);
    let l1 = lines(&s)[0].clone();
    click(&mut s, 25.2, 5.1); // within tolerance of the end point
    click(&mut s, 25.0, 20.0);
    s.sketch_key(SketchKey::Escape);
    let sketch = sk(&s);
    let l2 = lines(&s).into_iter().find(|l| *l != l1).unwrap();
    let ends = |l: &str| match &sketch.entity(l).unwrap().kind {
        EntityKind::Line { p1, p2 } => (p1.clone(), p2.clone()),
        _ => unreachable!(),
    };
    assert_eq!(ends(&l1).1, ends(&l2).0, "second line starts on the first line's end point");
    // a line started on the sketch origin gets a coincident constraint (F16)
    click(&mut s, 0.2, -0.1);
    click(&mut s, -20.0, -15.0);
    s.sketch_key(SketchKey::Escape);
    assert_eq!(count(&s, |c| matches!(&c.kind, ConstraintKind::Coincident { b, .. } if b == ORIGIN)), 1);
}

#[test]
fn circle_radius_point_on_origin_adds_coincident() {
    let mut s = new_sketch();
    s.execute(CommandId::CircleCenter);
    click(&mut s, 10.0, 0.0);
    click(&mut s, 0.1, 0.1);
    assert_eq!(count(&s, |c| matches!(&c.kind, ConstraintKind::PointOnCurve { p, .. } if p == ORIGIN)), 1);
    s.execute(CommandId::Arc3Pt);
    click(&mut s, 30.0, 0.0);
    click(&mut s, 50.0, 0.0);
    click(&mut s, 40.0, 10.0);
    assert!(sk(&s).entities.iter().any(|e| matches!(e.kind, EntityKind::Arc { .. })));
    s.execute(CommandId::Point);
    click(&mut s, 70.0, 5.0);
    assert!(point_at(&s, [70.0, 5.0]).is_some());
}

#[test]
fn dimensions_fully_constrain_rectangle_and_conflict_is_offered_as_driven() {
    let mut s = constrained_rectangle();
    assert_eq!(param_value(&s, "d1"), Some(50.0));
    assert_eq!(param_value(&s, "d2"), Some(30.0));
    assert!(point_at(&s, [50.0, 30.0]).is_some(), "geometry follows the dimensions");
    assert!(report(&s).is_fully_constrained(), "F17: H/V + origin + 2 dimensions");
    let o = s.sketch_overlay().unwrap();
    assert_eq!(o.status, "Fully constrained");
    assert!(o.curves.iter().all(|c| c.state == CurveState::Fixed));
    assert!(o.dimensions.iter().any(|d| d.text == "50 mm"));

    // a third dimension on the top edge over-constrains (F18)
    s.execute(CommandId::SketchDimension);
    click(&mut s, 25.0, 30.0);
    click(&mut s, 25.0, 38.0);
    assert!(matches!(s.sketch_prompt(), Some(SketchPrompt::Driven { .. })));
    assert_eq!(last_msg(&s), OVER_CONSTRAINED);
    let dims = count(&s, |c| c.kind.is_dimension());
    s.sketch_key(SketchKey::Enter); // accept as driven
    assert_eq!(count(&s, |c| c.kind.is_dimension()), dims + 1);
    assert_eq!(count(&s, |c| c.driven), 1);
    assert!(report(&s).is_fully_constrained());
    let o = s.sketch_overlay().unwrap();
    assert!(o.dimensions.iter().any(|d| d.driven && d.text == "(50 mm)"), "{:?}", o.dimensions.iter().map(|d| &d.text).collect::<Vec<_>>());

    // a conflicting geometric constraint is rejected too
    let before = sk(&s).constraints.len();
    s.execute(CommandId::ConstraintHorizontalVertical);
    click(&mut s, 25.0, 0.0);
    assert_eq!(last_msg(&s), OVER_CONSTRAINED);
    assert_eq!(sk(&s).constraints.len(), before);
}

#[test]
fn dimension_value_box_accepts_expressions_and_rejects_invalid() {
    let mut s = constrained_rectangle();
    // double-click the width label to edit it
    click(&mut s, 20.0, -8.0);
    ev(&mut s, PointerKind::DoubleClick, 20.0, -8.0);
    assert!(matches!(s.sketch_prompt(), Some(SketchPrompt::Value { .. })));
    s.sketch_set_value_text("foo * 2");
    s.sketch_key(SketchKey::Enter);
    assert!(matches!(s.sketch_prompt(), Some(SketchPrompt::Value { error: Some(_), .. })), "invalid expression keeps the box open");
    s.sketch_set_value_text("d2 + 10");
    s.sketch_key(SketchKey::Enter);
    assert!(s.sketch_prompt().is_none());
    assert_eq!(param_value(&s, "d1"), Some(40.0));
    assert!(point_at(&s, [40.0, 30.0]).is_some());
    // Esc in a value box leaves the value unchanged
    ev(&mut s, PointerKind::DoubleClick, 20.0, -8.0);
    s.sketch_set_value_text("12");
    s.sketch_key(SketchKey::Escape);
    assert_eq!(param_value(&s, "d1"), Some(40.0));
}

#[test]
fn finish_sketch_is_one_undo_step_and_undo_removes_it() {
    let mut s = new_sketch();
    s.execute(CommandId::Rectangle2Pt);
    click(&mut s, 10.0, 10.0);
    click(&mut s, 40.0, 30.0);
    s.execute(CommandId::Line);
    click(&mut s, 50.0, 10.0);
    click(&mut s, 70.0, 20.0);
    s.execute(CommandId::FinishSketch);
    assert!(s.active_sketch().is_none());
    assert_eq!(s.history.undo_label(), Some("Create Sketch"));
    assert!(s.doc.features.iter().any(|f| f.sketch().is_some_and(|k| k.entities.len() > 4)));
    s.undo();
    assert!(!s.doc.features.iter().any(|f| f.sketch().is_some()));
    assert!(!s.history.can_undo(), "exactly one undo step");
}

#[test]
fn cancel_restores_the_sketch() {
    let mut s = new_sketch();
    s.execute(CommandId::Line);
    click(&mut s, 5.0, 5.0);
    click(&mut s, 20.0, 12.0);
    s.execute(CommandId::FinishSketch);
    let id = s.doc.features.iter().find(|f| f.sketch().is_some()).unwrap().id.clone();
    let before = s.doc.feature(&id).unwrap().sketch().unwrap().clone();
    s.edit_sketch(&id);
    s.execute(CommandId::CircleCenter);
    click(&mut s, 40.0, 40.0);
    click(&mut s, 45.0, 40.0);
    assert!(sk(&s).entities.len() > before.entities.len());
    s.cancel_sketch();
    assert_eq!(s.doc.feature(&id).unwrap().sketch().unwrap(), &before);
    assert_eq!(s.history.undo_label(), Some("Create Sketch"));
}

#[test]
fn escape_with_select_tool_finishes_sketch() {
    let mut s = new_sketch();
    s.execute(CommandId::Line);
    s.sketch_key(SketchKey::Escape); // leaves the line tool
    assert!(s.active_sketch().is_some());
    s.sketch_key(SketchKey::Escape); // nothing in progress: finish
    assert!(s.active_sketch().is_none());
    assert_eq!(s.history.undo_label(), Some("Create Sketch"));
}

#[test]
fn dragging_free_point_moves_it_but_constrained_point_stays() {
    let mut s = new_sketch();
    s.settings.snap = false;
    s.execute(CommandId::Line);
    click(&mut s, 5.0, 5.0);
    click(&mut s, 20.0, 15.0);
    s.set_sketch_tool(SketchTool::Select);
    let p = point_at(&s, [20.0, 15.0]).unwrap();
    drag(&mut s, [20.0, 15.0], [24.0, 19.0]);
    let q = sk(&s).point(&p).unwrap();
    assert!((q[0] - 24.0).abs() < 1e-6 && (q[1] - 19.0).abs() < 1e-6, "{q:?}");

    let mut s = constrained_rectangle();
    let c = point_at(&s, [50.0, 30.0]).unwrap();
    drag(&mut s, [50.0, 30.0], [60.0, 40.0]);
    assert_eq!(sk(&s).point(&c), Some([50.0, 30.0]));
}

#[test]
fn dragging_partially_constrained_rectangle_keeps_it_rectangular() {
    let mut s = new_sketch();
    s.execute(CommandId::Rectangle2Pt);
    click(&mut s, 10.0, 10.0);
    click(&mut s, 40.0, 30.0);
    s.set_sketch_tool(SketchTool::Select);
    drag(&mut s, [40.0, 30.0], [50.0, 35.0]);
    let sketch = sk(&s);
    assert!(point_at(&s, [50.0, 35.0]).is_some(), "dragged corner follows the cursor");
    assert!(point_at(&s, [50.0, 10.0]).is_some() && point_at(&s, [10.0, 35.0]).is_some(), "neighbours stay H/V: {:?}", sketch.entities);
}

#[test]
fn delete_removes_entity_and_its_dimension_parameter() {
    let mut s = constrained_rectangle();
    let bottom = line_at(&s, [25.0, 0.0]);
    click(&mut s, 25.0, 0.0);
    assert_eq!(s.selection, vec![Pick::SketchEntity(bottom.clone())]);
    s.execute(CommandId::Delete);
    assert!(sk(&s).entity(&bottom).is_none());
    assert!(s.doc.parameter("d1").is_none(), "the width dimension's parameter is deleted with it");
    assert!(s.doc.parameter("d2").is_some());
    assert!(s.selection.is_empty());

    // a dimension selected by its label is deleted alone
    let mut s = constrained_rectangle();
    click(&mut s, 58.0, 10.0);
    assert!(matches!(s.selection.as_slice(), [Pick::SketchConstraint(_)]));
    s.sketch_key(SketchKey::Delete);
    assert!(s.doc.parameter("d2").is_none());
    assert_eq!(lines(&s).len(), 4);
    assert!(!report(&s).is_fully_constrained());
}

#[test]
fn glyph_selection_deletes_constraint() {
    let mut s = new_sketch();
    s.execute(CommandId::Rectangle2Pt);
    click(&mut s, 10.0, 10.0);
    click(&mut s, 40.0, 30.0);
    s.set_sketch_tool(SketchTool::Select);
    let o = s.sketch_overlay().unwrap();
    assert_eq!(o.glyphs.iter().filter(|g| g.label == "H").count(), 2);
    let g = o.glyphs.iter().find(|g| g.label == "V").unwrap();
    let (gid, gp) = (g.constraint.clone(), g.pos);
    click(&mut s, gp[0], gp[1]);
    assert_eq!(s.selection, vec![Pick::SketchConstraint(gid.clone())]);
    s.execute(CommandId::Delete);
    assert!(sk(&s).constraint(&gid).is_none());
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Vertical { .. })), 1);
}

#[test]
fn constraint_tools_pick_entities_and_use_preselection() {
    let mut s = new_sketch();
    s.settings.snap = false;
    s.execute(CommandId::Line);
    click(&mut s, 0.0 + 5.0, 5.0);
    click(&mut s, 25.0, 12.0);
    s.sketch_key(SketchKey::Escape);
    click(&mut s, 5.0, 20.0);
    click(&mut s, 15.0, 35.0);
    s.sketch_key(SketchKey::Escape);
    let l1 = line_at(&s, [5.0, 5.0]);
    let l2 = line_at(&s, [5.0, 20.0]);

    // pick two lines with the Parallel tool
    s.execute(CommandId::ConstraintParallel);
    click(&mut s, 15.0, 5.0 + 7.0 * 10.0 / 20.0);
    assert_eq!(s.active_sketch().unwrap().picks, vec![l1.clone()]);
    click(&mut s, 10.0, 27.5);
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Parallel { .. })), 1);
    let sketch = sk(&s);
    let (a1, a2) = sketch.line_points(&l1).unwrap();
    let (b1, b2) = sketch.line_points(&l2).unwrap();
    assert!(interact::cross(interact::sub(a2, a1), interact::sub(b2, b1)).abs() < 1e-6);

    // invalid pick: a point for Parallel is not accepted (filtered), message stays helpful
    s.execute(CommandId::ConstraintEqual);
    s.selection = vec![Pick::SketchEntity(l1.clone()), Pick::SketchEntity(l2.clone())];
    s.execute(CommandId::ConstraintEqual);
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Equal { .. })), 1);
    assert_eq!(s.active_sketch().unwrap().tool, SketchTool::Select, "pre-selection applies and ends the tool");

    // Coincident: point + point
    let p = match &sk(&s).entity(&l2).unwrap().kind {
        EntityKind::Line { p1, .. } => p1.clone(),
        _ => unreachable!(),
    };
    s.selection = vec![Pick::SketchEntity(p.clone()), Pick::SketchEntity(ORIGIN.into())];
    s.execute(CommandId::ConstraintCoincident);
    assert!(interact::len(sk(&s).point(&p).unwrap()) < 1e-9);

    // Fix toggles
    s.selection = vec![Pick::SketchEntity(l1.clone())];
    s.execute(CommandId::ConstraintFix);
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Fix { .. })), 2);
    s.selection = vec![Pick::SketchEntity(l1.clone())];
    s.execute(CommandId::ConstraintFix);
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Fix { .. })), 0);

    // wrong entity types give a helpful message and add nothing
    let n = sk(&s).constraints.len();
    s.selection = vec![Pick::SketchEntity(p.clone())];
    s.execute(CommandId::ConstraintHorizontalVertical);
    assert!(last_msg(&s).contains("select a line"), "{}", last_msg(&s));
    assert_eq!(sk(&s).constraints.len(), n);
}

#[test]
fn perpendicular_tangent_and_symmetry_tools() {
    let mut s = new_sketch();
    s.settings.snap = false;
    s.execute(CommandId::Line);
    click(&mut s, -20.0, 0.0);
    click(&mut s, 20.0, 1.5 * 40.0 / 40.0 * 3.0);
    s.sketch_key(SketchKey::Escape);
    let base = lines(&s)[0].clone();
    s.execute(CommandId::CircleCenter);
    click(&mut s, 0.0 + 1.0, 15.0);
    click(&mut s, 5.0, 15.0);
    let circle = sk(&s).entities.iter().find(|e| matches!(e.kind, EntityKind::Circle { .. })).unwrap().id.clone();
    s.selection = vec![Pick::SketchEntity(base.clone()), Pick::SketchEntity(circle.clone())];
    s.execute(CommandId::ConstraintTangent);
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Tangent { .. })), 1);
    assert!(report(&s).status == SolveStatus::Solved);

    s.execute(CommandId::Point);
    click(&mut s, -10.0, -10.0);
    click(&mut s, 10.0, -12.0);
    s.execute(CommandId::Line);
    click(&mut s, 0.5, -30.0);
    click(&mut s, 0.3, -40.0);
    s.sketch_key(SketchKey::Escape);
    let axis = line_at(&s, [0.4, -35.0]);
    let a = point_at(&s, [-10.0, -10.0]).unwrap();
    let b = point_at(&s, [10.0, -12.0]).unwrap();
    s.execute(CommandId::ConstraintSymmetry);
    for id in [&a, &b, &axis] {
        s.select(Pick::SketchEntity(id.clone()), true);
    }
    s.execute(CommandId::ConstraintSymmetry);
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Symmetric { .. })), 1);
    assert_eq!(report(&s).status, SolveStatus::Solved);
}

#[test]
fn toggle_construction_switches_linetype() {
    let mut s = new_sketch();
    s.execute(CommandId::Line);
    click(&mut s, 5.0, 5.0);
    click(&mut s, 20.0, 12.0);
    s.set_sketch_tool(SketchTool::Select);
    let l = lines(&s)[0].clone();
    click(&mut s, 12.5, 8.5);
    s.execute(CommandId::ToggleConstruction);
    assert!(sk(&s).entity(&l).unwrap().construction);
    s.selection.clear();
    ev(&mut s, PointerKind::Move, 100.0, 100.0);
    let o = s.sketch_overlay().unwrap();
    let c = o.curves.iter().find(|c| c.id == l).unwrap();
    assert!(c.dashed && c.state == CurveState::Construction);
}

#[test]
fn overlay_shows_previews_hover_and_selection() {
    let mut s = new_sketch();
    s.execute(CommandId::Rectangle2Pt);
    click(&mut s, 10.0, 10.0);
    ev(&mut s, PointerKind::Move, 30.0, 20.0);
    let o = s.sketch_overlay().unwrap();
    assert!(o.curves.iter().any(|c| c.state == CurveState::Preview && c.polyline.len() == 5));
    click(&mut s, 30.0, 20.0);
    s.set_sketch_tool(SketchTool::Select);
    ev(&mut s, PointerKind::Move, 20.0, 10.1);
    let bottom = line_at(&s, [20.0, 10.0]);
    assert_eq!(s.hover, Some(Pick::SketchEntity(bottom.clone())));
    let o = s.sketch_overlay().unwrap();
    assert_eq!(o.curves.iter().find(|c| c.id == bottom).unwrap().state, CurveState::Hovered);
    click(&mut s, 20.0, 10.0);
    ev(&mut s, PointerKind::Move, 100.0, 100.0);
    let o = s.sketch_overlay().unwrap();
    assert_eq!(o.curves.iter().find(|c| c.id == bottom).unwrap().state, CurveState::Selected);
    assert!(o.curves.iter().filter(|c| c.id != bottom).all(|c| c.state == CurveState::Free));
    assert!(o.status.contains("degrees of freedom"), "{}", o.status);
    assert!(o.points.iter().any(|p| p.id == ORIGIN));
}

#[test]
fn dragging_dimension_label_moves_it() {
    let mut s = constrained_rectangle();
    drag(&mut s, [20.0, -8.0], [25.0, -12.0]);
    let c = sk(&s).constraints.iter().find(|c| c.param.as_deref() == Some("d1")).unwrap().clone();
    assert_eq!(c.label, Some([25.0, -12.0]));
    assert!(report(&s).is_fully_constrained());
    let o = s.sketch_overlay().unwrap();
    assert!(o.dimensions.iter().any(|d| d.anchor == [25.0, -12.0]));
}

#[test]
fn cancel_restores_constraint_only_edits() {
    let mut s = new_sketch();
    s.execute(CommandId::Rectangle2Pt);
    click(&mut s, 10.0, 10.0);
    click(&mut s, 40.0, 30.0);
    s.execute(CommandId::FinishSketch);
    let id = s.doc.features.iter().find(|f| f.sketch().is_some()).unwrap().id.clone();
    s.edit_sketch(&id);
    ev(&mut s, PointerKind::Move, 100.0, 100.0); // glyph offsets follow the pick tolerance
    let g = s.sketch_overlay().unwrap().glyphs.into_iter().find(|g| g.label == "V").unwrap();
    click(&mut s, g.pos[0], g.pos[1]);
    s.execute(CommandId::Delete);
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Vertical { .. })), 1);
    s.cancel_sketch();
    let vertical = s.doc.feature(&id).unwrap().sketch().unwrap().constraints.iter().filter(|c| matches!(c.kind, ConstraintKind::Vertical { .. })).count();
    assert_eq!(vertical, 2);
}

#[test]
fn dimension_types_follow_the_selection() {
    let mut s = new_sketch();
    s.settings.snap = false;
    s.execute(CommandId::CircleCenter);
    click(&mut s, 100.0, 0.0);
    click(&mut s, 110.0, 0.0);
    s.execute(CommandId::Arc3Pt);
    click(&mut s, 130.0, 0.0);
    click(&mut s, 150.0, 0.0);
    click(&mut s, 140.0, 10.0);
    s.execute(CommandId::Line);
    click(&mut s, 10.0, 10.0);
    click(&mut s, 40.0, 20.0);
    click(&mut s, 50.0, 50.0);
    s.sketch_key(SketchKey::Escape);
    s.execute(CommandId::Point);
    click(&mut s, 20.0, 40.0);

    // circle -> diameter
    dimension(&mut s, [100.0, 10.0], [100.0, 20.0], "24");
    let circle = sk(&s).entities.iter().find(|e| matches!(e.kind, EntityKind::Circle { .. })).unwrap().id.clone();
    assert!(matches!(&sk(&s).entity(&circle).unwrap().kind, EntityKind::Circle { r, .. } if (r - 12.0).abs() < 1e-6));
    // arc -> radius
    dimension(&mut s, [140.0, 10.0], [140.0, 25.0], "15");
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Radius { .. })), 1);
    // two lines -> angle
    s.execute(CommandId::SketchDimension);
    click(&mut s, 25.0, 15.0);
    click(&mut s, 45.0, 35.0);
    click(&mut s, 45.0, 20.0);
    assert!(matches!(s.sketch_prompt(), Some(SketchPrompt::Value { text, .. }) if text.ends_with("deg")), "{:?}", s.sketch_prompt());
    s.sketch_key(SketchKey::Enter);
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Angle { .. })), 1);
    // point + line -> point-line distance
    let pt = point_at(&s, [20.0, 40.0]).unwrap();
    s.execute(CommandId::SketchDimension);
    click(&mut s, 20.0, 40.0);
    let l1 = line_at(&s, [25.0, 15.0]);
    let (a, b) = sk(&s).line_points(&l1).unwrap();
    let m = interact::lerp(a, b, 0.5);
    click(&mut s, m[0], m[1]);
    click(&mut s, 5.0, 30.0);
    assert!(sk(&s).constraints.iter().any(|c| matches!(&c.kind, ConstraintKind::PointLineDistance { p, line } if *p == pt && *line == l1)));
    s.sketch_key(SketchKey::Escape);
    let o = s.sketch_overlay().unwrap();
    assert!(o.dimensions.iter().any(|d| d.text == "Ø24 mm") && o.dimensions.iter().any(|d| d.text == "R15 mm"), "{:?}", o.dimensions.iter().map(|d| &d.text).collect::<Vec<_>>());
    assert_eq!(report(&s).status, SolveStatus::Solved);
}

// ---------------------------------------------------------------------------- advanced tools

fn areas(s: &AppState) -> Vec<f64> {
    let mut a: Vec<f64> = s.sketch_result().expect("solved sketch").profiles.iter().map(|p| p.area).collect();
    a.sort_by(f64::total_cmp);
    a
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn rect(s: &mut AppState, a: [f64; 2], b: [f64; 2]) {
    s.execute(CommandId::Rectangle2Pt);
    click(s, a[0], a[1]);
    click(s, b[0], b[1]);
}

fn line(s: &mut AppState, a: [f64; 2], b: [f64; 2]) {
    s.execute(CommandId::Line);
    click(s, a[0], a[1]);
    click(s, b[0], b[1]);
    s.sketch_key(SketchKey::Escape);
}

#[test]
fn advanced_tools_are_enabled_only_while_sketching() {
    let mut s = new_sketch();
    for id in [
        CommandId::Rectangle3Pt,
        CommandId::Circle2Pt,
        CommandId::ArcCenter,
        CommandId::Polygon,
        CommandId::Slot,
        CommandId::SketchFillet,
        CommandId::Trim,
        CommandId::Extend,
        CommandId::SketchOffset,
        CommandId::SketchMirror,
    ] {
        assert!(crate::state::command::spec(id).supported, "{id:?}");
        assert!(s.is_enabled(id), "{id:?}");
        s.execute(id);
        assert_ne!(s.active_sketch().unwrap().tool, SketchTool::Select, "{id:?} activates its tool");
        assert_eq!(crate::ui::ribbon::tool_for(id), Some(s.active_sketch().unwrap().tool), "{id:?} highlights in the ribbon");
        assert!(!s.sketch_tool_hint().is_empty());
    }
    s.finish_sketch();
    assert!(!s.is_enabled(CommandId::Trim));
}

#[test]
fn trim_rectangle_crossed_by_line() {
    let mut s = new_sketch();
    rect(&mut s, [5.0, 5.0], [45.0, 25.0]);
    line(&mut s, [25.0, 0.0], [25.0, 35.0]);
    let l = line_at(&s, [25.0, 15.0]);
    s.execute(CommandId::Trim);
    // hovering shows the segment that goes
    ev(&mut s, PointerKind::Move, 25.0, 2.0);
    assert!(s.sketch_overlay().unwrap().curves.iter().any(|c| c.id == PREVIEW_ID && c.state == CurveState::Error));
    click(&mut s, 25.0, 2.0);
    click(&mut s, 25.0, 32.0);
    let (a, b) = sk(&s).line_points(&l).unwrap();
    assert!(interact::dist(a, [25.0, 5.0]) < 1e-9 && interact::dist(b, [25.0, 25.0]) < 1e-9, "{a:?} {b:?}");
    let on_edges = count(&s, |c| matches!(&c.kind, ConstraintKind::PointOnCurve { p, .. } if sk(&s).entity(p).is_some_and(|e| e.owner.as_deref() == Some(l.as_str()))));
    assert_eq!(on_edges, 2, "the trimmed ends stay on the edges");
    assert_eq!(areas(&s), vec![400.0, 400.0]);
    // trimming the right half of the top edge opens the right square
    click(&mut s, 35.0, 25.0);
    let a = areas(&s);
    assert_eq!(a.len(), 1);
    assert!(close(a[0], 400.0), "{a:?}");
    // a curve without intersections is deleted
    line(&mut s, [60.0, 0.0], [60.0, 10.0]);
    let n = lines(&s).len();
    s.execute(CommandId::Trim);
    click(&mut s, 60.0, 5.0);
    assert_eq!(lines(&s).len(), n - 1);
    assert_eq!(report(&s).status, SolveStatus::Solved);
}

#[test]
fn extend_line_to_rectangle_edges() {
    let mut s = new_sketch();
    rect(&mut s, [5.0, 5.0], [45.0, 25.0]);
    line(&mut s, [15.0, 15.0], [25.0, 15.0]);
    let l = line_at(&s, [20.0, 15.0]);
    s.execute(CommandId::Extend);
    click(&mut s, 24.0, 15.0);
    click(&mut s, 16.0, 15.0);
    let (a, b) = sk(&s).line_points(&l).unwrap();
    assert!(interact::dist(a, [5.0, 15.0]) < 1e-9 && interact::dist(b, [45.0, 15.0]) < 1e-9, "{a:?} {b:?}");
    assert_eq!(areas(&s), vec![400.0, 400.0]);
}

#[test]
fn fillet_corner_of_constrained_rectangle() {
    let mut s = constrained_rectangle();
    assert!(report(&s).is_fully_constrained());
    s.execute(CommandId::SketchFillet);
    click(&mut s, 50.0, 30.0); // the corner point
    assert!(matches!(s.sketch_prompt(), Some(SketchPrompt::Value { .. })), "radius box opens: {}", last_msg(&s));
    s.sketch_set_value_text("5");
    s.sketch_key(SketchKey::Enter);
    assert!(s.sketch_prompt().is_none(), "{}", last_msg(&s));
    let r = 5.0;
    let a = areas(&s);
    assert_eq!(a.len(), 1);
    assert!(close(a[0], 50.0 * 30.0 - (r * r - std::f64::consts::PI * r * r / 4.0)), "{a:?}");
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Tangent { .. })), 2);
    assert!(report(&s).is_fully_constrained(), "{:?}", report(&s));
    assert!(point_at(&s, [45.0, 30.0]).is_some() && point_at(&s, [50.0, 25.0]).is_some(), "tangent points");
    // a larger radius re-solves with the tangency kept
    let rp = sk(&s).constraints.iter().find(|c| matches!(c.kind, ConstraintKind::Radius { .. })).unwrap().id.clone();
    s.open_value_edit(&rp);
    s.sketch_set_value_text("8");
    s.sketch_key(SketchKey::Enter);
    assert!(close(areas(&s)[0], 1500.0 - (64.0 - std::f64::consts::PI * 16.0)), "{:?}", areas(&s));
    assert!(point_at(&s, [42.0, 30.0]).is_some(), "tangent point moved with the radius");
    // the next fillet starts from the last radius; pick two lines this time (corner on the origin)
    click(&mut s, 25.0, 0.0);
    click(&mut s, 0.0, 15.0);
    assert!(matches!(s.sketch_prompt(), Some(SketchPrompt::Value { text, .. }) if text == "8 mm"), "{:?}", s.sketch_prompt());
    s.sketch_key(SketchKey::Enter);
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Radius { .. })), 2);
    assert!(report(&s).is_fully_constrained(), "the corner stays on the origin: {:?}", report(&s));
}

#[test]
fn offset_rectangle_gives_two_profile_ring() {
    let mut s = new_sketch();
    rect(&mut s, [5.0, 5.0], [45.0, 25.0]);
    s.execute(CommandId::SketchOffset);
    click(&mut s, 25.0, 25.0); // selects the chain
    ev(&mut s, PointerKind::Move, 25.0, 28.0);
    assert!(s.sketch_overlay().unwrap().curves.iter().filter(|c| c.id == PREVIEW_ID).count() >= 4, "offset preview");
    click(&mut s, 25.0, 28.0); // outside
    assert!(matches!(s.sketch_prompt(), Some(SketchPrompt::Value { text, .. }) if text == "3 mm"), "{:?} {}", s.sketch_prompt(), last_msg(&s));
    s.sketch_set_value_text("5");
    s.sketch_key(SketchKey::Enter);
    assert!(s.sketch_prompt().is_none(), "{}", last_msg(&s));
    assert_eq!(lines(&s).len(), 8);
    assert_eq!(areas(&s), vec![700.0, 800.0]);
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Parallel { .. })), 4);
    // one offset parameter drives every side
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::PointLineDistance { .. })), 4);
    assert_eq!(s.doc.parameters.iter().filter(|p| p.expression == "5").count(), 1);
}

#[test]
fn mirror_line_is_clicked_without_enter() {
    let mut s = new_sketch();
    line(&mut s, [0.0, -10.0], [0.0, 40.0]);
    let axis = line_at(&s, [0.0, 15.0]);
    s.execute(CommandId::CircleCenter);
    click(&mut s, 20.0, 15.0);
    click(&mut s, 25.0, 15.0);
    line(&mut s, [10.0, 30.0], [30.0, 30.0]);
    s.execute(CommandId::SketchMirror);
    click(&mut s, 20.0, 30.0); // the first object may be a line
    click(&mut s, 25.0, 15.0); // the circle
    assert_eq!(s.active_sketch().unwrap().picks.len(), 2);
    click(&mut s, 20.0, 30.0); // a picked line toggles off, it is not the mirror line
    assert_eq!(s.active_sketch().unwrap().picks.len(), 1);
    shift_click(&mut s, 20.0, 30.0); // with objects picked, Shift adds a line
    assert_eq!(s.active_sketch().unwrap().picks.len(), 2);
    ev(&mut s, PointerKind::Move, 0.0, 15.0);
    assert_eq!(s.sketch_overlay().unwrap().curves.iter().filter(|c| c.id == PREVIEW_ID).count(), 2, "mirror preview while hovering the line");
    click(&mut s, 0.0, 15.0); // an unpicked line: the mirror line, no Enter needed
    assert!(s.active_sketch().unwrap().picks.is_empty(), "{}", last_msg(&s));
    assert_eq!(lines(&s).len(), 3);
    assert!(point_at(&s, [-20.0, 15.0]).is_some(), "mirrored circle centre");
    assert!(point_at(&s, [-10.0, 30.0]).is_some() && point_at(&s, [-30.0, 30.0]).is_some(), "mirrored line");
    assert!(count(&s, |c| matches!(&c.kind, ConstraintKind::Symmetric { line, .. } if *line == axis)) >= 3);
}

#[test]
fn fillet_line_and_arc_in_the_sketch() {
    let mut s = new_sketch();
    // quarter arc centred (10,10) from (30,10) counter-clockwise to (10,30)
    s.execute(CommandId::ArcCenter);
    click(&mut s, 10.0, 10.0);
    click(&mut s, 30.0, 10.0);
    ev(&mut s, PointerKind::Move, 27.0, 20.0);
    click(&mut s, 10.0, 30.0);
    // a line from the arc start to the right
    line(&mut s, [30.0, 10.0], [60.0, 10.0]);
    assert_eq!(report(&s).status, SolveStatus::Solved);
    let dof = report(&s).dof;
    s.execute(CommandId::SketchFillet);
    ev(&mut s, PointerKind::Move, 30.0, 10.0);
    assert_eq!(s.sketch_overlay().unwrap().curves.iter().filter(|c| c.id == PREVIEW_ID).count(), 1, "fillet preview at the corner: {}", last_msg(&s));
    click(&mut s, 30.0, 10.0); // the shared corner point
    assert!(matches!(s.sketch_prompt(), Some(SketchPrompt::Value { .. })), "radius box opens: {}", last_msg(&s));
    s.sketch_set_value_text("5");
    s.sketch_key(SketchKey::Enter);
    assert!(s.sketch_prompt().is_none(), "{}", last_msg(&s));
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Tangent { .. })), 2);
    let r = report(&s);
    assert_eq!(r.status, SolveStatus::Solved);
    assert_eq!(r.dof, dof, "the fillet and its radius dimension add no freedom: {r:?}");
    // the free sketch re-solves when the radius changes, so check the tangency itself: the
    // fillet lies outside the arc (centres R + 5 apart) and 5 above the line, and joins both
    let k = sk(&s);
    let arcs: Vec<(String, [f64; 2], f64, String, String)> = k
        .entities
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Arc { c, start, end, r } => Some((e.id.clone(), k.point(c).unwrap(), *r, start.clone(), end.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(arcs.len(), 2);
    let fil = arcs.iter().find(|a| close(a.2, 5.0)).expect("radius-5 fillet arc");
    let arc = arcs.iter().find(|a| !close(a.2, 5.0)).unwrap();
    assert!(close(interact::dist(fil.1, arc.1), arc.2 + 5.0), "external tangency");
    let l = lines(&s)[0].clone();
    let (a, b) = k.line_points(&l).unwrap();
    assert!(close(a[1], b[1]) && close(fil.1[1] - a[1], 5.0), "5 above the horizontal line: {a:?} {:?}", fil.1);
    let ends = [fil.3.as_str(), fil.4.as_str()];
    let (lp1, _) = match &k.entity(&l).unwrap().kind {
        EntityKind::Line { p1, p2 } => (p1.clone(), p2.clone()),
        _ => unreachable!(),
    };
    assert!(ends.contains(&lp1.as_str()) && ends.contains(&arc.3.as_str()), "the fillet joins the line and the arc");
    assert!(close(a[0], fil.1[0]), "the line's tangent point is below the fillet centre");
    assert!(point_at(&s, [30.0, 10.0]).is_none(), "the corner is trimmed away");
}

#[test]
fn mirror_produces_symmetric_copies() {
    let mut s = new_sketch();
    line(&mut s, [0.0, -10.0], [0.0, 40.0]);
    let axis = line_at(&s, [0.0, 15.0]);
    rect(&mut s, [10.0, 5.0], [30.0, 25.0]);
    s.execute(CommandId::SketchMirror);
    click(&mut s, 20.0, 5.0);
    // further lines are added with Shift (a plain click on a line would pick the mirror line)
    for p in [[30.0, 15.0], [20.0, 25.0], [10.0, 15.0]] {
        shift_click(&mut s, p[0], p[1]);
    }
    assert_eq!(s.active_sketch().unwrap().picks.len(), 4);
    s.sketch_key(SketchKey::Enter);
    ev(&mut s, PointerKind::Move, 0.0, 15.0);
    assert_eq!(s.sketch_overlay().unwrap().curves.iter().filter(|c| c.id == PREVIEW_ID).count(), 4, "mirror preview");
    click(&mut s, 0.0, 15.0);
    assert_eq!(lines(&s).len(), 9);
    assert_eq!(count(&s, |c| matches!(&c.kind, ConstraintKind::Symmetric { line, .. } if *line == axis)), 4);
    for p in [[-10.0, 5.0], [-30.0, 5.0], [-30.0, 25.0], [-10.0, 25.0]] {
        assert!(point_at(&s, p).is_some(), "mirrored corner {p:?}");
    }
    assert_eq!(areas(&s), vec![400.0, 400.0]);
    // the copies follow their sources
    let w = point_at(&s, [30.0, 25.0]).unwrap();
    let mut moved = sk(&s);
    moved.set_point(&w, [34.0, 25.0]);
    let (a1, a2) = match &moved.entity(&axis).unwrap().kind {
        EntityKind::Line { p1, p2 } => (p1.clone(), p2.clone()),
        _ => unreachable!(),
    };
    let r = cad_core::sketch::solve(&mut moved, &s.dimension_values(), &[w.as_str(), a1.as_str(), a2.as_str()]);
    assert_eq!(r.status, SolveStatus::Solved);
    assert!(moved.entities.iter().any(|e| matches!(e.kind, EntityKind::Point { x, y } if close(x, -34.0) && close(y, 25.0))));
}

#[test]
fn polygon_hexagon_area() {
    let mut s = new_sketch();
    s.execute(CommandId::Polygon);
    assert_eq!(s.polygon_options(), Some((6, true)));
    click(&mut s, 0.1, 0.1); // centre on the origin
    click(&mut s, 20.0, 0.0);
    assert_eq!(lines(&s).len(), 6);
    let a = areas(&s);
    assert_eq!(a.len(), 1);
    assert!(close(a[0], 1.5 * 3f64.sqrt() * 400.0), "{a:?}");
    assert_eq!(report(&s).dof, 2, "size and rotation remain");
    // circumscribed square: the click is an edge midpoint
    s.set_polygon_options(4, false);
    click(&mut s, 100.0, 0.0);
    click(&mut s, 110.0, 0.0);
    assert!(areas(&s).iter().any(|x| close(*x, 400.0)), "{:?}", areas(&s));
}

#[test]
fn slot_area() {
    let mut s = new_sketch();
    s.execute(CommandId::Slot);
    click(&mut s, 10.0, 10.0);
    click(&mut s, 40.0, 10.0);
    click(&mut s, 25.0, 16.0); // width point: radius 6
    let (r, l) = (6.0, 30.0);
    let a = areas(&s);
    assert_eq!(a.len(), 1);
    assert!(close(a[0], std::f64::consts::PI * r * r + 2.0 * r * l), "{a:?}");
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Tangent { .. })), 4);
    assert_eq!(report(&s).dof, 5);
    // a radius dimension keeps the slot tangent
    dimension(&mut s, [4.0, 10.0], [-5.0, 5.0], "4");
    let a = areas(&s);
    assert!(close(a[0], std::f64::consts::PI * 16.0 + 8.0 * l), "{a:?}");
}

#[test]
fn two_point_circle_three_point_rectangle_and_center_arc() {
    let mut s = new_sketch();
    s.execute(CommandId::Circle2Pt);
    click(&mut s, 10.0, 0.0);
    click(&mut s, 30.0, 0.0);
    let c = sk(&s).entities.iter().find(|e| matches!(e.kind, EntityKind::Circle { .. })).unwrap().clone();
    assert!(matches!(c.kind, EntityKind::Circle { r, .. } if close(r, 10.0)));
    assert!(point_at(&s, [20.0, 0.0]).is_some(), "centre between the clicks");

    s.execute(CommandId::Rectangle3Pt);
    click(&mut s, 50.0, 0.0);
    click(&mut s, 80.0, 0.0);
    click(&mut s, 70.0, 20.0);
    assert!(areas(&s).iter().any(|a| close(*a, 600.0)), "{:?}", areas(&s));
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Perpendicular { .. })), 1);
    assert_eq!(count(&s, |c| matches!(c.kind, ConstraintKind::Parallel { .. })), 2);

    // centre point arc: moving counter-clockwise gives the upper-right quarter...
    s.execute(CommandId::ArcCenter);
    click(&mut s, 0.0, 50.0);
    click(&mut s, 10.0, 50.0);
    ev(&mut s, PointerKind::Move, 7.0, 57.0);
    click(&mut s, 0.0, 60.0);
    // ...moving clockwise from the same kind of start gives the lower half
    click(&mut s, 0.0, 80.0);
    click(&mut s, 10.0, 80.0);
    for p in [[7.0, 73.0], [0.0, 70.0], [-7.0, 73.0]] {
        ev(&mut s, PointerKind::Move, p[0], p[1]);
    }
    click(&mut s, -10.0, 80.0);
    let sk = sk(&s);
    let arcs: Vec<&Entity> = sk.entities.iter().filter(|e| matches!(e.kind, EntityKind::Arc { .. })).collect();
    assert_eq!(arcs.len(), 2);
    let below = |e: &Entity| cad_core::sketch::profiles::entity_polyline(&sk, e).iter().all(|p| p[1] <= 80.0 + 1e-9);
    let sweep = |e: &Entity| match &e.kind {
        EntityKind::Arc { c, start, end, .. } => {
            let (c, a, b) = (sk.point(c).unwrap(), sk.point(start).unwrap(), sk.point(end).unwrap());
            ((b[1] - c[1]).atan2(b[0] - c[0]) - (a[1] - c[1]).atan2(a[0] - c[0])).rem_euclid(std::f64::consts::TAU).to_degrees()
        }
        _ => 0.0,
    };
    assert!(close(sweep(arcs[0]), 90.0), "{}", sweep(arcs[0]));
    assert!(close(sweep(arcs[1]), 180.0) && below(arcs[1]), "{}", sweep(arcs[1]));
}
