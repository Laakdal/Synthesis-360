//! Headless tests of the INSPECT tools: Measure (minimum distance, angle, closest-point
//! markers), Interference and Center of Mass.

use super::*;
use crate::state::command::{spec, Panel};
use crate::state::{AppState, Pick};
use cad_core::{ops, CurveKind, OriginPlane, PlaneRef, SurfaceKind};

fn new_state() -> AppState {
    static OCC_INIT: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = OCC_INIT.lock().unwrap_or_else(|e| e.into_inner());
    AppState::new()
}

/// Unconstrained w x h x d box (corner at `at`) as a new body; returns its body id.
fn add_box(st: &mut AppState, at: [f64; 2], w: f64, h: f64, d: &str) -> String {
    let sk = ops::new_sketch(&mut st.doc, PlaneRef::Origin { plane: OriginPlane::XY });
    ops::sketch_mut(&mut st.doc, &sk).unwrap().add_rectangle(at, [at[0] + w, at[1] + h]);
    st.recompute();
    let p = &st.model.sketches[&sk].profiles[0];
    let prof = vec![ProfileRef { curves: p.curves.clone(), sample: p.interior_point() }];
    let ext = ops::new_extrude(&mut st.doc, &sk, prof, d, ExtentDirection::Normal, Operation::New);
    st.recompute();
    assert_eq!(st.model.health_of(&ext), Health::Healthy);
    assert!(st.model.body(&ext).is_some());
    ext
}

fn click(st: &mut AppState, p: Pick) {
    st.hover = Some(p.clone());
    st.select(p, false);
    st.hover = None;
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn face_with_normal(st: &AppState, body: &str, n: [f64; 3]) -> Pick {
    let t = st.model.body(body).unwrap().topology();
    let i = t.face_info.iter().position(|f| f.surface == SurfaceKind::Plane && cad_kernel::frame::dot(f.normal, n) > 0.999).unwrap();
    Pick::Face { body: body.into(), face: i }
}

fn vertical_edge(st: &AppState, body: &str) -> Pick {
    let t = st.model.body(body).unwrap().topology();
    let i = t.edge_info.iter().position(|e| e.curve == CurveKind::Line && e.dir[2].abs() > 0.999).unwrap();
    Pick::Edge { body: body.into(), edge: i }
}

fn inspect_dialog(st: &AppState) -> &InspectDialog {
    match &st.dialog {
        Some(Dialog::Inspect(x)) => x,
        d => panic!("no inspect dialog: {d:?}"),
    }
}

fn compute(st: &mut AppState) {
    let Some(Dialog::Inspect(mut x)) = st.dialog.take() else { panic!("no inspect dialog") };
    st.inspect_compute(&mut x);
    st.dialog = Some(Dialog::Inspect(x));
}

#[test]
fn inspect_commands_are_registered_and_enabled() {
    let st = new_state();
    for id in [CommandId::Measure, CommandId::Interference, CommandId::CenterOfMass] {
        assert!(spec(id).supported && spec(id).panel == Panel::Inspect, "{id:?}");
        assert!(st.is_enabled(id), "{id:?}");
    }
}

#[test]
fn measure_minimum_distance_angle_and_markers() {
    let mut st = new_state();
    let a = add_box(&mut st, [0.0, 0.0], 20.0, 20.0, "10 mm");
    let b = add_box(&mut st, [30.0, 0.0], 10.0, 10.0, "10 mm");
    st.execute(CommandId::Measure);
    assert!(st.inspect_markers().is_empty());
    click(&mut st, Pick::Body { body: a.clone() });
    st.hover = Some(Pick::Body { body: b.clone() });
    st.select(Pick::Body { body: b.clone() }, true);
    let r = st.measure_report();
    assert_eq!(r.items.len(), 2);
    let m = r.min.expect("minimum distance between two bodies");
    assert!(close(m.distance, 10.0) && close(r.distance.unwrap(), 10.0), "{r:?}");
    assert!(close(m.a[0], 20.0) && close(m.b[0], 30.0), "closest points on the facing faces: {m:?}");
    assert!(close(r.delta.unwrap()[0], 10.0));
    // the bounding-box centres are further apart
    assert!(close(r.center_distance.unwrap(), (25.0f64 * 25.0 + 5.0 * 5.0).sqrt()), "{:?}", r.center_distance);
    assert_eq!(r.angle, None, "no angle between bodies");
    let mk = st.inspect_markers();
    assert_eq!(mk.len(), 3);
    assert!(matches!(mk[0], InspectMarker::Segment(p, q) if close(p[0], 20.0) && close(q[0], 30.0)));
    // the cached result is reused and stays correct
    assert_eq!(st.measure_report().min, Some(m));

    // two adjacent faces: 90 deg, touching
    let set_picks = |st: &mut AppState, picks: Vec<Pick>| {
        if let Some(Dialog::Measure(d)) = st.dialog.as_mut() {
            d.picks = picks;
        }
    };
    let picks = vec![face_with_normal(&st, &a, [0.0, 0.0, 1.0]), face_with_normal(&st, &a, [1.0, 0.0, 0.0])];
    set_picks(&mut st, picks);
    let r = st.measure_report();
    assert!(close(r.angle.unwrap(), 90.0) && r.min.unwrap().distance.abs() < 1e-7, "{r:?}");
    // a vertical edge and the top face of the other box: 90 deg; parallel faces: 0 deg, 10 apart
    let picks = vec![vertical_edge(&st, &a), face_with_normal(&st, &b, [0.0, 0.0, 1.0])];
    set_picks(&mut st, picks);
    assert!(close(st.measure_report().angle.unwrap(), 90.0));
    let picks = vec![face_with_normal(&st, &a, [1.0, 0.0, 0.0]), face_with_normal(&st, &b, [-1.0, 0.0, 0.0])];
    set_picks(&mut st, picks);
    let r = st.measure_report();
    assert!(close(r.angle.unwrap(), 0.0) && close(r.distance.unwrap(), 10.0), "{r:?}");
    st.dialog_ok();
    assert!(st.dialog.is_none() && st.inspect_markers().is_empty());
}

#[test]
fn interference_dialog_reports_overlapping_pairs() {
    let mut st = new_state();
    let a = add_box(&mut st, [0.0, 0.0], 20.0, 20.0, "10 mm");
    let b = add_box(&mut st, [10.0, 5.0], 20.0, 10.0, "10 mm");
    let c = add_box(&mut st, [100.0, 0.0], 10.0, 10.0, "10 mm");
    let undo_before = st.history.can_undo();
    st.execute(CommandId::Interference);
    assert_eq!(st.dialog.as_ref().unwrap().title(), "INTERFERENCE");
    assert!(inspect_dialog(&st).interference.is_none(), "computed on request");
    // nothing selected: every visible body
    compute(&mut st);
    let x = inspect_dialog(&st);
    assert_eq!(x.checked, 3);
    let rows = x.interference.as_ref().unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!((rows[0].a.as_str(), rows[0].b.as_str()), (a.as_str(), b.as_str()));
    assert!(close(rows[0].volume, 10.0 * 10.0 * 10.0), "{}", rows[0].volume);
    assert_eq!((rows[0].a_name.as_str(), rows[0].b_name.as_str()), ("Body1", "Body2"));
    // selecting bodies restricts the check and drops the old result
    click(&mut st, Pick::Body { body: a.clone() });
    st.hover = Some(Pick::Body { body: c.clone() });
    st.select(Pick::Body { body: c.clone() }, true);
    assert_eq!(inspect_dialog(&st).bodies, vec![a.clone(), c.clone()]);
    assert!(inspect_dialog(&st).interference.is_none());
    compute(&mut st);
    assert_eq!(inspect_dialog(&st).interference.as_ref().unwrap().len(), 0);
    assert!(inspect_dialog(&st).status.as_deref().unwrap().starts_with("No interference"));
    // a face click designates its body
    let top = face_with_normal(&st, &a, [0.0, 0.0, 1.0]);
    click(&mut st, top);
    assert_eq!(inspect_dialog(&st).bodies, vec![c.clone()], "toggling a body's face toggles the body");
    st.dialog_ok();
    assert!(st.dialog.is_none());
    assert_eq!(st.history.can_undo(), undo_before, "inspecting is not an undo step");
}

#[test]
fn center_of_mass_dialog_follows_the_selection() {
    let mut st = new_state();
    let a = add_box(&mut st, [0.0, 0.0], 40.0, 20.0, "10 mm");
    let b = add_box(&mut st, [100.0, 0.0], 10.0, 10.0, "10 mm");
    st.select(Pick::Body { body: a.clone() }, false);
    st.execute(CommandId::CenterOfMass);
    assert_eq!(st.dialog.as_ref().unwrap().title(), "CENTER OF MASS");
    let m = inspect_dialog(&st).mass.expect("pre-selected body");
    assert!(close(m.volume, 8000.0) && close(m.area, 2.0 * (800.0 + 400.0 + 200.0)));
    assert!(m.centroid.iter().zip([20.0, 10.0, 5.0]).all(|(p, q)| close(*p, q)), "{:?}", m.centroid);
    assert_eq!(st.inspect_markers(), vec![InspectMarker::Point(m.centroid)]);
    // add the second body: volume-weighted centre
    st.hover = Some(Pick::Body { body: b.clone() });
    st.select(Pick::Body { body: b.clone() }, true);
    let m = inspect_dialog(&st).mass.unwrap();
    assert!(close(m.volume, 9000.0) && close(m.centroid[0], (20.0 * 8000.0 + 105.0 * 1000.0) / 9000.0), "{m:?}");
    // cleared: all visible bodies (the same two)
    if let Some(Dialog::Inspect(x)) = st.dialog.as_mut() {
        x.clear_inputs();
    }
    let Some(Dialog::Inspect(mut x)) = st.dialog.take() else { unreachable!() };
    st.inspect_refresh_if_stale(&mut x);
    assert!(close(x.mass.unwrap().volume, 9000.0));
    st.dialog = Some(Dialog::Inspect(x));
    // the model changes while the dialog is open: the result follows (the panel checks each frame)
    add_box(&mut st, [0.0, 100.0], 10.0, 10.0, "10 mm");
    let Some(Dialog::Inspect(mut x)) = st.dialog.take() else { unreachable!() };
    st.inspect_refresh_if_stale(&mut x);
    assert!(close(x.mass.unwrap().volume, 10000.0), "{:?}", x.mass);
    st.dialog = Some(Dialog::Inspect(x));
    st.dialog_cancel();
    assert!(st.dialog.is_none());
}

#[test]
fn interference_result_is_dropped_when_the_model_changes() {
    let mut st = new_state();
    add_box(&mut st, [0.0, 0.0], 20.0, 20.0, "10 mm");
    add_box(&mut st, [10.0, 5.0], 20.0, 10.0, "10 mm");
    st.execute(CommandId::Interference);
    compute(&mut st);
    assert_eq!(inspect_dialog(&st).interference.as_ref().unwrap().len(), 1);
    let Some(Dialog::Inspect(mut x)) = st.dialog.take() else { unreachable!() };
    st.inspect_refresh_if_stale(&mut x);
    assert!(x.interference.is_some(), "unchanged model: the result stays");
    st.dialog = Some(Dialog::Inspect(x));
    add_box(&mut st, [15.0, 0.0], 10.0, 10.0, "10 mm");
    let Some(Dialog::Inspect(mut x)) = st.dialog.take() else { unreachable!() };
    st.inspect_refresh_if_stale(&mut x);
    assert!(x.interference.is_none() && x.status.as_deref().is_some_and(|s| s.contains("Compute again")), "{:?}", x.status);
    st.dialog = Some(Dialog::Inspect(x));
    compute(&mut st);
    assert_eq!(inspect_dialog(&st).interference.as_ref().unwrap().len(), 3, "the new box overlaps both");
}
