// CAD360 kernel shim: exception-safe Open CASCADE operations for the Rust kernel.
//
// OCCT reports failures by throwing Standard_Failure, which does not derive from std::exception
// and therefore cannot cross the cxx bridge (it would abort the process). Every function here
// catches all exceptions, records a message retrievable with cad_last_error(), and returns a
// null shape / false instead.
#pragma once

#include "rust/cxx.h"
#include <bindings_common.hxx>

#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepBndLib.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepFilletAPI_MakeChamfer.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <BRepGProp.hxx>
#include <BRepLProp_SLProps.hxx>
#include <BRepLib_ToolTriangulatedShape.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepPrimAPI_MakeRevol.hxx>
#include <BRepTools.hxx>
#include <BRep_Builder.hxx>
#include <BRep_Tool.hxx>
#include <Bnd_Box.hxx>
#include <GC_MakeArcOfCircle.hxx>
#include <GCPnts_AbscissaPoint.hxx>
#include <GCPnts_TangentialDeflection.hxx>
#include <GProp_GProps.hxx>
#include <Geom_TrimmedCurve.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <Interface_Static.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Poly_Triangulation.hxx>
#include <STEPControl_Reader.hxx>
#include <STEPControl_Writer.hxx>
#include <ShapeFix_Face.hxx>
#include <ShapeFix_Shape.hxx>
#include <Standard_Failure.hxx>
#include <StlAPI_Writer.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopTools_IndexedDataMapOfShapeListOfShape.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include <TopTools_ListOfShape.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Edge.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Shape.hxx>
#include <TopoDS_Wire.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Pnt.hxx>
#include <gp_Vec.hxx>

#include <memory>
#include <string>
#include <vector>

inline std::string &cad_err_ref() {
  static thread_local std::string err;
  return err;
}
inline rust::String cad_last_error() { return rust::String(cad_err_ref()); }

#define CAD_CATCH(fallback)                                                                                       \
  catch (Standard_Failure & f) {                                                                                 \
    const char *m = f.GetMessageString();                                                                        \
    cad_err_ref() = std::string(f.DynamicType()->Name()) + (m && *m ? std::string(": ") + m : std::string()); \
    return fallback;                                                                                             \
  }                                                                                                              \
  catch (std::exception & e) {                                                                                   \
    cad_err_ref() = e.what();                                                                                    \
    return fallback;                                                                                             \
  }                                                                                                              \
  catch (...) {                                                                                                  \
    cad_err_ref() = "unknown Open CASCADE failure";                                                              \
    return fallback;                                                                                             \
  }

inline std::unique_ptr<TopoDS_Shape> cad_fail(const char *msg) {
  cad_err_ref() = msg;
  return nullptr;
}
inline std::unique_ptr<TopoDS_Shape> cad_own(const TopoDS_Shape &s) {
  if (s.IsNull()) return cad_fail("operation produced an empty shape");
  return std::unique_ptr<TopoDS_Shape>(new TopoDS_Shape(s));
}

/// Planar face from loops. data = [nloops, (nsegs, (kind, sx,sy,sz, mx,my,mz, ex,ey,ez)*)*]
/// kind 0 = line, 1 = arc through (s, m, e). The first loop is the outer boundary.
inline std::unique_ptr<TopoDS_Shape> cad_make_face(rust::Slice<const double> data) {
  try {
    cad_err_ref().clear();
    size_t i = 0;
    auto next = [&]() -> double {
      if (i >= data.size()) throw Standard_Failure("malformed loop data");
      return data[i++];
    };
    int nloops = (int)next();
    std::vector<TopoDS_Wire> wires;
    for (int l = 0; l < nloops; ++l) {
      int nseg = (int)next();
      BRepBuilderAPI_MakeWire mw;
      for (int s = 0; s < nseg; ++s) {
        // read sequentially: argument evaluation order is unspecified in C++ (MSVC goes right-to-left)
        int kind = (int)next();
        double c[9];
        for (int k = 0; k < 9; ++k) c[k] = next();
        gp_Pnt a(c[0], c[1], c[2]);
        gp_Pnt m(c[3], c[4], c[5]);
        gp_Pnt b(c[6], c[7], c[8]);
        if (kind == 0) {
          BRepBuilderAPI_MakeEdge me(a, b);
          if (!me.IsDone()) return cad_fail("could not build a line edge (zero length?)");
          mw.Add(me.Edge());
        } else {
          GC_MakeArcOfCircle arc(a, m, b);
          if (!arc.IsDone()) return cad_fail("could not build an arc edge (collinear points?)");
          BRepBuilderAPI_MakeEdge me(arc.Value());
          if (!me.IsDone()) return cad_fail("could not build an arc edge");
          mw.Add(me.Edge());
        }
      }
      if (!mw.IsDone()) return cad_fail("profile loop is not connected");
      wires.push_back(mw.Wire());
    }
    if (wires.empty()) return cad_fail("profile has no loops");
    BRepBuilderAPI_MakeFace mf(wires[0], Standard_True);
    if (!mf.IsDone()) return cad_fail("profile is not planar or not closed");
    for (size_t w = 1; w < wires.size(); ++w) mf.Add(TopoDS::Wire(wires[w].Reversed()));
    ShapeFix_Face fix(mf.Face());
    fix.FixOrientation();
    fix.Perform();
    return cad_own(fix.Face());
  }
  CAD_CATCH(nullptr)
}

inline std::unique_ptr<TopoDS_Shape> cad_prism(const TopoDS_Shape &face, double dx, double dy, double dz) {
  try {
    cad_err_ref().clear();
    BRepPrimAPI_MakePrism mk(face, gp_Vec(dx, dy, dz));
    if (!mk.IsDone()) return cad_fail("extrusion failed");
    return cad_own(mk.Shape());
  }
  CAD_CATCH(nullptr)
}

inline std::unique_ptr<TopoDS_Shape> cad_revol(const TopoDS_Shape &face, double ox, double oy, double oz, double ax,
                                               double ay, double az, double angle) {
  try {
    cad_err_ref().clear();
    BRepPrimAPI_MakeRevol mk(face, gp_Ax1(gp_Pnt(ox, oy, oz), gp_Dir(ax, ay, az)), angle);
    if (!mk.IsDone()) return cad_fail("revolve failed");
    return cad_own(mk.Shape());
  }
  CAD_CATCH(nullptr)
}

/// op: 0 = fuse (join), 1 = cut, 2 = common (intersect)
inline std::unique_ptr<TopoDS_Shape> cad_boolean(const TopoDS_Shape &a, const TopoDS_Shape &b, int op) {
  try {
    cad_err_ref().clear();
    TopoDS_Shape r;
    if (op == 0) {
      BRepAlgoAPI_Fuse mk(a, b);
      if (!mk.IsDone() || mk.HasErrors()) return cad_fail("join (fuse) failed");
      r = mk.Shape();
    } else if (op == 1) {
      BRepAlgoAPI_Cut mk(a, b);
      if (!mk.IsDone() || mk.HasErrors()) return cad_fail("cut failed");
      r = mk.Shape();
    } else {
      BRepAlgoAPI_Common mk(a, b);
      if (!mk.IsDone() || mk.HasErrors()) return cad_fail("intersect failed");
      r = mk.Shape();
    }
    return cad_own(r);
  }
  CAD_CATCH(nullptr)
}

inline std::unique_ptr<TopoDS_Shape> cad_compound_new() {
  TopoDS_Compound c;
  BRep_Builder b;
  b.MakeCompound(c);
  return std::unique_ptr<TopoDS_Shape>(new TopoDS_Shape(c));
}
inline bool cad_compound_add(TopoDS_Shape &compound, const TopoDS_Shape &s) {
  try {
    BRep_Builder b;
    b.Add(compound, s);
    return true;
  }
  CAD_CATCH(false)
}

inline std::unique_ptr<TopoDS_Shape> cad_fillet(const TopoDS_Shape &shape, const TopoDS_Shape &edges, double r) {
  try {
    cad_err_ref().clear();
    BRepFilletAPI_MakeFillet mk(shape);
    int n = 0;
    for (TopExp_Explorer ex(edges, TopAbs_EDGE); ex.More(); ex.Next(), ++n) mk.Add(r, TopoDS::Edge(ex.Current()));
    if (n == 0) return cad_fail("no edges selected");
    mk.Build();
    if (!mk.IsDone()) return cad_fail("fillet could not be computed with this radius");
    return cad_own(mk.Shape());
  }
  CAD_CATCH(nullptr)
}

inline std::unique_ptr<TopoDS_Shape> cad_chamfer(const TopoDS_Shape &shape, const TopoDS_Shape &edges, double d) {
  try {
    cad_err_ref().clear();
    BRepFilletAPI_MakeChamfer mk(shape);
    int n = 0;
    for (TopExp_Explorer ex(edges, TopAbs_EDGE); ex.More(); ex.Next(), ++n) mk.Add(d, TopoDS::Edge(ex.Current()));
    if (n == 0) return cad_fail("no edges selected");
    mk.Build();
    if (!mk.IsDone()) return cad_fail("chamfer could not be computed with this distance");
    return cad_own(mk.Shape());
  }
  CAD_CATCH(nullptr)
}

inline std::unique_ptr<TopoDS_Shape> cad_cylinder(double ox, double oy, double oz, double dx, double dy, double dz,
                                                  double r, double h) {
  try {
    cad_err_ref().clear();
    BRepPrimAPI_MakeCylinder mk(gp_Ax2(gp_Pnt(ox, oy, oz), gp_Dir(dx, dy, dz)), r, h);
    return cad_own(mk.Shape());
  }
  CAD_CATCH(nullptr)
}

/// Unique sub-shapes of a kind (TopAbs_ShapeEnum: 2 solid, 4 face, 6 edge, 7 vertex), in a stable order.
inline std::unique_ptr<std::vector<TopoDS_Shape>> cad_subshapes(const TopoDS_Shape &shape, int kind) {
  auto out = std::unique_ptr<std::vector<TopoDS_Shape>>(new std::vector<TopoDS_Shape>());
  try {
    TopTools_IndexedMapOfShape map;
    TopExp::MapShapes(shape, (TopAbs_ShapeEnum)kind, map);
    for (int i = 1; i <= map.Extent(); ++i) out->push_back(map(i));
  } catch (...) {
    cad_err_ref() = "could not enumerate sub-shapes";
  }
  return out;
}

/// For each unique edge (cad_subshapes order), the indices of up to two adjacent faces (-1 if none).
inline bool cad_edge_faces(const TopoDS_Shape &shape, rust::Vec<int32_t> &out) {
  try {
    TopTools_IndexedMapOfShape faces, edges;
    TopExp::MapShapes(shape, TopAbs_FACE, faces);
    TopExp::MapShapes(shape, TopAbs_EDGE, edges);
    TopTools_IndexedDataMapOfShapeListOfShape anc;
    TopExp::MapShapesAndAncestors(shape, TopAbs_EDGE, TopAbs_FACE, anc);
    for (int i = 1; i <= edges.Extent(); ++i) {
      int f[2] = {-1, -1};
      int k = 0;
      if (anc.Contains(edges(i))) {
        for (TopTools_ListOfShape::Iterator it(anc.FindFromKey(edges(i))); it.More() && k < 2; it.Next()) {
          int idx = faces.FindIndex(it.Value());
          if (idx > 0 && (k == 0 || f[0] != idx - 1)) f[k++] = idx - 1;
        }
      }
      out.push_back(f[0]);
      out.push_back(f[1]);
    }
    return true;
  }
  CAD_CATCH(false)
}

inline bool cad_mesh(const TopoDS_Shape &shape, double deflection, double angular) {
  try {
    BRepTools::Clean(shape);
    BRepMesh_IncrementalMesh mk(shape, deflection, Standard_False, angular, Standard_True);
    return mk.IsDone();
  }
  CAD_CATCH(false)
}

/// Appends the triangulation of one face (positions, per-vertex normals, triangle indices, with the
/// index offset applied) to the output vectors. The shape must have been meshed with cad_mesh.
inline bool cad_face_triangles(const TopoDS_Shape &shape, rust::Vec<float> &pos, rust::Vec<float> &nrm,
                               rust::Vec<uint32_t> &idx) {
  try {
    const TopoDS_Face &face = TopoDS::Face(shape);
    TopLoc_Location loc;
    Handle(Poly_Triangulation) tri = BRep_Tool::Triangulation(face, loc);
    if (tri.IsNull()) return cad_fail("face has no triangulation"), false;
    if (!tri->HasNormals()) BRepLib_ToolTriangulatedShape::ComputeNormals(face, tri);
    const gp_Trsf &trsf = loc.Transformation();
    const bool reversed = face.Orientation() == TopAbs_REVERSED;
    const uint32_t base = (uint32_t)(pos.size() / 3);
    for (int i = 1; i <= tri->NbNodes(); ++i) {
      gp_Pnt p = tri->Node(i).Transformed(trsf);
      pos.push_back((float)p.X());
      pos.push_back((float)p.Y());
      pos.push_back((float)p.Z());
      gp_Dir n = tri->Normal(i);
      n.Transform(trsf);
      if (reversed) n.Reverse();
      nrm.push_back((float)n.X());
      nrm.push_back((float)n.Y());
      nrm.push_back((float)n.Z());
    }
    for (int t = 1; t <= tri->NbTriangles(); ++t) {
      int a, b, c;
      tri->Triangle(t).Get(a, b, c);
      if (reversed) std::swap(b, c);
      idx.push_back(base + a - 1);
      idx.push_back(base + b - 1);
      idx.push_back(base + c - 1);
    }
    return true;
  }
  CAD_CATCH(false)
}

/// Polyline approximation of an edge (x,y,z triples).
inline bool cad_edge_polyline(const TopoDS_Shape &shape, double deflection, rust::Vec<float> &pts) {
  try {
    const TopoDS_Edge &edge = TopoDS::Edge(shape);
    if (BRep_Tool::Degenerated(edge)) return true;
    BRepAdaptor_Curve c(edge);
    GCPnts_TangentialDeflection d(c, 0.1, deflection);
    for (int i = 1; i <= d.NbPoints(); ++i) {
      gp_Pnt p = d.Value(i);
      pts.push_back((float)p.X());
      pts.push_back((float)p.Y());
      pts.push_back((float)p.Z());
    }
    return true;
  }
  CAD_CATCH(false)
}

/// out[0] surface type (GeomAbs_SurfaceType: 0 plane, 1 cylinder, ...), out[1] area, out[2..5] centroid,
/// out[5..8] outward normal at the centroid parameter, out[8..11] axis (cylinder/cone), out[11..14]
/// axis location, out[14] radius.
inline bool cad_face_info(const TopoDS_Shape &shape, rust::Slice<double> out) {
  try {
    if (out.size() < 15) return false;
    for (size_t i = 0; i < out.size(); ++i) out[i] = 0;
    const TopoDS_Face &face = TopoDS::Face(shape);
    BRepAdaptor_Surface s(face, Standard_True);
    out[0] = (double)s.GetType();
    GProp_GProps props;
    BRepGProp::SurfaceProperties(face, props);
    out[1] = props.Mass();
    gp_Pnt c = props.CentreOfMass();
    out[2] = c.X();
    out[3] = c.Y();
    out[4] = c.Z();
    double u = 0.5 * (s.FirstUParameter() + s.LastUParameter());
    double v = 0.5 * (s.FirstVParameter() + s.LastVParameter());
    BRepLProp_SLProps lp(s, u, v, 1, 1e-7);
    if (lp.IsNormalDefined()) {
      gp_Dir n = lp.Normal();
      if (face.Orientation() == TopAbs_REVERSED) n.Reverse();
      out[5] = n.X();
      out[6] = n.Y();
      out[7] = n.Z();
    }
    if (s.GetType() == GeomAbs_Cylinder) {
      gp_Cylinder cy = s.Cylinder();
      out[8] = cy.Axis().Direction().X();
      out[9] = cy.Axis().Direction().Y();
      out[10] = cy.Axis().Direction().Z();
      out[11] = cy.Location().X();
      out[12] = cy.Location().Y();
      out[13] = cy.Location().Z();
      out[14] = cy.Radius();
    }
    return true;
  }
  CAD_CATCH(false)
}

/// out[0] curve type (GeomAbs_CurveType: 0 line, 1 circle, ...), out[1] length, out[2..5] start,
/// out[5..8] mid, out[8..11] end, out[11..14] unit tangent at mid, out[14..17] circle centre, out[17] radius,
/// out[18] closed (1/0).
inline bool cad_edge_info(const TopoDS_Shape &shape, rust::Slice<double> out) {
  try {
    if (out.size() < 19) return false;
    for (size_t i = 0; i < out.size(); ++i) out[i] = 0;
    const TopoDS_Edge &edge = TopoDS::Edge(shape);
    if (BRep_Tool::Degenerated(edge)) {
      out[0] = -1;
      return true;
    }
    BRepAdaptor_Curve c(edge);
    out[0] = (double)c.GetType();
    out[1] = GCPnts_AbscissaPoint::Length(c);
    double f = c.FirstParameter(), l = c.LastParameter(), m = 0.5 * (f + l);
    gp_Pnt p;
    gp_Vec t;
    p = c.Value(f);
    out[2] = p.X(), out[3] = p.Y(), out[4] = p.Z();
    c.D1(m, p, t);
    out[5] = p.X(), out[6] = p.Y(), out[7] = p.Z();
    if (t.Magnitude() > 1e-12) t.Normalize();
    out[11] = t.X(), out[12] = t.Y(), out[13] = t.Z();
    p = c.Value(l);
    out[8] = p.X(), out[9] = p.Y(), out[10] = p.Z();
    if (c.GetType() == GeomAbs_Circle) {
      gp_Circ ci = c.Circle();
      out[14] = ci.Location().X(), out[15] = ci.Location().Y(), out[16] = ci.Location().Z();
      out[17] = ci.Radius();
    }
    out[18] = BRep_Tool::IsClosed(edge) || c.IsClosed() ? 1 : 0;
    return true;
  }
  CAD_CATCH(false)
}

inline double cad_volume(const TopoDS_Shape &shape) {
  try {
    GProp_GProps props;
    BRepGProp::VolumeProperties(shape, props);
    return props.Mass();
  }
  CAD_CATCH(0.0)
}

inline double cad_area(const TopoDS_Shape &shape) {
  try {
    GProp_GProps props;
    BRepGProp::SurfaceProperties(shape, props);
    return props.Mass();
  }
  CAD_CATCH(0.0)
}

inline bool cad_bbox(const TopoDS_Shape &shape, rust::Slice<double> out) {
  try {
    if (out.size() < 6) return false;
    Bnd_Box box;
    BRepBndLib::Add(shape, box, Standard_False);
    if (box.IsVoid()) return false;
    box.Get(out[0], out[1], out[2], out[3], out[4], out[5]);
    return true;
  }
  CAD_CATCH(false)
}

inline bool cad_is_valid(const TopoDS_Shape &shape) {
  try {
    BRepCheck_Analyzer a(shape);
    return a.IsValid();
  }
  CAD_CATCH(false)
}

inline void cad_quiet() {
  try {
    Message::DefaultMessenger()->RemovePrinters(STANDARD_TYPE(Message_Printer));
  } catch (...) {
  }
}

inline bool cad_write_step(const TopoDS_Shape &shape, rust::Str path) {
  try {
    cad_err_ref().clear();
    STEPControl_Writer w;
    Interface_Static::SetCVal("write.step.unit", "MM");
    if (w.Transfer(shape, STEPControl_AsIs) != IFSelect_RetDone) return cad_fail("STEP transfer failed"), false;
    if (w.Write(std::string(path).c_str()) != IFSelect_RetDone) return cad_fail("could not write the STEP file"), false;
    return true;
  }
  CAD_CATCH(false)
}

inline std::unique_ptr<TopoDS_Shape> cad_read_step(rust::Str path) {
  try {
    cad_err_ref().clear();
    STEPControl_Reader r;
    if (r.ReadFile(std::string(path).c_str()) != IFSelect_RetDone) return cad_fail("could not read the STEP file");
    r.TransferRoots();
    TopoDS_Shape s = r.OneShape();
    if (s.IsNull()) return cad_fail("the STEP file contains no shape");
    return cad_own(s);
  }
  CAD_CATCH(nullptr)
}

/// The shape must have been meshed (cad_mesh) first.
inline bool cad_write_stl(const TopoDS_Shape &shape, rust::Str path, bool ascii) {
  try {
    cad_err_ref().clear();
    StlAPI_Writer w;
    w.ASCIIMode() = ascii;
    if (!w.Write(shape, std::string(path).c_str())) return cad_fail("could not write the STL file"), false;
    return true;
  }
  CAD_CATCH(false)
}

inline std::unique_ptr<TopoDS_Shape> cad_copy(const TopoDS_Shape &shape) {
  return std::unique_ptr<TopoDS_Shape>(new TopoDS_Shape(shape));
}

inline int cad_shape_type(const TopoDS_Shape &shape) { return shape.IsNull() ? -1 : (int)shape.ShapeType(); }

// ---------------------------------------------------------------------------------------------
// CAD360 modelling features: shell and rigid / mirror transforms.

#include <BRepBuilderAPI_Transform.hxx>
#include <BRepOffsetAPI_MakeThickSolid.hxx>
#include <gp_Trsf.hxx>

/// Hollows `shape` (a solid) to an inside wall `thickness`, removing the faces in `faces` (a
/// compound of faces of `shape`). Fails when OCCT cannot build the offset (e.g. too thick).
inline std::unique_ptr<TopoDS_Shape> cad_shell(const TopoDS_Shape &shape, const TopoDS_Shape &faces, double thickness) {
  try {
    cad_err_ref().clear();
    if (!(thickness > 0)) return cad_fail("thickness must be greater than zero");
    TopTools_ListOfShape remove;
    for (TopExp_Explorer ex(faces, TopAbs_FACE); ex.More(); ex.Next()) remove.Append(ex.Current());
    if (remove.IsEmpty()) return cad_fail("no faces selected");
    BRepOffsetAPI_MakeThickSolid mk;
    mk.MakeThickSolidByJoin(shape, remove, -thickness, 1.0e-3, BRepOffset_Skin, Standard_False, Standard_False,
                            GeomAbs_Arc);
    if (!mk.IsDone()) return cad_fail("the shell could not be computed with this thickness");
    return cad_own(mk.Shape());
  }
  CAD_CATCH(nullptr)
}

/// Transformed copy of a shape.
/// kind 0: rigid affine transform, m = 12 row-major coefficients [a11 a12 a13 a14  a21 .. a24  a31 .. a34]
///         (rotation part must be orthonormal with determinant +1).
/// kind 1: mirror across the plane through (m[0], m[1], m[2]) with normal (m[3], m[4], m[5]).
inline std::unique_ptr<TopoDS_Shape> cad_transform(const TopoDS_Shape &shape, int kind, rust::Slice<const double> m) {
  try {
    cad_err_ref().clear();
    gp_Trsf t;
    if (kind == 0) {
      if (m.size() < 12) return cad_fail("transform needs 12 coefficients");
      t.SetValues(m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7], m[8], m[9], m[10], m[11]);
    } else if (kind == 1) {
      if (m.size() < 6) return cad_fail("mirror needs a point and a normal");
      t.SetMirror(gp_Ax2(gp_Pnt(m[0], m[1], m[2]), gp_Dir(m[3], m[4], m[5])));
    } else {
      return cad_fail("unknown transform kind");
    }
    BRepBuilderAPI_Transform mk(shape, t, Standard_True);
    if (!mk.IsDone()) return cad_fail("the transform failed");
    return cad_own(mk.Shape());
  }
  CAD_CATCH(nullptr)
}

// ===== naming =====
// History-based (generative) face naming. Each cad_*_named function performs the same operation
// as its plain counterpart and additionally fills `hist` with the origin of every face of the
// result (faces in TopExp::MapShapes order, i.e. cad_subshapes(result, FACE) order):
//   hist = [nfaces, for each face: count, (input, kind, index) * count]
// input: argument number (0 = the shape / first argument, 1 = the tool of a boolean)
// index: 0-based MapShapes index of the origin sub-shape within that input (faces, edges or
//        vertices of the input, depending on kind; for kinds 4/5 the face within the cap)
// kind:  0 = same or modified face, 1 = face generated from a face (offset walls),
//        2 = face generated from an edge (sweep sides, fillet/chamfer blends),
//        3 = face generated from a vertex (corner blends), 4 = sweep start cap, 5 = sweep end cap
// History queries are individually guarded: an algorithm that cannot answer for a sub-shape just
// contributes no origin for it.

#include <BRepBuilderAPI_MakeShape.hxx>
#include <BRepBuilderAPI_MakeVertex.hxx>
#include <BRepExtrema_DistShapeShape.hxx>

struct CadHistory {
  TopTools_IndexedMapOfShape faces;
  std::vector<std::vector<int>> origins;  // per result face: flat (input, kind, index) triples
  explicit CadHistory(const TopoDS_Shape &result) {
    TopExp::MapShapes(result, TopAbs_FACE, faces);
    origins.resize(faces.Extent());
  }
  void add(const TopoDS_Shape &s, int input, int kind, int index) {
    if (s.IsNull()) return;
    for (TopExp_Explorer ex(s, TopAbs_FACE); ex.More(); ex.Next()) {
      int j = faces.FindIndex(ex.Current());
      if (j <= 0) continue;
      std::vector<int> &o = origins[j - 1];
      bool dup = false;
      for (size_t t = 0; t + 2 < o.size(); t += 3)
        if (o[t] == input && o[t + 1] == kind && o[t + 2] == index) dup = true;
      if (!dup) {
        o.push_back(input);
        o.push_back(kind);
        o.push_back(index);
      }
    }
  }
  void add_list(const TopTools_ListOfShape &l, int input, int kind, int index) {
    for (TopTools_ListOfShape::Iterator it(l); it.More(); it.Next()) add(it.Value(), input, kind, index);
  }
  /// Traces the faces, edges and vertices of `in` through `mk`. `face_generated`: also ask
  /// Generated(face) (off for sweeps, where it returns the whole swept solid).
  void trace(BRepBuilderAPI_MakeShape &mk, const TopoDS_Shape &in, int input, bool face_generated) {
    TopTools_IndexedMapOfShape fm, em, vm;
    TopExp::MapShapes(in, TopAbs_FACE, fm);
    TopExp::MapShapes(in, TopAbs_EDGE, em);
    TopExp::MapShapes(in, TopAbs_VERTEX, vm);
    for (int i = 1; i <= fm.Extent(); ++i) {
      const TopoDS_Shape &f = fm(i);
      if (faces.Contains(f)) add(f, input, 0, i - 1);
      try {
        add_list(mk.Modified(f), input, 0, i - 1);
      } catch (...) {
      }
      if (face_generated) {
        try {
          add_list(mk.Generated(f), input, 1, i - 1);
        } catch (...) {
        }
      }
    }
    for (int i = 1; i <= em.Extent(); ++i) {
      try {
        add_list(mk.Generated(em(i)), input, 2, i - 1);
      } catch (...) {
      }
    }
    for (int i = 1; i <= vm.Extent(); ++i) {
      try {
        add_list(mk.Generated(vm(i)), input, 3, i - 1);
      } catch (...) {
      }
    }
  }
  /// Sweep faces the algorithm reports no history for (observed: the planar faces of a full 360 deg
  /// revolution): the face generated by a profile edge contains that edge's midpoint (the sweep
  /// starts at the profile), so an untraced face is attributed to the profile edge, not yet
  /// claimed by another face, whose midpoint lies on it.
  void seam_fallback(const TopoDS_Shape &in) {
    TopTools_IndexedMapOfShape em;
    TopExp::MapShapes(in, TopAbs_EDGE, em);
    std::vector<bool> used(em.Extent(), false);
    bool missing = false;
    for (const auto &o : origins) {
      if (o.empty()) missing = true;
      for (size_t t = 0; t + 2 < o.size(); t += 3) {
        if (o[t] == 0 && o[t + 1] == 2 && o[t + 2] >= 0 && o[t + 2] < (int)used.size()) used[o[t + 2]] = true;
      }
    }
    if (!missing) return;
    for (int j = 1; j <= faces.Extent(); ++j) {
      if (!origins[j - 1].empty()) continue;
      for (int i = 1; i <= em.Extent(); ++i) {
        if (used[i - 1]) continue;
        const TopoDS_Edge &e = TopoDS::Edge(em(i));
        if (BRep_Tool::Degenerated(e)) continue;
        try {
          BRepAdaptor_Curve c(e);
          gp_Pnt p = c.Value(0.5 * (c.FirstParameter() + c.LastParameter()));
          BRepExtrema_DistShapeShape d(BRepBuilderAPI_MakeVertex(p).Vertex(), faces(j));
          if (d.IsDone() && d.Value() < 1e-6) {
            add(faces(j), 0, 2, i - 1);
            used[i - 1] = true;
            break;
          }
        } catch (...) {
        }
      }
    }
  }
  /// Faces of a sweep cap (FirstShape / LastShape), numbered within the cap.
  void cap(const TopoDS_Shape &c, int kind) {
    if (c.IsNull()) return;
    TopTools_IndexedMapOfShape cm;
    TopExp::MapShapes(c, TopAbs_FACE, cm);
    for (int i = 1; i <= cm.Extent(); ++i) add(cm(i), 0, kind, i - 1);
  }
  void write(rust::Vec<int32_t> &out) const {
    out.push_back((int32_t)origins.size());
    for (const auto &o : origins) {
      out.push_back((int32_t)(o.size() / 3));
      for (int v : o) out.push_back(v);
    }
  }
};

inline std::unique_ptr<TopoDS_Shape> cad_prism_named(const TopoDS_Shape &face, double dx, double dy, double dz,
                                                     rust::Vec<int32_t> &hist) {
  try {
    cad_err_ref().clear();
    BRepPrimAPI_MakePrism mk(face, gp_Vec(dx, dy, dz));
    if (!mk.IsDone()) return cad_fail("extrusion failed");
    CadHistory h(mk.Shape());
    h.trace(mk, face, 0, false);
    h.cap(mk.FirstShape(), 4);
    h.cap(mk.LastShape(), 5);
    h.seam_fallback(face);
    h.write(hist);
    return cad_own(mk.Shape());
  }
  CAD_CATCH(nullptr)
}

inline std::unique_ptr<TopoDS_Shape> cad_revol_named(const TopoDS_Shape &face, double ox, double oy, double oz,
                                                     double ax, double ay, double az, double angle,
                                                     rust::Vec<int32_t> &hist) {
  try {
    cad_err_ref().clear();
    BRepPrimAPI_MakeRevol mk(face, gp_Ax1(gp_Pnt(ox, oy, oz), gp_Dir(ax, ay, az)), angle);
    if (!mk.IsDone()) return cad_fail("revolve failed");
    CadHistory h(mk.Shape());
    h.trace(mk, face, 0, false);
    h.cap(mk.FirstShape(), 4);
    h.cap(mk.LastShape(), 5);
    h.seam_fallback(face);
    h.write(hist);
    return cad_own(mk.Shape());
  }
  CAD_CATCH(nullptr)
}

/// op: 0 = fuse (join), 1 = cut, 2 = common (intersect); inputs 0 = a, 1 = b.
inline std::unique_ptr<TopoDS_Shape> cad_boolean_named(const TopoDS_Shape &a, const TopoDS_Shape &b, int op,
                                                       rust::Vec<int32_t> &hist) {
  try {
    cad_err_ref().clear();
    std::unique_ptr<BRepAlgoAPI_BooleanOperation> mk;
    if (op == 0)
      mk.reset(new BRepAlgoAPI_Fuse(a, b));
    else if (op == 1)
      mk.reset(new BRepAlgoAPI_Cut(a, b));
    else
      mk.reset(new BRepAlgoAPI_Common(a, b));
    if (!mk->IsDone() || mk->HasErrors())
      return cad_fail(op == 0 ? "join (fuse) failed" : op == 1 ? "cut failed" : "intersect failed");
    CadHistory h(mk->Shape());
    h.trace(*mk, a, 0, true);
    h.trace(*mk, b, 1, true);
    h.write(hist);
    return cad_own(mk->Shape());
  }
  CAD_CATCH(nullptr)
}

/// fillet: true = constant-radius fillet, false = equal-distance chamfer.
inline std::unique_ptr<TopoDS_Shape> cad_blend_named(const TopoDS_Shape &shape, const TopoDS_Shape &edges, double v,
                                                     bool fillet, rust::Vec<int32_t> &hist) {
  try {
    cad_err_ref().clear();
    std::unique_ptr<BRepFilletAPI_LocalOperation> op;
    int n = 0;
    if (fillet) {
      auto *mk = new BRepFilletAPI_MakeFillet(shape);
      op.reset(mk);
      for (TopExp_Explorer ex(edges, TopAbs_EDGE); ex.More(); ex.Next(), ++n) mk->Add(v, TopoDS::Edge(ex.Current()));
    } else {
      auto *mk = new BRepFilletAPI_MakeChamfer(shape);
      op.reset(mk);
      for (TopExp_Explorer ex(edges, TopAbs_EDGE); ex.More(); ex.Next(), ++n) mk->Add(v, TopoDS::Edge(ex.Current()));
    }
    if (n == 0) return cad_fail("no edges selected");
    op->Build();
    if (!op->IsDone())
      return cad_fail(fillet ? "fillet could not be computed with this radius"
                             : "chamfer could not be computed with this distance");
    CadHistory h(op->Shape());
    h.trace(*op, shape, 0, true);
    h.write(hist);
    return cad_own(op->Shape());
  }
  CAD_CATCH(nullptr)
}

inline std::unique_ptr<TopoDS_Shape> cad_shell_named(const TopoDS_Shape &shape, const TopoDS_Shape &faces,
                                                     double thickness, rust::Vec<int32_t> &hist) {
  try {
    cad_err_ref().clear();
    if (!(thickness > 0)) return cad_fail("thickness must be greater than zero");
    TopTools_ListOfShape remove;
    for (TopExp_Explorer ex(faces, TopAbs_FACE); ex.More(); ex.Next()) remove.Append(ex.Current());
    if (remove.IsEmpty()) return cad_fail("no faces selected");
    BRepOffsetAPI_MakeThickSolid mk;
    mk.MakeThickSolidByJoin(shape, remove, -thickness, 1.0e-3, BRepOffset_Skin, Standard_False, Standard_False,
                            GeomAbs_Arc);
    if (!mk.IsDone()) return cad_fail("the shell could not be computed with this thickness");
    CadHistory h(mk.Shape());
    h.trace(mk, shape, 0, true);
    h.write(hist);
    return cad_own(mk.Shape());
  }
  CAD_CATCH(nullptr)
}

/// As cad_transform (kind 0 rigid 3x4, kind 1 mirror point + normal), with history.
inline std::unique_ptr<TopoDS_Shape> cad_transform_named(const TopoDS_Shape &shape, int kind,
                                                         rust::Slice<const double> m, rust::Vec<int32_t> &hist) {
  try {
    cad_err_ref().clear();
    gp_Trsf t;
    if (kind == 0) {
      if (m.size() < 12) return cad_fail("transform needs 12 coefficients");
      t.SetValues(m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7], m[8], m[9], m[10], m[11]);
    } else if (kind == 1) {
      if (m.size() < 6) return cad_fail("mirror needs a point and a normal");
      t.SetMirror(gp_Ax2(gp_Pnt(m[0], m[1], m[2]), gp_Dir(m[3], m[4], m[5])));
    } else {
      return cad_fail("unknown transform kind");
    }
    BRepBuilderAPI_Transform mk(shape, t, Standard_True);
    if (!mk.IsDone()) return cad_fail("the transform failed");
    CadHistory h(mk.Shape());
    h.trace(mk, shape, 0, false);
    h.write(hist);
    return cad_own(mk.Shape());
  }
  CAD_CATCH(nullptr)
}

/// For each sub-shape of `child` of the given kind (MapShapes order), its 0-based MapShapes index in
/// `parent`, or -1 (e.g. the faces of one solid of a boolean result within the whole result).
inline bool cad_subshape_indices(const TopoDS_Shape &parent, const TopoDS_Shape &child, int kind,
                                 rust::Vec<int32_t> &out) {
  try {
    TopTools_IndexedMapOfShape pm, cm;
    TopExp::MapShapes(parent, (TopAbs_ShapeEnum)kind, pm);
    TopExp::MapShapes(child, (TopAbs_ShapeEnum)kind, cm);
    for (int i = 1; i <= cm.Extent(); ++i) out.push_back((int32_t)pm.FindIndex(cm(i)) - 1);
    return true;
  }
  CAD_CATCH(false)
}

// ===== surface =====
// CAD360 surface and mesh features: wires from sketch curves, sewing (stitch), offset surfaces,
// thicken, patch (planar face or N-sided filling), triangulation-only mesh faces and mesh to B-rep.

#include <BRepBuilderAPI_MakePolygon.hxx>
#include <BRepBuilderAPI_MakeSolid.hxx>
#include <BRepBuilderAPI_Sewing.hxx>
#include <BRepLib.hxx>
#include <BRepOffsetAPI_MakeFilling.hxx>
#include <BRepOffsetAPI_MakeOffsetShape.hxx>
#include <BRepOffset_MakeOffset.hxx>
#include <ShapeAnalysis_FreeBounds.hxx>
#include <ShapeFix_Shell.hxx>
#include <ShapeFix_Solid.hxx>
#include <ShapeUpgrade_UnifySameDomain.hxx>
#include <TopTools_HSequenceOfShape.hxx>
#include <TopoDS_Iterator.hxx>
#include <TopoDS_Shell.hxx>
#include <TopoDS_Solid.hxx>
#include <gp_Circ.hxx>

/// Wires from curve chains. data = [nchains, (nsegs, (kind, sx,sy,sz, mx,my,mz, ex,ey,ez)*)*];
/// kind 0 = line s->e, 1 = arc through (s, m, e), 2 = full circle (centre s, point on it m,
/// normal e). Each chain (edges in path order) becomes one wire; returns a compound of wires.
inline std::unique_ptr<TopoDS_Shape> cad_make_wires(rust::Slice<const double> data) {
  try {
    cad_err_ref().clear();
    size_t i = 0;
    auto next = [&]() -> double {
      if (i >= data.size()) throw Standard_Failure("malformed curve data");
      return data[i++];
    };
    TopoDS_Compound out;
    BRep_Builder bb;
    bb.MakeCompound(out);
    int nchains = (int)next();
    int nwires = 0;
    for (int l = 0; l < nchains; ++l) {
      int nseg = (int)next();
      BRepBuilderAPI_MakeWire mw;
      for (int s = 0; s < nseg; ++s) {
        int kind = (int)next();
        double c[9];
        for (int k = 0; k < 9; ++k) c[k] = next();
        gp_Pnt a(c[0], c[1], c[2]);
        gp_Pnt m(c[3], c[4], c[5]);
        gp_Pnt b(c[6], c[7], c[8]);
        TopoDS_Edge e;
        if (kind == 0) {
          BRepBuilderAPI_MakeEdge me(a, b);
          if (!me.IsDone()) return cad_fail("could not build a line edge (zero length?)");
          e = me.Edge();
        } else if (kind == 1) {
          GC_MakeArcOfCircle arc(a, m, b);
          if (!arc.IsDone()) return cad_fail("could not build an arc edge (collinear points?)");
          BRepBuilderAPI_MakeEdge me(arc.Value());
          if (!me.IsDone()) return cad_fail("could not build an arc edge");
          e = me.Edge();
        } else {
          double r = a.Distance(m);
          if (r < 1e-9) return cad_fail("circle radius is zero");
          gp_Circ circ(gp_Ax2(a, gp_Dir(c[6], c[7], c[8]), gp_Vec(a, m)), r);
          BRepBuilderAPI_MakeEdge me(circ);
          if (!me.IsDone()) return cad_fail("could not build a circle edge");
          e = me.Edge();
        }
        mw.Add(e);
        if (!mw.IsDone()) return cad_fail("the selected curves are not connected in order");
      }
      if (nseg == 0) continue;
      bb.Add(out, mw.Wire());
      ++nwires;
    }
    if (nwires == 0) return cad_fail("no curves selected");
    return cad_own(out);
  }
  CAD_CATCH(nullptr)
}

/// Direct children of a shape (TopoDS_Iterator order), e.g. the pieces of a compound.
inline std::unique_ptr<std::vector<TopoDS_Shape>> cad_children(const TopoDS_Shape &shape) {
  auto out = std::unique_ptr<std::vector<TopoDS_Shape>>(new std::vector<TopoDS_Shape>());
  try {
    for (TopoDS_Iterator it(shape); it.More(); it.Next()) out->push_back(it.Value());
  } catch (...) {
    cad_err_ref() = "could not iterate the shape";
  }
  return out;
}

/// Closed shell -> oriented, fixed solid (null on failure).
/// Canonical solid from the faces of a closed shell or solid: one FORWARD shell holding the faces
/// with their composed orientation, flipped when needed so the volume is positive (modelling
/// algorithms such as fillet misbehave on solids whose shell orientation is reversed).
inline TopoDS_Shape cad_canonical_solid(const TopoDS_Shape &s) {
  BRep_Builder b;
  for (int pass = 0; pass < 2; ++pass) {
    TopoDS_Shell sh;
    b.MakeShell(sh);
    for (TopExp_Explorer ex(s, TopAbs_FACE); ex.More(); ex.Next()) b.Add(sh, pass == 0 ? ex.Current() : ex.Current().Reversed());
    sh.Closed(Standard_True);
    TopoDS_Solid so;
    b.MakeSolid(so);
    b.Add(so, sh);
    GProp_GProps p;
    BRepGProp::VolumeProperties(so, p);
    if (p.Mass() > 0 || pass == 1) return so;
  }
  return TopoDS_Shape();
}

inline TopoDS_Shape cad_shell_to_solid(const TopoDS_Shell &shell) {
  BRepBuilderAPI_MakeSolid ms(shell);
  if (!ms.IsDone()) return TopoDS_Shape();
  TopoDS_Solid so = ms.Solid();
  BRepLib::OrientClosedSolid(so);
  ShapeFix_Solid fix(so);
  fix.Perform();
  TopoDS_Shape r = fix.Solid();
  if (r.IsNull()) r = so;
  TopoDS_Shape c = cad_canonical_solid(r);
  return c.IsNull() ? r : c;
}

/// Sews the faces of `shape` with `tolerance`. Returns a compound whose children are the
/// pieces: with `make_solid`, every closed shell becomes a solid; other shells and loose faces
/// stay surfaces.
inline std::unique_ptr<TopoDS_Shape> cad_sew(const TopoDS_Shape &shape, double tolerance, bool make_solid) {
  try {
    cad_err_ref().clear();
    if (!(tolerance > 0)) return cad_fail("tolerance must be greater than zero");
    BRepBuilderAPI_Sewing sw(tolerance);
    int n = 0;
    for (TopExp_Explorer ex(shape, TopAbs_FACE); ex.More(); ex.Next(), ++n) sw.Add(ex.Current());
    if (n == 0) return cad_fail("there are no faces to stitch");
    sw.Perform();
    TopoDS_Shape r = sw.SewedShape();
    if (r.IsNull()) return cad_fail("sewing produced no shape");
    TopoDS_Compound out;
    BRep_Builder bb;
    bb.MakeCompound(out);
    for (TopExp_Explorer ex(r, TopAbs_SHELL); ex.More(); ex.Next()) {
      TopoDS_Shell sh = TopoDS::Shell(ex.Current());
      if (make_solid && BRep_Tool::IsClosed(sh)) {
        TopoDS_Shape so = cad_shell_to_solid(sh);
        bb.Add(out, so.IsNull() ? TopoDS_Shape(sh) : so);
      } else {
        bb.Add(out, sh);
      }
    }
    for (TopExp_Explorer ex(r, TopAbs_FACE, TopAbs_SHELL); ex.More(); ex.Next()) bb.Add(out, ex.Current());
    return cad_own(out);
  }
  CAD_CATCH(nullptr)
}

/// Offset of faces / shells by `distance` along their normals (join algorithm, falling back to
/// the simple algorithm).
inline std::unique_ptr<TopoDS_Shape> cad_offset_surface(const TopoDS_Shape &shape, double distance) {
  try {
    cad_err_ref().clear();
    try {
      BRepOffsetAPI_MakeOffsetShape mk;
      mk.PerformByJoin(shape, distance, 1.0e-4, BRepOffset_Skin, Standard_False, Standard_False, GeomAbs_Arc);
      if (mk.IsDone() && !mk.Shape().IsNull()) return cad_own(mk.Shape());
    } catch (Standard_Failure &) {
    }
    BRepOffsetAPI_MakeOffsetShape mk2;
    mk2.PerformBySimple(shape, distance);
    if (!mk2.IsDone()) return cad_fail("the offset surface could not be computed with this distance");
    return cad_own(mk2.Shape());
  }
  CAD_CATCH(nullptr)
}

/// Thickens a face or open shell into a solid (`thickness` along the normal; negative = opposite).
inline std::unique_ptr<TopoDS_Shape> cad_thicken(const TopoDS_Shape &shape, double thickness) {
  try {
    cad_err_ref().clear();
    if (thickness == 0) return cad_fail("thickness must not be zero");
    TopoDS_Shape r;
    // join algorithm with thickening (sharp corners between faces are intersected), falling
    // back to the simple algorithm (no intersections; fine for smooth surfaces)
    try {
      BRepOffset_MakeOffset mo;
      mo.Initialize(shape, thickness, 1.0e-4, BRepOffset_Skin, Standard_False, Standard_False, GeomAbs_Intersection,
                    Standard_True);
      mo.MakeOffsetShape();
      if (mo.IsDone() && !mo.Shape().IsNull()) {
        TopoDS_Shape t = mo.Shape();
        TopExp_Explorer ex(t, TopAbs_SOLID);
        if (ex.More()) r = ex.Current();
        else if (t.ShapeType() == TopAbs_SHELL && BRep_Tool::IsClosed(t)) r = t;
        if (!r.IsNull() && !BRepCheck_Analyzer(r).IsValid()) r.Nullify();
      }
    } catch (Standard_Failure &) {
      r.Nullify();
    }
    if (r.IsNull()) {
      BRepOffsetAPI_MakeThickSolid mk;
      mk.MakeThickSolidBySimple(shape, thickness);
      if (!mk.IsDone()) return cad_fail("the surface could not be thickened");
      r = mk.Shape();
    }
    if (r.IsNull()) return cad_fail("thicken produced no shape");
    // a closed shell result becomes a solid; solids are made outward-oriented
    if (r.ShapeType() == TopAbs_SHELL && BRep_Tool::IsClosed(r)) {
      TopoDS_Shape so = cad_shell_to_solid(TopoDS::Shell(r));
      if (!so.IsNull()) r = so;
    } else if (r.ShapeType() == TopAbs_SOLID) {
      TopoDS_Shape so = cad_canonical_solid(r);
      if (!so.IsNull()) r = so;
    }
    return cad_own(r);
  }
  CAD_CATCH(nullptr)
}

/// Patch: fills the closed boundary formed by `edges` (a compound of edges). A planar loop
/// becomes a planar face; otherwise an N-sided filling surface (C0 to the boundary) is built.
inline std::unique_ptr<TopoDS_Shape> cad_fill(const TopoDS_Shape &edges) {
  try {
    cad_err_ref().clear();
    Handle(TopTools_HSequenceOfShape) seq = new TopTools_HSequenceOfShape();
    for (TopExp_Explorer ex(edges, TopAbs_EDGE); ex.More(); ex.Next()) seq->Append(ex.Current());
    if (seq->IsEmpty()) return cad_fail("no boundary edges selected");
    Handle(TopTools_HSequenceOfShape) wires = new TopTools_HSequenceOfShape();
    ShapeAnalysis_FreeBounds::ConnectEdgesToWires(seq, 1.0e-4, Standard_False, wires);
    if (wires->Length() != 1) return cad_fail("the boundary edges must form one closed loop");
    TopoDS_Wire w = TopoDS::Wire(wires->Value(1));
    if (!BRep_Tool::IsClosed(w)) return cad_fail("the boundary is not closed");
    {
      BRepBuilderAPI_MakeFace mf(w, Standard_True);
      if (mf.IsDone()) {
        ShapeFix_Face fix(mf.Face());
        fix.FixOrientation();
        fix.Perform();
        return cad_own(fix.Face());
      }
    }
    BRepOffsetAPI_MakeFilling fill;
    for (int k = 1; k <= seq->Length(); ++k) fill.Add(TopoDS::Edge(seq->Value(k)), GeomAbs_C0, Standard_True);
    fill.Build();
    if (!fill.IsDone()) return cad_fail("the boundary could not be filled");
    return cad_own(fill.Shape());
  }
  CAD_CATCH(nullptr)
}

/// A mesh body: one face without a surface that carries the triangles as its triangulation
/// (tris = 9 coordinates per triangle; nodes are not shared so each triangle is shaded flat).
inline std::unique_ptr<TopoDS_Shape> cad_mesh_face(rust::Slice<const double> tris) {
  try {
    cad_err_ref().clear();
    const int n = (int)(tris.size() / 9);
    if (n == 0) return cad_fail("the mesh has no triangles");
    Handle(Poly_Triangulation) t = new Poly_Triangulation(3 * n, n, Standard_False, Standard_True);
    for (int k = 0; k < n; ++k) {
      const double *c = tris.data() + 9 * k;
      gp_Pnt p[3] = {gp_Pnt(c[0], c[1], c[2]), gp_Pnt(c[3], c[4], c[5]), gp_Pnt(c[6], c[7], c[8])};
      gp_Vec nv = gp_Vec(p[0], p[1]).Crossed(gp_Vec(p[0], p[2]));
      gp_Dir d = nv.Magnitude() > 1e-300 ? gp_Dir(nv) : gp_Dir(0, 0, 1);
      for (int j = 0; j < 3; ++j) {
        t->SetNode(3 * k + j + 1, p[j]);
        t->SetNormal(3 * k + j + 1, d);
      }
      t->SetTriangle(k + 1, Poly_Triangle(3 * k + 1, 3 * k + 2, 3 * k + 3));
    }
    TopoDS_Face f;
    BRep_Builder bb;
    bb.MakeFace(f, t);
    return cad_own(f);
  }
  CAD_CATCH(nullptr)
}

/// Mesh -> B-rep: one planar face per triangle, sewn with `tolerance`; closed shells become
/// solids; coplanar triangles are merged (ShapeUpgrade_UnifySameDomain). Returns a compound of
/// the pieces (see cad_sew).
inline std::unique_ptr<TopoDS_Shape> cad_mesh_to_brep(rust::Slice<const double> tris, double tolerance) {
  try {
    cad_err_ref().clear();
    const int n = (int)(tris.size() / 9);
    if (n == 0) return cad_fail("the mesh has no triangles");
    BRepBuilderAPI_Sewing sw(tolerance);
    int faces = 0;
    for (int k = 0; k < n; ++k) {
      const double *c = tris.data() + 9 * k;
      gp_Pnt p0(c[0], c[1], c[2]), p1(c[3], c[4], c[5]), p2(c[6], c[7], c[8]);
      if (gp_Vec(p0, p1).Crossed(gp_Vec(p0, p2)).Magnitude() < 1e-12) continue;  // degenerate
      BRepBuilderAPI_MakePolygon poly(p0, p1, p2, Standard_True);
      if (!poly.IsDone()) continue;
      BRepBuilderAPI_MakeFace mf(poly.Wire(), Standard_True);
      if (!mf.IsDone()) continue;
      sw.Add(mf.Face());
      ++faces;
    }
    if (faces == 0) return cad_fail("the mesh has no valid triangles");
    sw.Perform();
    TopoDS_Shape r = sw.SewedShape();
    TopoDS_Compound out;
    BRep_Builder bb;
    bb.MakeCompound(out);
    for (TopExp_Explorer ex(r, TopAbs_SHELL); ex.More(); ex.Next()) {
      TopoDS_Shell sh = TopoDS::Shell(ex.Current());
      TopoDS_Shape piece = sh;
      if (BRep_Tool::IsClosed(sh)) {
        TopoDS_Shape so = cad_shell_to_solid(sh);
        if (!so.IsNull()) piece = so;
      }
      try {
        ShapeUpgrade_UnifySameDomain u(piece, Standard_True, Standard_True, Standard_False);
        u.Build();
        if (!u.Shape().IsNull()) piece = u.Shape();
      } catch (Standard_Failure &) {
      }
      bb.Add(out, piece);
    }
    for (TopExp_Explorer ex(r, TopAbs_FACE, TopAbs_SHELL); ex.More(); ex.Next()) bb.Add(out, ex.Current());
    return cad_own(out);
  }
  CAD_CATCH(nullptr)
}

// ===== inspect =====
// CAD360 INSPECT panel: minimum distance (Measure) and mass properties (Center of Mass). The
// interference check uses cad_boolean (common) + cad_volume.

#include <BRepExtrema_DistShapeShape.hxx>
#include <cmath>

/// Minimum distance between two shapes (faces, edges, vertices, solids or compounds).
/// out = [distance, ax, ay, az, bx, by, bz, inner] where a / b are the closest points on the
/// first / second shape and inner = 1 when one solid lies inside the other (distance 0).
inline bool cad_min_distance(const TopoDS_Shape &a, const TopoDS_Shape &b, rust::Slice<double> out) {
  try {
    cad_err_ref().clear();
    if (out.size() < 8) throw Standard_Failure("output buffer too small");
    if (a.IsNull() || b.IsNull()) throw Standard_Failure("empty shape");
    BRepExtrema_DistShapeShape d(a, b, Extrema_ExtFlag_MIN);
    if (!d.IsDone() || d.NbSolution() < 1) throw Standard_Failure("the minimum distance could not be computed");
    gp_Pnt pa = d.PointOnShape1(1), pb = d.PointOnShape2(1);
    out[0] = d.Value();
    out[1] = pa.X();
    out[2] = pa.Y();
    out[3] = pa.Z();
    out[4] = pb.X();
    out[5] = pb.Y();
    out[6] = pb.Z();
    out[7] = d.InnerSolution() ? 1.0 : 0.0;
    return true;
  }
  CAD_CATCH(false)
}

/// Mass properties with unit density. out = [volume, area, cx, cy, cz] where (cx, cy, cz) is the
/// centre of mass of the volume (of the surface when the shape encloses no volume).
inline bool cad_mass_props(const TopoDS_Shape &s, rust::Slice<double> out) {
  try {
    cad_err_ref().clear();
    if (out.size() < 5) throw Standard_Failure("output buffer too small");
    if (s.IsNull()) throw Standard_Failure("empty shape");
    GProp_GProps vp, sp;
    BRepGProp::SurfaceProperties(s, sp);
    bool solid = TopExp_Explorer(s, TopAbs_SOLID).More();
    if (solid) BRepGProp::VolumeProperties(s, vp);
    const GProp_GProps &c = solid && std::abs(vp.Mass()) > 1e-12 ? vp : sp;
    gp_Pnt g = c.CentreOfMass();
    out[0] = solid ? std::abs(vp.Mass()) : 0.0;
    out[1] = sp.Mass();
    out[2] = g.X();
    out[3] = g.Y();
    out[4] = g.Z();
    return true;
  }
  CAD_CATCH(false)
}
