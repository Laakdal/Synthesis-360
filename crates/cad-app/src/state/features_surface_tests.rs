//! Headless tests of the SURFACE / MESH / UTILITIES dialogs: live preview, OK = one undo step,
//! Cancel restores, timeline edit, canvas routing, Insert Mesh and 3D Print.

use super::*;
use crate::state::{AppState, FileAction, Pick};
use cad_core::{ops, OriginPlane, PlaneRef};
use cad_kernel::BodyKind;
use std::f64::consts::PI;

static OCC_INIT: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn new_state() -> AppState {
    let _guard = OCC_INIT.lock().unwrap_or_else(|e| e.into_inner());
    AppState::new()
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn surface(st: &AppState) -> &SurfaceDialog {
    match st.dialog.as_ref() {
        Some(Dialog::Surface(d)) => d,
        other => panic!("no surface dialog: {:?}", other.map(|d| d.title())),
    }
}

/// Sketch on XY with an open chain of 3 lines (90 mm) and, optionally, a 50 x 30 rectangle
/// away from it. Returns (sketch, chain line ids).
fn sketch_with_chain(st: &mut AppState, rect: bool) -> (String, Vec<String>) {
    let sk = ops::new_sketch(&mut st.doc, PlaneRef::Origin { plane: OriginPlane::XY });
    let s = ops::sketch_mut(&mut st.doc, &sk).unwrap();
    let ids = vec![s.add_line([0.0, 0.0], [40.0, 0.0]), s.add_line([40.0, 0.0], [40.0, 20.0]), s.add_line([40.0, 20.0], [10.0, 20.0])];
    if rect {
        s.add_rectangle([100.0, 0.0], [150.0, 30.0]);
    }
    st.recompute();
    (sk, ids)
}

fn area(st: &AppState, id: &str) -> f64 {
    st.model.body(id).unwrap_or_else(|| panic!("no body {id}")).shape.area()
}

/// 50 x 30 patch made through the Patch dialog. Returns the patch feature id.
fn plate_via_dialog(st: &mut AppState) -> String {
    let sk = ops::new_sketch(&mut st.doc, PlaneRef::Origin { plane: OriginPlane::XY });
    ops::sketch_mut(&mut st.doc, &sk).unwrap().add_rectangle([0.0, 0.0], [50.0, 30.0]);
    st.recompute();
    st.select(Pick::Profile { sketch: sk, index: 0 }, false);
    st.execute(CommandId::Patch);
    assert_eq!(st.dialog_blocker(), None);
    let fid = surface(st).session().feature.clone().expect("patch previewed");
    st.dialog_ok();
    assert!(st.dialog.is_none());
    fid
}

#[test]
fn surface_extrude_dialog_previews_commits_cancels_and_edits() {
    let mut st = new_state();
    let (sk, _) = sketch_with_chain(&mut st, false);
    let before = st.doc.clone();
    // pick the sketch in the timeline (an open chain has no profile to click)
    st.select(Pick::Feature(sk.clone()), false);
    st.execute(CommandId::SurfaceExtrude);
    assert_eq!(st.dialog.as_ref().unwrap().title(), "EXTRUDE");
    let SurfaceDialog::Extrude(x) = surface(&st) else { panic!() };
    assert_eq!(x.curves.len(), 3, "every chain of the sketch is selected");
    let fid = x.s.feature.clone().expect("live preview");
    assert!(close(area(&st, &fid), 90.0 * 10.0, 1e-6), "default 10 mm preview");
    assert!(st.dialog_status().unwrap().starts_with("3 Curves | 1 Chain | Length : 90.00 mm"), "{:?}", st.dialog_status());
    assert!(st.dialog_set_primary("15 mm"));
    assert!(close(area(&st, &fid), 90.0 * 15.0, 1e-6));
    // Cancel restores the document
    st.dialog_cancel();
    assert_eq!(st.doc, before);
    assert!(st.model.bodies.is_empty());

    // again, then OK: one undo step, sketch auto-hidden, surface body
    st.select(Pick::Feature(sk.clone()), false);
    st.execute(CommandId::SurfaceExtrude);
    assert!(st.dialog_set_primary("15 mm"));
    st.dialog_ok();
    assert!(st.dialog.is_none());
    let fid = st.doc.features.last().unwrap().id.clone();
    let b = st.model.body(&fid).unwrap();
    assert_eq!(b.kind, BodyKind::Surface);
    assert_eq!(b.volume(), 0.0);
    assert!(matches!(st.doc.feature(&sk).unwrap().kind, cad_core::FeatureKind::Sketch { visible: false, .. }));
    assert_eq!(st.history.undo_label(), Some("Extrude"));

    // timeline edit reopens the dialog with its values
    st.edit_feature(&fid);
    assert_eq!(st.dialog.as_ref().unwrap().title(), "EDIT FEATURE");
    let SurfaceDialog::Extrude(x) = surface(&st) else { panic!() };
    assert_eq!(x.distance.text, "15 mm");
    assert_eq!(x.curves.len(), 3);
    assert!(st.dialog_set_primary("20 mm"));
    st.dialog_edit(|d| {
        if let Dialog::Surface(SurfaceDialog::Extrude(x)) = d {
            x.direction = cad_core::ExtentDirection::Symmetric;
        }
    });
    st.dialog_ok();
    assert!(close(area(&st, &fid), 90.0 * 20.0, 1e-6));
    let bb = st.model.body(&fid).unwrap().topology().bbox;
    assert!(close(bb[2], -10.0, 1e-6), "{bb:?}");
    st.undo();
    assert!(close(area(&st, &fid), 90.0 * 15.0, 1e-6));
}

#[test]
fn profile_click_selects_its_chain_and_the_chain_list_toggles_others() {
    let mut st = new_state();
    let (sk, chain) = sketch_with_chain(&mut st, true);
    // the rectangle is the only profile
    st.select(Pick::Profile { sketch: sk.clone(), index: 0 }, false);
    st.execute(CommandId::SurfaceExtrude);
    let SurfaceDialog::Extrude(x) = surface(&st) else { panic!() };
    assert_eq!(x.curves.len(), 4, "the rectangle's 4 lines");
    assert!(chain.iter().all(|c| !x.curves.contains(c)));
    // the profile is highlighted
    assert_eq!(st.selection, vec![Pick::Profile { sketch: sk.clone(), index: 0 }]);
    // switch the open chain on from the list
    let chains = st.sketch_curve_chains(&x.s.base.0, &sk);
    assert_eq!(chains.len(), 2);
    let open = chains.iter().position(|c| !c.closed).unwrap();
    st.surface_toggle_chain(open);
    let SurfaceDialog::Extrude(x) = surface(&st) else { panic!() };
    assert_eq!(x.curves.len(), 7);
    let fid = x.s.feature.clone().unwrap();
    assert!(close(area(&st, &fid), (90.0 + 160.0) * 10.0, 1e-6));
    // clicking the profile again removes the rectangle's chain
    st.hover = Some(Pick::Profile { sketch: sk.clone(), index: 0 });
    st.select(Pick::Profile { sketch: sk.clone(), index: 0 }, true);
    let SurfaceDialog::Extrude(x) = surface(&st) else { panic!() };
    assert_eq!(x.curves.len(), 3, "{:?}", x.curves);
    st.dialog_ok();
    assert!(close(area(&st, &fid), 90.0 * 10.0, 1e-6));
}

#[test]
fn surface_revolve_dialog_uses_the_default_axis() {
    let mut st = new_state();
    let sk = ops::new_sketch(&mut st.doc, PlaneRef::Origin { plane: OriginPlane::XY });
    ops::sketch_mut(&mut st.doc, &sk).unwrap().add_line([10.0, 0.0], [10.0, 20.0]);
    st.recompute();
    st.execute(CommandId::SurfaceRevolve);
    assert_eq!(st.dialog_blocker().as_deref(), Some("Select a sketch and the curves to revolve."));
    st.surface_set_sketch(&sk);
    let SurfaceDialog::Revolve(x) = surface(&st) else { panic!() };
    assert!(x.axis.is_some(), "an origin axis in the sketch plane");
    st.dialog_edit(|d| {
        if let Dialog::Surface(SurfaceDialog::Revolve(x)) = d {
            x.axis = Some(cad_core::RevolveAxis::Origin { axis: cad_core::OriginAxis::Y });
        }
    });
    st.dialog_ok();
    let fid = st.doc.features.last().unwrap().id.clone();
    assert!(close(area(&st, &fid), 2.0 * PI * 10.0 * 20.0, 1e-4), "{}", area(&st, &fid));
    st.edit_feature(&fid);
    assert!(st.dialog_set_primary("180 deg"));
    st.dialog_ok();
    assert!(close(area(&st, &fid), PI * 10.0 * 20.0, 1e-4));
}

#[test]
fn patch_then_thicken_and_offset_through_the_dialogs() {
    let mut st = new_state();
    let p = plate_via_dialog(&mut st);
    assert_eq!(st.model.body(&p).unwrap().kind, BodyKind::Surface);
    assert!(close(area(&st, &p), 1500.0, 1e-6));
    assert_eq!(st.measure_item(&Pick::Body { body: p.clone() }).unwrap().volume, Some(0.0));

    // Offset: a body pick adds all its faces
    st.select(Pick::Body { body: p.clone() }, false);
    st.execute(CommandId::OffsetSurface);
    let SurfaceDialog::Offset(x) = surface(&st) else { panic!() };
    assert_eq!(x.faces.len(), 1);
    assert_eq!(st.dialog_status().as_deref(), Some("1 Face"));
    st.dialog_ok();
    let o = st.doc.features.last().unwrap().id.clone();
    let bb = st.model.body(&o).unwrap().topology().bbox;
    assert!(close(bb[2].abs(), 5.0, 1e-6), "{bb:?}");

    // Thicken the plate (face click), symmetric
    st.hover = Some(Pick::Face { body: p.clone(), face: 0 });
    st.select(Pick::Face { body: p.clone(), face: 0 }, false);
    st.execute(CommandId::Thicken);
    assert_eq!(st.dialog.as_ref().unwrap().title(), "THICKEN");
    st.dialog_edit(|d| {
        if let Dialog::Surface(SurfaceDialog::Thicken(x)) = d {
            x.symmetric = true;
        }
    });
    st.dialog_ok();
    let t = st.doc.features.last().unwrap().id.clone();
    let b = st.model.body(&t).unwrap();
    assert_eq!(b.kind, BodyKind::Solid);
    assert!(close(b.volume(), 3000.0, 1e-6));
    assert!(st.model.body(&p).is_none(), "the thickened surface is consumed");
    // edit the thickness
    st.edit_feature(&t);
    assert!(st.dialog_set_primary("4 mm"));
    st.dialog_ok();
    assert!(close(st.model.body(&t).unwrap().volume(), 6000.0, 1e-6));
}

#[test]
fn stitch_dialog_accepts_surfaces_only() {
    let mut st = new_state();
    let a = plate_via_dialog(&mut st);
    // a second plate sharing the edge x = 50 (vertical, on the YZ offset plane would need a
    // construction plane; use a coplanar neighbour instead)
    let sk = ops::new_sketch(&mut st.doc, PlaneRef::Origin { plane: OriginPlane::XY });
    ops::sketch_mut(&mut st.doc, &sk).unwrap().add_rectangle([50.0, 0.0], [80.0, 30.0]);
    st.recompute();
    st.select(Pick::Profile { sketch: sk, index: 0 }, false);
    st.execute(CommandId::Patch);
    st.dialog_ok();
    let b = st.doc.features.last().unwrap().id.clone();
    // a solid box elsewhere
    let bx = ops::new_sketch(&mut st.doc, PlaneRef::Origin { plane: OriginPlane::XY });
    ops::sketch_mut(&mut st.doc, &bx).unwrap().add_rectangle([200.0, 0.0], [210.0, 10.0]);
    st.recompute();
    let pr = st.model.sketches[&bx].profiles[0].clone();
    let solid = ops::new_extrude(&mut st.doc, &bx, vec![cad_core::ProfileRef { curves: pr.curves.clone(), sample: pr.interior_point() }], "5 mm", cad_core::ExtentDirection::Normal, cad_core::Operation::New);
    st.recompute();

    st.select(Pick::Body { body: a.clone() }, false);
    st.execute(CommandId::Stitch);
    st.hover = Some(Pick::Body { body: solid.clone() });
    st.select(Pick::Body { body: solid.clone() }, true);
    let SurfaceDialog::Stitch(x) = surface(&st) else { panic!() };
    assert_eq!(x.bodies, vec![a.clone()], "the solid is refused");
    assert!(x.s.notice.as_deref().unwrap_or("").contains("surface bodies"));
    st.hover = Some(Pick::Body { body: b.clone() });
    st.select(Pick::Body { body: b.clone() }, true);
    let SurfaceDialog::Stitch(x) = surface(&st) else { panic!() };
    assert_eq!(x.bodies.len(), 2);
    st.dialog_ok();
    let stitched = st.model.body(&a).unwrap();
    assert_eq!(stitched.kind, BodyKind::Surface);
    assert_eq!(stitched.topology().faces.len(), 2);
    assert!(st.model.body(&b).is_none(), "the second surface is consumed");
    assert!(close(stitched.shape.area(), 2400.0, 1e-6));
    // the shared edge is stitched: 7 unique edges instead of 8
    assert_eq!(stitched.topology().edges.len(), 7);
}

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cad360-surface-app-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// A state with one 40 x 20 x 10 box.
fn box_state() -> AppState {
    let mut st = new_state();
    let sk = ops::new_sketch(&mut st.doc, PlaneRef::Origin { plane: OriginPlane::XY });
    ops::sketch_mut(&mut st.doc, &sk).unwrap().add_rectangle([0.0, 0.0], [40.0, 20.0]);
    st.recompute();
    let pr = st.model.sketches[&sk].profiles[0].clone();
    ops::new_extrude(&mut st.doc, &sk, vec![cad_core::ProfileRef { curves: pr.curves.clone(), sample: pr.interior_point() }], "10 mm", cad_core::ExtentDirection::Normal, cad_core::Operation::New);
    st.recompute();
    st
}

#[test]
fn insert_mesh_file_previews_a_mesh_body_and_converts_it() {
    let mut src = box_state();
    let path = temp("box.stl");
    assert!(src.export_stl(&path));

    let mut st = new_state();
    assert!(st.insert_mesh_file(&path));
    assert_eq!(st.dialog.as_ref().unwrap().title(), "INSERT MESH");
    assert_eq!(st.dialog_status().as_deref(), Some("box.stl | 12 Triangles"));
    st.dialog_ok();
    let m = st.doc.features.last().unwrap().id.clone();
    assert_eq!(st.history.undo_label(), Some("Insert Mesh"));
    let b = st.model.body(&m).unwrap();
    assert_eq!(b.kind, BodyKind::Mesh);
    assert!(close(b.volume(), 8000.0, 1e-3));
    assert!(close(st.measure_item(&Pick::Body { body: m.clone() }).unwrap().surface_area.unwrap(), 2800.0, 1e-3));
    assert_eq!(b.mesh().indices.len(), 36);

    // edit: Convert to B-rep
    st.edit_feature(&m);
    st.dialog_edit(|d| {
        if let Dialog::Surface(SurfaceDialog::Mesh(x)) = d {
            x.to_brep = true;
        }
    });
    st.dialog_ok();
    let b = st.model.body(&m).unwrap();
    assert_eq!(b.kind, BodyKind::Solid);
    assert_eq!(b.topology().faces.len(), 6);
    st.undo();
    assert_eq!(st.model.body(&m).unwrap().kind, BodyKind::Mesh);

    // the command without a file waits for one; a bad file is reported
    st.execute(CommandId::InsertMesh);
    assert_eq!(st.dialog_blocker().as_deref(), Some("Select an STL file."));
    let bad = temp("bad.stl");
    std::fs::write(&bad, b"garbage").unwrap();
    assert!(!st.insert_mesh_file(&bad));
    assert_eq!(st.last_message().unwrap().level, crate::state::Level::Error);
    // the panel's file pick goes through the dialog payload
    st.dialog_edit(|d| {
        if let Dialog::Surface(SurfaceDialog::Mesh(x)) = d {
            x.file = Some(path.clone());
        }
    });
    assert!(surface(&st).session().feature.is_some());
    st.dialog_cancel();
    assert_eq!(st.model.bodies.len(), 1);
}

#[test]
fn three_d_print_requests_an_stl_export_with_its_refinement() {
    let mut st = box_state();
    // a cylinder so the refinement matters
    let sk = ops::new_sketch(&mut st.doc, PlaneRef::Origin { plane: OriginPlane::XY });
    ops::sketch_mut(&mut st.doc, &sk).unwrap().add_circle([100.0, 0.0], 20.0);
    st.recompute();
    let pr = st.model.sketches[&sk].profiles[0].clone();
    let cyl = ops::new_extrude(&mut st.doc, &sk, vec![cad_core::ProfileRef { curves: pr.curves.clone(), sample: pr.interior_point() }], "10 mm", cad_core::ExtentDirection::Normal, cad_core::Operation::New);
    st.recompute();
    let undo_before = st.history.undo_label().map(str::to_string);

    let mut counts = Vec::new();
    for r in [cad_kernel::mesh::Refinement::Coarse, cad_kernel::mesh::Refinement::Fine] {
        st.select(Pick::Body { body: cyl.clone() }, false);
        st.execute(CommandId::ThreeDPrint);
        assert_eq!(st.dialog.as_ref().unwrap().title(), "3D PRINT");
        assert_eq!(st.dialog_status().as_deref(), Some("1 Body to export"));
        st.dialog_edit(|d| {
            if let Dialog::Surface(SurfaceDialog::Print(x)) = d {
                x.refinement = r;
            }
        });
        st.dialog_ok();
        assert_eq!(st.file_request, Some(FileAction::ExportStl), "OK asks for the file name");
        st.file_request = None;
        let SurfaceDialog::Print(x) = surface(&st) else { panic!() };
        assert!(x.pending);
        let path = temp(&format!("print-{}.stl", r.label()));
        assert!(st.export_stl_requested(&path));
        assert!(st.dialog.is_none(), "the export closes the dialog");
        counts.push(cad_kernel::mesh::stl_triangle_count(&std::fs::read(&path).unwrap()));
    }
    assert!(counts[0] < counts[1], "coarse {} < fine {}", counts[0], counts[1]);
    assert_eq!(st.history.undo_label().map(str::to_string), undo_before, "printing changes nothing");

    // no selection: every visible body; a cancelled file dialog re-arms the dialog
    st.clear_selection();
    st.execute(CommandId::ThreeDPrint);
    assert_eq!(st.dialog_status().as_deref(), Some("2 Bodies to export"));
    st.dialog_ok();
    st.export_stl_cancelled();
    let SurfaceDialog::Print(x) = surface(&st) else { panic!() };
    assert!(!x.pending);
    st.dialog_cancel();
    // without a pending print the request is the plain STL export
    let plain = temp("plain.stl");
    assert!(st.export_stl_requested(&plain));
    assert!(plain.exists());
}

#[test]
fn surface_commands_wait_for_the_sketch_to_finish() {
    let mut st = new_state();
    st.select(Pick::OriginPlane(OriginPlane::XY), false);
    st.execute(CommandId::CreateSketch);
    assert!(matches!(st.mode, crate::state::Mode::Sketch(_)));
    for id in [CommandId::SurfaceExtrude, CommandId::Patch, CommandId::ThreeDPrint] {
        // disabled while sketching (greyed in the ribbon); executing it opens nothing
        assert!(!st.is_enabled(id), "{id:?}");
        st.execute(id);
        assert!(st.dialog.is_none(), "{id:?}");
    }
    assert!(st.last_message().unwrap().text.contains("not available right now"));
}
