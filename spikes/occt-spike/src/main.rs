//! OCCT feasibility spike: proves the statically built Open CASCADE links and runs on this host,
//! and exercises the operations CAD360 needs, via the stock `opencascade` crate.
use opencascade::primitives::{Edge, EdgeType, Face, Shape, Wire};
use glam::dvec3;

fn main() {
    let out = std::env::temp_dir().join("cad360-occt-spike");
    std::fs::create_dir_all(&out).unwrap();

    // 50 x 30 profile with a 12 mm hole, on XY
    let rect = Wire::from_ordered_points([dvec3(0., 0., 0.), dvec3(50., 0., 0.), dvec3(50., 30., 0.), dvec3(0., 30., 0.)]).unwrap();
    let hole = Wire::from_edges([&Edge::circle(dvec3(25., 15., 0.), dvec3(0., 0., 1.), 6.0)]);
    let face = Face::from_wire_with_holes(&rect, &[hole]);
    let solid: Shape = face.extrude(dvec3(0., 0., 10.)).into();
    println!("extrude: faces={} edges={}", solid.faces().count(), solid.edges().count());

    // cut with a box, fillet the 4 outer vertical edges, chamfer one bottom edge
    let tool = Shape::box_from_corners(dvec3(40., -1., 5.), dvec3(51., 31., 11.));
    let cut: Shape = solid.subtract(&tool).into();
    println!("cut: faces={}", cut.faces().count());
    let verticals: Vec<_> = cut
        .edges()
        .filter(|e| e.edge_type() == EdgeType::Line && {
            let (a, b) = (e.start_point(), e.end_point());
            (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9 && (a.x.abs() < 1e-9)
        })
        .collect();
    println!("selected {} vertical edges at x=0", verticals.len());
    let filleted = cut.fillet_edges(3.0, &verticals);
    println!("fillet: faces={}", filleted.faces().count());
    let bottom: Vec<_> = filleted
        .edges()
        .filter(|e| e.edge_type() == EdgeType::Line && e.start_point().z.abs() < 1e-9 && e.end_point().z.abs() < 1e-9 && (e.start_point().y - e.end_point().y).abs() < 1e-9 && e.start_point().y.abs() < 1e-9)
        .collect();
    let chamfered = filleted.chamfer_edges(1.0, &bottom);
    println!("chamfer({} edges): faces={}", bottom.len(), chamfered.faces().count());

    // revolve a rectangle about the Z axis -> ring
    let ring_profile = Face::from_wire(&Wire::from_ordered_points([dvec3(10., 0., 0.), dvec3(15., 0., 0.), dvec3(15., 0., 8.), dvec3(10., 0., 8.)]).unwrap());
    let ring: Shape = ring_profile.revolve(dvec3(0., 0., 0.), dvec3(0., 0., 1.), None).into();
    println!("revolve: faces={}", ring.faces().count());

    // exchange
    let step = out.join("spike.step");
    chamfered.write_step(&step).unwrap();
    let back = Shape::read_step(&step).unwrap();
    println!("STEP round trip: faces {} -> {}", chamfered.faces().count(), back.faces().count());
    chamfered.write_stl(out.join("spike.stl")).unwrap();
    let mesh = chamfered.mesh().unwrap();
    println!("mesh: {} vertices, {} triangles", mesh.vertices.len(), mesh.indices.len() / 3);
    for f in chamfered.faces().take(3) {
        println!("face area={:.3} normal={:?} centroid={:?}", f.surface_area(), f.normal_at_center(), f.center_of_mass());
    }
    println!("OK {}", out.display());
}
