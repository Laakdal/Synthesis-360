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
