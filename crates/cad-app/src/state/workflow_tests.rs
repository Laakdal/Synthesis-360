//! Phase-7 functional workflow, executed headlessly through AppState (the same calls the UI
//! makes): new document → sketch plane → rectangle → dimensions/constraints → finish → extrude →
//! edit distance → edit sketch → undo/redo → save → reopen → export.

use super::*;
use cad_core::sketch::{ConstraintKind as K, DistanceOrientation as O, EntityKind, ORIGIN};
use cad_core::{ExtentDirection, FeatureKind, Operation, OriginPlane, ProfileRef};

fn tmp(name: &str) -> std::path::PathBuf {
    // per-process directory: concurrent test runs (other sessions/agents) must not share files
    let d = std::env::temp_dir().join(format!("cad360-app-tests-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

fn volume(s: &AppState) -> f64 {
    s.model.bodies.iter().map(|b| b.volume()).sum()
}

#[test]
fn phase7_workflow_headless() {
    // 1. new document
    let mut s = AppState::new();
    s.new_design();
    assert!(s.doc.features.is_empty());

    // 2. sketch plane: Create Sketch with nothing selected prompts for a plane (Fusion F8)
    s.execute(CommandId::CreateSketch);
    assert!(s.plane_prompt, "Create Sketch without a selection waits for a plane");
    s.select(Pick::OriginPlane(OriginPlane::XY), false);
    s.execute(CommandId::CreateSketch);
    let sk = s.active_sketch().expect("sketch mode entered").feature.clone();

    // 3. rectangle (geometry like the 2-Point Rectangle tool: 2H + 2V, Fusion F13)
    let lines = cad_core::ops::sketch_mut(&mut s.doc, &sk).unwrap().add_rectangle([2.0, 3.0], [40.0, 25.0]);
    let sketch = s.doc.feature(&sk).unwrap().sketch().unwrap();
    let hv = sketch.constraints.iter().filter(|c| matches!(c.kind, K::Horizontal { .. } | K::Vertical { .. })).count();
    assert_eq!(hv, 4);

    // 4. dimensions and constraints -> fully constrained (Fusion F17)
    let corner = match &sketch.entity(&lines[0]).unwrap().kind {
        EntityKind::Line { p1, .. } => p1.clone(),
        _ => unreachable!(),
    };
    cad_core::ops::sketch_mut(&mut s.doc, &sk).unwrap().add_constraint(K::Coincident { a: corner, b: ORIGIN.into() });
    let (_, pw) = cad_core::ops::add_dimension(&mut s.doc, &sk, K::Distance { a: lines[0].clone(), b: lines[0].clone(), orientation: O::Horizontal }, "50 mm", [25.0, -6.0], false).unwrap();
    cad_core::ops::add_dimension(&mut s.doc, &sk, K::Distance { a: lines[1].clone(), b: lines[1].clone(), orientation: O::Vertical }, "30 mm", [56.0, 15.0], false).unwrap();
    s.recompute();
    assert!(s.model.sketches[&sk].report.is_fully_constrained());

    // 5. finish sketch = one undo step
    let undo_before = s.history.undo_label().map(str::to_string);
    s.execute(CommandId::FinishSketch);
    assert!(s.active_sketch().is_none());
    assert_ne!(s.history.undo_label().map(str::to_string), undo_before);

    // 6. extrude the profile
    let p = &s.model.sketches[&sk].profiles[0];
    let pref = ProfileRef { curves: p.curves.clone(), sample: p.interior_point() };
    let sk2 = sk.clone();
    assert!(s.apply("Extrude", |d| {
        cad_core::ops::new_extrude(d, &sk2, vec![pref], "10 mm", ExtentDirection::Normal, Operation::New);
        Ok(())
    }));
    assert!((volume(&s) - 15000.0).abs() < 1e-6);
    let ext = s.doc.features.last().unwrap().id.clone();
    let dist = cad_core::ops::primary_parameter(s.doc.feature(&ext).unwrap()).unwrap().to_string();

    // 7-8. modify the extrusion distance -> solid changes
    assert!(s.set_parameter(&dist, "20 mm"));
    assert!((volume(&s) - 30000.0).abs() < 1e-6);
    // invalid parameter is rejected with an error message and changes nothing
    assert!(!s.set_parameter(&dist, "15 deg"));
    assert_eq!(s.last_message().unwrap().level, Level::Error);
    assert!((volume(&s) - 30000.0).abs() < 1e-6);

    // 9-10. edit the original sketch -> dependent extrude recomputes
    s.edit_sketch(&sk);
    assert!(s.active_sketch().is_some());
    assert_eq!(s.health(&ext), cad_kernel::Health::RolledBack, "downstream features roll back while editing");
    s.doc.set_expression(&pw, "60 mm").unwrap();
    s.recompute();
    s.finish_sketch();
    assert_eq!(s.health(&ext), cad_kernel::Health::Healthy);
    assert!((volume(&s) - 36000.0).abs() < 1e-6);

    // 11. undo / redo
    s.undo();
    assert!((volume(&s) - 30000.0).abs() < 1e-6, "undo of the sketch edit");
    s.redo();
    assert!((volume(&s) - 36000.0).abs() < 1e-6, "redo of the sketch edit");

    // 12-14. save, reopen, verify geometry, parameters and history
    let file = tmp("workflow.c360");
    assert!(s.save_to(&file));
    let mut t = AppState::new();
    assert!(t.open(&file));
    assert_eq!(t.doc.features.len(), 2);
    assert_eq!(t.doc.parameters, s.doc.parameters);
    assert!((volume(&t) - 36000.0).abs() < 1e-6);
    assert!(matches!(t.doc.features[1].kind, FeatureKind::Extrude { .. }));
    assert!(t.model.sketches[&sk].report.is_fully_constrained());

    // 15. export STEP / STL and verify the exported result
    let step = tmp("workflow.step");
    let stl = tmp("workflow.stl");
    assert!(t.export_step(&step));
    assert!(t.export_stl(&stl));
    let text = std::fs::read_to_string(&step).unwrap();
    assert!(text.starts_with("ISO-10303-21") && text.contains("MANIFOLD_SOLID_BREP"));
    let stl_bytes = std::fs::read(&stl).unwrap();
    let n_tri = u32::from_le_bytes(stl_bytes[80..84].try_into().unwrap()) as usize;
    assert_eq!(stl_bytes.len(), 84 + n_tri * 50, "binary STL layout");
    assert!(n_tri >= 12);
    // re-import the STEP: same volume
    let mut u = AppState::new();
    assert!(u.import_step(&step));
    assert!((volume(&u) - 36000.0).abs() < 0.5);
}

#[test]
fn errors_are_reported_not_crashing() {
    let mut s = AppState::new();
    assert!(!s.open(std::path::Path::new("Z:/definitely/missing.c360")));
    assert_eq!(s.last_message().unwrap().level, Level::Error);
    let bad = tmp("bad.c360");
    std::fs::write(&bad, "{ not json").unwrap();
    assert!(!s.open(&bad));
    s.execute(CommandId::Sweep);
    assert!(s.last_message().unwrap().text.contains(NOT_SUPPORTED));
    assert!(!s.export_step(&tmp("empty.step")), "nothing to export");
}

#[test]
fn timeline_marker_suppress_and_delete() {
    let mut s = AppState::new();
    s.select(Pick::OriginPlane(OriginPlane::XY), false);
    s.execute(CommandId::CreateSketch);
    let sk = s.active_sketch().unwrap().feature.clone();
    cad_core::ops::sketch_mut(&mut s.doc, &sk).unwrap().add_rectangle([0.0, 0.0], [10.0, 10.0]);
    s.recompute();
    s.finish_sketch();
    let p = &s.model.sketches[&sk].profiles[0];
    let pref = ProfileRef { curves: p.curves.clone(), sample: p.interior_point() };
    let sk2 = sk.clone();
    s.apply("Extrude", |d| {
        cad_core::ops::new_extrude(d, &sk2, vec![pref], "5 mm", ExtentDirection::Normal, Operation::New);
        Ok(())
    });
    let ext = s.doc.features[1].id.clone();
    assert_eq!(s.model.bodies.len(), 1);
    s.set_marker(1);
    assert!(s.model.bodies.is_empty());
    assert_eq!(s.health(&ext), cad_kernel::Health::RolledBack);
    s.set_marker(2);
    s.toggle_suppress(&ext);
    assert_eq!(s.health(&ext), cad_kernel::Health::Suppressed);
    s.toggle_suppress(&ext);
    assert_eq!(s.model.bodies.len(), 1);
    s.delete_feature(&sk);
    assert!(s.health(&ext).is_error(), "the extrude's sketch is gone");
    s.undo();
    assert_eq!(s.health(&ext), cad_kernel::Health::Healthy);
}

// ---------------------------------------------------------------------------------------------
// The same workflow driven only through the interactive paths the canvas and dialogs use:
// tool commands, pointer events (Move, Press, Release, Click as app.rs emits them), the dimension
// value box, the Extrude dialog, timeline edit (edit_feature) and the sketch value editor.

use crate::state::sketch::{PointerKind, SketchPointer};

fn tap(s: &mut AppState, x: f64, y: f64) {
    for kind in [PointerKind::Move, PointerKind::Press, PointerKind::Release, PointerKind::Click] {
        s.sketch_pointer(SketchPointer { pos: [x, y], tolerance: 0.5, kind, shift: false, ctrl: false });
    }
}

fn dimension_with_value(s: &AppState, value: f64) -> String {
    let sk = s.doc.features.iter().find(|f| f.sketch().is_some()).unwrap();
    let vals = s.doc.parameter_values().values;
    sk.sketch()
        .unwrap()
        .constraints
        .iter()
        .find(|c| c.param.as_ref().is_some_and(|p| (vals.get(p).copied().unwrap_or(f64::NAN) - value).abs() < 1e-9))
        .map(|c| c.id.clone())
        .expect("dimension with that value")
}

#[test]
fn phase7_workflow_through_tools_and_dialogs() {
    let mut s = AppState::new();
    // 1-2. new document, Create Sketch prompt, pick XY
    s.execute(CommandId::NewDesign);
    s.execute(CommandId::CreateSketch);
    assert!(s.plane_prompt);
    s.select(Pick::OriginPlane(OriginPlane::XY), false);
    s.execute(CommandId::CreateSketch);
    let sk = s.active_sketch().expect("sketch mode").feature.clone();

    // 3. 2-Point Rectangle tool: first corner snaps onto the origin
    s.execute(CommandId::Rectangle2Pt);
    tap(&mut s, 0.0, 0.0);
    tap(&mut s, 50.0, 30.0);
    let sketch = s.doc.feature(&sk).unwrap().sketch().unwrap();
    assert_eq!(sketch.entities.iter().filter(|e| matches!(e.kind, EntityKind::Line { .. })).count(), 4);
    assert_eq!(sketch.constraints.iter().filter(|c| matches!(c.kind, K::Horizontal { .. } | K::Vertical { .. })).count(), 4);

    // 4. Sketch Dimension tool: pick an edge, place the label, type the value
    s.execute(CommandId::SketchDimension);
    tap(&mut s, 25.0, 0.0);
    tap(&mut s, 25.0, -8.0);
    s.sketch_set_value_text("50 mm");
    s.sketch_value_commit();
    tap(&mut s, 50.0, 15.0);
    tap(&mut s, 58.0, 15.0);
    s.sketch_set_value_text("30 mm");
    s.sketch_value_commit();
    assert!(s.model.sketches[&sk].report.is_fully_constrained(), "rectangle on the origin + 2 dimensions");

    // 5. Finish Sketch (one undo step)
    s.execute(CommandId::FinishSketch);
    assert!(s.active_sketch().is_none());

    // 6. select the profile, Extrude dialog, type the distance, OK
    s.select(Pick::Profile { sketch: sk.clone(), index: 0 }, false);
    s.execute(CommandId::Extrude);
    assert!(s.dialog.is_some(), "Extrude dialog open");
    assert!(s.dialog_set_primary("10 mm"));
    s.dialog_ok();
    assert!(s.dialog.is_none());
    assert!((volume(&s) - 15000.0).abs() < 1e-6, "{}", volume(&s));
    let ext = s.doc.features.iter().find(|f| matches!(f.kind, FeatureKind::Extrude { .. })).unwrap().id.clone();

    // 7-8. timeline edit of the extrude: invalid value rejected, then 20 mm
    s.edit_feature(&ext);
    assert!(s.dialog.is_some(), "edit reopens the dialog");
    assert!(!s.dialog_set_primary("15 deg"), "unit mismatch is rejected in the dialog");
    assert!(s.dialog_set_primary("20 mm"));
    s.dialog_ok();
    assert!((volume(&s) - 30000.0).abs() < 1e-6);

    // 9-10. edit the sketch: open the width dimension's value box and type 60 mm
    s.edit_feature(&sk);
    assert!(s.active_sketch().is_some());
    let width = dimension_with_value(&s, 50.0);
    s.open_value_edit(&width);
    s.sketch_set_value_text("60 mm");
    s.sketch_value_commit();
    s.execute(CommandId::FinishSketch);
    assert_eq!(s.health(&ext), cad_kernel::Health::Healthy);
    assert!((volume(&s) - 36000.0).abs() < 1e-6, "{}", volume(&s));

    // 11. undo / redo through the commands
    s.execute(CommandId::Undo);
    assert!((volume(&s) - 30000.0).abs() < 1e-6);
    s.execute(CommandId::Redo);
    assert!((volume(&s) - 36000.0).abs() < 1e-6);

    // 12-14. save / reopen
    let file = tmp("workflow-ui.c360");
    assert!(s.save_to(&file));
    let mut t = AppState::new();
    assert!(t.open(&file));
    assert_eq!(t.doc.parameters, s.doc.parameters);
    assert!((volume(&t) - 36000.0).abs() < 1e-6);
    assert!(t.model.sketches[&sk].report.is_fully_constrained());

    // 15. export and verify by re-import
    let step = tmp("workflow-ui.step");
    assert!(t.export_step(&step));
    let mut u = AppState::new();
    assert!(u.import_step(&step));
    assert!((volume(&u) - 36000.0).abs() < 0.5);
}
