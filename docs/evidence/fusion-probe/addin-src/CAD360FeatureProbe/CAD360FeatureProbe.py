"""CAD360 ASM feature-level reference probe (pass v6) for Autodesk Fusion.

Builds every case of research/feature-cases.json (extrude, revolve, fillet, chamfer, shell) as a
real parametric feature in a new, unsaved Fusion design, measures the resulting body (topology,
per-face / per-edge / per-vertex geometry, mass properties, tolerances) and exports it, so the Rust
kernel (crates/cad-geom) can be compared against Autodesk ShapeManager (ASM).

Black-box only: everything goes through Fusion's public Python API.

Safety:
  * does nothing unless the sentinel file exists (it is deleted on start);
  * never saves to the cloud; every document it opens is closed with close(False);
  * raw output goes to a local staging folder (%LOCALAPPDATA%/Temp/CAD360FeatureProbe-out) and is
    scrubbed of user names / absolute user paths before anything is copied into research/.

Units: the Fusion API uses centimetres and radians. Raw geometry is logged in cm (keys end in
_cm/_cm2/_cm3); verification values computed here are in mm (keys end in _mm).
"""
import adsk.core, adsk.fusion, traceback, json, os, threading, time, math, sys

STAGE = os.path.join(os.environ.get("LOCALAPPDATA", ""), "Temp", "CAD360FeatureProbe-out")
SENTINEL = os.path.join(STAGE, "RUN_FEATURE_PROBE")
CASES_PATH = r"C:\Mudrock's Headquarters\Project\CAD360\research\feature-cases.json"
EXPORT = os.path.join(STAGE, "export")
EVENT_ID = "CAD360FeatureProbeStepEvent"
MM = 0.1  # mm -> cm
VERY_HIGH = adsk.fusion.CalculationAccuracy.VeryHighCalculationAccuracy
PICK_TOL_CM = 1e-5      # 1e-4 mm: max distance between a case point and the picked entity
SPAN_TOL_MM = 1e-5

app = None
ui = None
tbm = None
handlers = []
stop_flag = threading.Event()
step_done = threading.Event()
state = {}

SURF = {0: "plane", 1: "cylinder", 2: "cone", 3: "sphere", 4: "torus", 5: "elliptical_cylinder",
        6: "elliptical_cone", 7: "nurbs"}
CURVE = {0: "line", 1: "arc", 2: "circle", 3: "ellipse", 4: "elliptical_arc", 5: "infinite_line", 6: "nurbs",
         7: "polyline"}


# --------------------------------------------------------------------------- logging / helpers
def log(kind, **data):
    rec = {"t": time.strftime("%Y-%m-%dT%H:%M:%S"), "kind": kind}
    rec.update(data)
    with open(os.path.join(STAGE, "probe-log.jsonl"), "a", encoding="utf-8") as f:
        f.write(json.dumps(rec, default=str) + "\n")


def err_text(e):
    return {"type": type(e).__name__, "text": str(e)}


def call(rec, label, fn):
    """Run fn(); store its return value (or the exception) under rec[label]. Never raises."""
    try:
        v = fn()
        rec[label] = {"ok": True, "value": v}
        return v
    except Exception as e:
        rec[label] = {"ok": False, "error": err_text(e), "trace": traceback.format_exc(limit=3)}
        return None


def safe(fn, default=None):
    try:
        return fn()
    except Exception:
        return default


def pt(p):
    return None if p is None else [p.x, p.y, p.z]


def vec(v):
    return None if v is None else [v.x, v.y, v.z]


def desc(v):
    if v is None:
        return None
    if isinstance(v, (bool, int, float, str)):
        return v
    try:
        return v.objectType
    except Exception:
        return repr(v)


def VI(x):
    return adsk.core.ValueInput.createByReal(x)


# vector maths on python lists (mm unless stated)
def add(a, b):
    return [a[i] + b[i] for i in range(3)]


def sub(a, b):
    return [a[i] - b[i] for i in range(3)]


def mul(a, s):
    return [a[i] * s for i in range(3)]


def dot(a, b):
    return sum(a[i] * b[i] for i in range(3))


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def norm(a):
    return math.sqrt(dot(a, a))


def unit(a):
    n = norm(a)
    return [x / n for x in a]


def P3(xyz_mm):
    return adsk.core.Point3D.create(xyz_mm[0] * MM, xyz_mm[1] * MM, xyz_mm[2] * MM)


def mm_of(p):
    return [p.x / MM, p.y / MM, p.z / MM]


# --------------------------------------------------------------------------- case geometry (mm)
class Frame:
    def __init__(self, plane):
        self.o = [float(v) for v in plane["origin"]]
        self.x = unit([float(v) for v in plane["x"]])
        self.y = unit([float(v) for v in plane["y"]])
        self.z = unit(cross(self.x, self.y))

    def world(self, uv):
        return add(self.o, add(mul(self.x, uv[0]), mul(self.y, uv[1])))


def arc_point(seg, ang_deg):
    a = math.radians(ang_deg)
    return [seg["c"][0] + seg["r"] * math.cos(a), seg["c"][1] + seg["r"] * math.sin(a)]


def loop_signed_area(loop):
    """Green's theorem over a loop of segments, in plane coordinates (mm^2)."""
    A = 0.0
    for s in loop:
        if "line" in s:
            a, b = s["line"]["a"], s["line"]["b"]
            A += 0.5 * (a[0] * b[1] - b[0] * a[1])
        elif "arc" in s:
            g = s["arc"]
            t0 = math.radians(g["a0"])
            t1 = t0 + math.radians(g["sweep"])
            cx, cy, r = g["c"][0], g["c"][1], g["r"]
            A += 0.5 * (cx * r * (math.sin(t1) - math.sin(t0)) - cy * r * (math.cos(t1) - math.cos(t0))
                        + r * r * (t1 - t0))
        elif "circle" in s:
            A += math.pi * s["circle"]["r"] ** 2
    return A


def region_area(loops):
    areas = sorted((abs(loop_signed_area(l)) for l in loops), reverse=True)
    return areas[0] - sum(areas[1:])


def loop_sample_points(loop):
    pts = []
    for s in loop:
        if "line" in s:
            pts += [s["line"]["a"], s["line"]["b"]]
        elif "arc" in s:
            g = s["arc"]
            for k in range(9):
                pts.append(arc_point(g, g["a0"] + g["sweep"] * k / 8.0))
        elif "circle" in s:
            g = s["circle"]
            for k in range(8):
                pts.append([g["c"][0] + g["r"] * math.cos(k * math.pi / 4), g["c"][1] + g["r"] * math.sin(k * math.pi / 4)])
    return pts


# --------------------------------------------------------------------------- sketch construction
def make_sketch(root, frame, info):
    """Create a sketch whose plane equals the case plane; returns (sketch, sign) where
    sign = (sketch normal in model space) . (case normal z = x cross y)."""
    z = frame.z
    cp = None
    base = None
    for name, n in (("xYConstructionPlane", [0, 0, 1]), ("xZConstructionPlane", [0, 1, 0]),
                    ("yZConstructionPlane", [1, 0, 0])):
        if norm(cross(z, n)) < 1e-12:
            base = (name, n)
            break
    if base is not None:
        bp = getattr(root, base[0])
        off = dot(frame.o, base[1])  # signed offset along the world axis
        if abs(off) < 1e-12:
            cp = bp
            info["construction_plane"] = base[0]
        else:
            tried = []
            for s in (1.0, -1.0):
                ci = root.constructionPlanes.createInput()
                ci.setByOffset(bp, VI(s * off * MM))
                c = root.constructionPlanes.add(ci)
                g = c.geometry
                d = dot(sub(frame.o, mm_of(g.origin)), vec(g.normal))
                tried.append({"offset_cm": s * off * MM, "residual_mm": d})
                if abs(d) < 1e-9:
                    cp = c
                    info["construction_plane"] = "setByOffset(%s, %r cm)" % (base[0], s * off * MM)
                    break
                c.deleteMe()
            info["offset_tries"] = tried
    elif abs(dot(z, [1, 0, 0])) < 1e-12 and abs(dot(frame.o, z)) < 1e-9:
        # plane contains the world X axis: rotate the XY plane about the X construction axis
        ang = math.atan2(-z[1], z[2])
        tried = []
        for a in (ang, -ang):
            ci = root.constructionPlanes.createInput()
            ci.setByAngle(root.xConstructionAxis, VI(a), root.xYConstructionPlane)
            c = root.constructionPlanes.add(ci)
            g = c.geometry
            gn = vec(g.normal)
            par = norm(cross(gn, z))
            res = dot(sub(frame.o, mm_of(g.origin)), gn)
            tried.append({"angle_rad": a, "plane_normal": gn, "parallel_residual": par, "contains_origin_residual_mm": res})
            if par < 1e-9 and abs(res) < 1e-9:
                cp = c
                info["construction_plane"] = "setByAngle(xConstructionAxis, %r rad, xYConstructionPlane)" % a
                break
            c.deleteMe()
        info["angle_tries"] = tried
    if cp is None:
        raise RuntimeError("no construction-plane strategy for plane " + json.dumps({"o": frame.o, "x": frame.x, "y": frame.y}))
    g = cp.geometry
    info["construction_plane_geometry_cm"] = {"origin": pt(g.origin), "normal": vec(g.normal)}
    sk = root.sketches.add(cp)
    cs = tuple(sk.transform.getAsCoordinateSystem())
    if len(cs) == 5:  # tolerate a leading success flag
        cs = cs[1:]
    o, xa, ya, za = cs
    info["sketch_transform"] = {"origin_cm": pt(o), "x": vec(xa), "y": vec(ya), "z": vec(za)}
    sign = 1.0 if dot(vec(za), z) > 0 else -1.0
    info["sketch_normal_dot_case_normal"] = dot(vec(za), z)
    return sk, sign


def draw_profile(sk, sign, frame, loops, info):
    lines = sk.sketchCurves.sketchLines
    arcs = sk.sketchCurves.sketchArcs
    circles = sk.sketchCurves.sketchCircles
    maxz = [0.0]
    arcs_info = []

    def Psk(uv):
        p = sk.modelToSketchSpace(P3(frame.world(uv)))
        maxz[0] = max(maxz[0], abs(p.z))
        return adsk.core.Point3D.create(p.x, p.y, 0.0)

    def world_cm(uv):
        return [v * MM for v in frame.world(uv)]

    def sp_at(arc, uv):
        """Return the arc's sketch point (start or end) that lies at the case point uv."""
        w = world_cm(uv)
        best = None
        for sp in (arc.startSketchPoint, arc.endSketchPoint):
            d = norm(sub(pt(sp.worldGeometry), w))
            if best is None or d < best[0]:
                best = (d, sp)
        return best

    for loop in loops:
        first_sp = None
        last_sp = None
        n = len(loop)
        for i, seg in enumerate(loop):
            is_last = (i == n - 1)
            if "circle" in seg:
                g = seg["circle"]
                circles.addByCenterRadius(Psk(g["c"]), g["r"] * MM)
            elif "line" in seg:
                g = seg["line"]
                a = last_sp if last_sp is not None else Psk(g["a"])
                b = first_sp if (is_last and first_sp is not None) else Psk(g["b"])
                ln = lines.addByTwoPoints(a, b)
                if first_sp is None:
                    first_sp = ln.startSketchPoint
                last_sp = ln.endSketchPoint
            elif "arc" in seg:
                g = seg["arc"]
                p_start = arc_point(g, g["a0"])
                p_end = arc_point(g, g["a0"] + g["sweep"])
                p_mid = arc_point(g, g["a0"] + g["sweep"] / 2.0)
                start = last_sp if last_sp is not None else Psk(p_start)
                ai = {"case": g, "sketch_sign": sign}
                arc = None
                for attempt, s in enumerate((sign, -sign)):
                    sw = math.radians(g["sweep"]) * s
                    arc = arcs.addByCenterStartSweep(Psk(g["c"]), start, sw)
                    ev = arc.worldGeometry.evaluator
                    ok, p0, p1 = ev.getParameterExtents()
                    ok2, pm = ev.getPointAtParameter(0.5 * (p0 + p1))
                    dmid = norm(sub(pt(pm), world_cm(p_mid)))
                    ai.setdefault("attempts", []).append({"sweep_rad_passed": sw, "mid_error_cm": dmid})
                    if dmid < 1e-7:
                        ai["sweep_rad_used"] = sw
                        ai["flipped_vs_sign_rule"] = attempt == 1
                        break
                    arc.deleteMe()
                    arc = None
                if arc is None:
                    raise RuntimeError("arc midpoint never matched: " + json.dumps(g))
                d_end, end_sp = sp_at(arc, p_end)
                d_start, start_sp = sp_at(arc, p_start)
                ai["end_error_cm"] = d_end
                ai["start_error_cm"] = d_start
                ai["start_is_arc_startSketchPoint"] = start_sp == arc.startSketchPoint
                if first_sp is None:
                    first_sp = start_sp
                if is_last and first_sp is not None and end_sp is not first_sp:
                    ai["merge_closing_point"] = safe(lambda: end_sp.merge(first_sp), "merge failed")
                last_sp = end_sp
                arcs_info.append(ai)
    info["arcs"] = arcs_info
    info["max_abs_sketch_z_cm"] = maxz[0]
    info["sketch_curves"] = sk.sketchCurves.count


def pick_profile(sk, expected_mm2, info):
    areas = []
    best = None
    for i in range(sk.profiles.count):
        p = sk.profiles.item(i)
        a = p.areaProperties(VERY_HIGH).area / (MM * MM)
        areas.append(a)
        err = abs(a - expected_mm2) / expected_mm2
        if best is None or err < best[0]:
            best = (err, i, p)
    info["profile_areas_mm2"] = areas
    info["expected_region_area_mm2"] = expected_mm2
    info["chosen_profile"] = best[1] if best else None
    info["chosen_profile_rel_error"] = best[0] if best else None
    if best is None:
        raise RuntimeError("sketch has no profiles")
    if best[0] > 1e-7:
        info["profile_area_warning"] = "no profile matches the expected region area within 1e-7 relative"
    return best[2]


# --------------------------------------------------------------------------- body helpers
def span_along(body, origin_mm, n):
    vals = []
    for e in body.edges:
        try:
            ev = e.evaluator
            ok, p0, p1 = ev.getParameterExtents()
            ok2, pts = ev.getStrokes(p0, p1, 1e-4)
            for p in pts:
                vals.append(dot(sub(mm_of(p), origin_mm), n))
        except Exception:
            pass
    for v in body.vertices:
        vals.append(dot(sub(mm_of(v.geometry), origin_mm), n))
    if not vals:
        return None
    return [min(vals), max(vals)]


def centroid_mm(body):
    pp = body.getPhysicalProperties(VERY_HIGH)
    return mm_of(pp.centerOfMass)


def feature_health(feat, out):
    call(out, "healthState", lambda: feat.healthState)
    call(out, "errorOrWarningMessage", lambda: feat.errorOrWarningMessage)


# --------------------------------------------------------------------------- feature builders
def build_extrude(root, c, fa):
    frame = Frame(c["profile"]["plane"])
    sk_info = {}
    fa["sketch"] = sk_info
    sk, sign = make_sketch(root, frame, sk_info)
    draw_profile(sk, sign, frame, c["profile"]["loops"], sk_info)
    expected = region_area(c["profile"]["loops"])
    lo, hi = min(c["from"], c["to"]), max(c["from"], c["to"])
    fa["target_span_mm"] = [lo, hi]
    exts = root.features.extrudeFeatures
    NB = adsk.fusion.FeatureOperations.NewBodyFeatureOperation
    POS = adsk.fusion.ExtentDirections.PositiveExtentDirection
    NEG = adsk.fusion.ExtentDirections.NegativeExtentDirection

    def attempt(flip):
        prof = pick_profile(sk, expected, sk_info)
        ei = exts.createInput(prof, NB)
        if lo == 0 or hi == 0:
            d = hi - lo
            case_dir = 1.0 if hi > 0 else -1.0
            want = case_dir * sign * (-1.0 if flip else 1.0)
            dirn = POS if want > 0 else NEG
            ei.setOneSideExtent(adsk.fusion.DistanceExtentDefinition.create(VI(d * MM)), dirn)
            api = "setOneSideExtent(DistanceExtentDefinition.create(%r cm), %s)" % (
                d * MM, "PositiveExtentDirection" if dirn == POS else "NegativeExtentDirection")
        elif abs(lo + hi) < 1e-12:
            ei.setSymmetricExtent(VI(hi * MM), False)
            api = "setSymmetricExtent(%r cm, isFullLength=False)" % (hi * MM)
        elif lo < 0 < hi:
            a, b = (hi, -lo) if (sign > 0) != flip else (-lo, hi)
            ei.setTwoSidesExtent(adsk.fusion.DistanceExtentDefinition.create(VI(a * MM)),
                                 adsk.fusion.DistanceExtentDefinition.create(VI(b * MM)))
            api = "setTwoSidesExtent(%r cm, %r cm)" % (a * MM, b * MM)
        else:
            start, d = (lo, hi - lo) if hi > 0 else (hi, hi - lo)
            raise RuntimeError("offset-start extrude not implemented (span %r..%r)" % (lo, hi))
        feat = exts.add(ei)
        return feat, api

    tries = []
    feat = None
    for flip in (False, True):
        feat, api = attempt(flip)
        body = root.bRepBodies.item(root.bRepBodies.count - 1)
        span = span_along(body, frame.o, frame.z)
        cen = centroid_mm(body)
        cz = dot(sub(cen, frame.o), frame.z)
        ok = span is not None and abs(span[0] - lo) < SPAN_TOL_MM and abs(span[1] - hi) < SPAN_TOL_MM
        tries.append({"api": api, "flip": flip, "span_mm": span, "centroid_offset_mm": cz,
                      "centroid_offset_expected_mm": 0.5 * (lo + hi), "span_ok": ok})
        if ok:
            fa["extent_api"] = api
            break
        feat.deleteMe()
        feat = None
    fa["extent_tries"] = tries
    fa["operation"] = "NewBodyFeatureOperation"
    if feat is None:
        raise RuntimeError("extrude span never matched the target range")
    fa["span_verified"] = True
    return feat


def build_revolve(root, c, fa):
    frame = Frame(c["profile"]["plane"])
    sk_info = {}
    fa["sketch"] = sk_info
    sk, sign = make_sketch(root, frame, sk_info)
    draw_profile(sk, sign, frame, c["profile"]["loops"], sk_info)
    expected = region_area(c["profile"]["loops"])
    ax_o = [float(v) for v in c["axis"]["origin"]]
    ax_d = unit([float(v) for v in c["axis"]["dir"]])
    axis = None
    if norm(ax_o) < 1e-12:
        for name, n in (("zConstructionAxis", [0, 0, 1]), ("xConstructionAxis", [1, 0, 0]),
                        ("yConstructionAxis", [0, 1, 0])):
            if norm(cross(ax_d, n)) < 1e-12:
                axis = getattr(root, name)
                fa["axis_entity"] = name
                fa["axis_entity_dir_dot_case_dir"] = dot(ax_d, n)
                break
    if axis is None:
        raise RuntimeError("no construction axis matches the case axis")
    ang = float(c["angle"])
    revs = root.features.revolveFeatures
    NB = adsk.fusion.FeatureOperations.NewBodyFeatureOperation
    # direction check reference: interior point of the outer loop, perpendicular to the axis
    outer = max(c["profile"]["loops"], key=lambda l: abs(loop_signed_area(l)))
    sp = loop_sample_points(outer)
    inner_uv = [sum(p[0] for p in sp) / len(sp), sum(p[1] for p in sp) / len(sp)]
    w = sub(frame.world(inner_uv), ax_o)
    w_perp = sub(w, mul(ax_d, dot(w, ax_d)))
    side = cross(ax_d, unit(w_perp))  # +angle (right-hand about ax_d) sweeps towards this side
    fa["side_reference"] = {"profile_point_mm": frame.world(inner_uv), "expected_side_dir": side}
    tries = []
    feat = None
    candidates = [math.radians(ang)] if ang >= 360 else [math.radians(ang), -math.radians(ang)]
    for a in candidates:
        prof = pick_profile(sk, expected, sk_info)
        ri = revs.createInput(prof, axis, NB)
        ri.setAngleExtent(False, VI(a))
        api = "setAngleExtent(isSymmetric=False, %r rad)" % a
        feat = revs.add(ri)
        body = root.bRepBodies.item(root.bRepBodies.count - 1)
        cen = centroid_mm(body)
        s = dot(sub(cen, ax_o), side)
        ok = True if ang >= 360 else s > 1e-6
        tries.append({"api": api, "centroid_mm": cen, "centroid_dot_expected_side_mm": s, "side_ok": ok})
        if ok:
            fa["extent_api"] = api
            break
        feat.deleteMe()
        feat = None
    fa["extent_tries"] = tries
    fa["operation"] = "NewBodyFeatureOperation"
    if feat is None:
        raise RuntimeError("revolve direction never matched the right-hand rule")
    return feat


def find_edges(body, points, fa):
    res = []
    out = []
    for p in points:
        P = P3(p)
        hits = []
        for i, e in enumerate(body.edges):
            try:
                ev = e.evaluator
                ok, prm = ev.getParameterAtPoint(P)
                if not ok:
                    continue
                ok2, q = ev.getPointAtParameter(prm)
                ok3, p0, p1 = ev.getParameterExtents()
                d = q.distanceTo(P)
                inside = (p0 - 1e-9) <= prm <= (p1 + 1e-9)
                if d < PICK_TOL_CM and inside:
                    hits.append((i, e, d))
            except Exception:
                pass
        fb = safe(lambda: body.parentComponent.findBRepUsingPoint(
            P, adsk.fusion.BRepEntityTypes.BRepEdgeEntityType, PICK_TOL_CM, False).count)
        out.append({"point_mm": p, "hits": [h[0] for h in hits], "distances_cm": [h[2] for h in hits],
                    "findBRepUsingPoint_count": fb})
        if len(hits) != 1:
            raise RuntimeError("edge pick at %r is not unique: %d hits" % (p, len(hits)))
        res.append(hits[0][1])
    fa["edge_picks"] = out
    return res


def find_faces(body, points, fa):
    res = []
    out = []
    for p in points:
        P = P3(p)
        hits = []
        for i, f in enumerate(body.faces):
            try:
                ev = f.evaluator
                ok, uv = ev.getParameterAtPoint(P)
                if not ok:
                    continue
                ok2, q = ev.getPointAtParameter(uv)
                d = q.distanceTo(P)
                on = ev.isParameterOnFace(uv)
                if d < PICK_TOL_CM and on:
                    hits.append((i, f, d))
            except Exception:
                pass
        fb = safe(lambda: body.parentComponent.findBRepUsingPoint(
            P, adsk.fusion.BRepEntityTypes.BRepFaceEntityType, PICK_TOL_CM, False).count)
        out.append({"point_mm": p, "hits": [h[0] for h in hits], "distances_cm": [h[2] for h in hits],
                    "findBRepUsingPoint_count": fb})
        if len(hits) != 1:
            raise RuntimeError("face pick at %r is not unique: %d hits" % (p, len(hits)))
        res.append(hits[0][1])
    fa["face_picks"] = out
    return res


def coll(items):
    oc = adsk.core.ObjectCollection.create()
    for it in items:
        oc.add(it)
    return oc


def build_modifier(root, c, base_case, fa):
    fa["base"] = {}
    if base_case["kind"] == "extrude":
        build_extrude(root, base_case, fa["base"])
    else:
        build_revolve(root, base_case, fa["base"])
    body = root.bRepBodies.item(0)
    fa["base_body"] = {"faces": body.faces.count, "edges": body.edges.count, "vertices": body.vertices.count}
    k = c["kind"]
    if k == "fillet":
        edges = find_edges(body, c["edges"], fa)
        fl = root.features.filletFeatures
        fi = fl.createInput()
        fa["isRollingBallCorner_default"] = safe(lambda: fi.isRollingBallCorner)
        es = fi.edgeSetInputs.addConstantRadiusEdgeSet(coll(edges), VI(c["radius"] * MM), False)
        fa["edge_set_continuity_default"] = safe(lambda: es.continuity)
        fa["api"] = ("FilletFeatures.createInput(); edgeSetInputs.addConstantRadiusEdgeSet(%d edges, %r cm, "
                     "isTangentChain=False); isRollingBallCorner left at default" % (len(edges), c["radius"] * MM))
        return fl.add(fi)
    if k == "chamfer":
        edges = find_edges(body, c["edges"], fa)
        ch = root.features.chamferFeatures
        ci = ch.createInput2()
        fa["cornerType_default"] = safe(lambda: ci.cornerType)
        r = ci.chamferEdgeSets.addEqualDistanceChamferEdgeSet(coll(edges), VI(c["distance"] * MM), False)
        fa["addEqualDistanceChamferEdgeSet_returned"] = r
        fa["api"] = ("ChamferFeatures.createInput2(); chamferEdgeSets.addEqualDistanceChamferEdgeSet(%d edges, %r cm, "
                     "isTangentChain=False); cornerType left at default" % (len(edges), c["distance"] * MM))
        return ch.add(ci)
    if k == "shell":
        faces = find_faces(body, c["faces"], fa)
        sh = root.features.shellFeatures
        si = sh.createInput(coll(faces), False)
        if c.get("direction", "inside") == "inside":
            si.insideThickness = VI(c["thickness"] * MM)
        else:
            si.outsideThickness = VI(c["thickness"] * MM)
        fa["shellType_default"] = safe(lambda: si.shellType)
        fa["api"] = ("ShellFeatures.createInput(%d faces, isTangentChain=False); insideThickness=%r cm; "
                     "shellType left at default" % (len(faces), c["thickness"] * MM))
        return sh.add(si)
    raise ValueError("unknown kind " + k)


# --------------------------------------------------------------------------- measurement
def topology(body):
    """v5 measurement (unchanged keys)."""
    t = {}
    call(t, "isSolid", lambda: body.isSolid)
    call(t, "isTemporary", lambda: body.isTemporary)
    call(t, "lumps", lambda: body.lumps.count)
    call(t, "shells", lambda: body.shells.count)
    call(t, "faces", lambda: body.faces.count)
    call(t, "edges", lambda: body.edges.count)
    call(t, "vertices", lambda: body.vertices.count)
    call(t, "body_volume_cm3", lambda: body.volume)
    call(t, "body_area_cm2", lambda: body.area)
    call(t, "bbox_cm", lambda: [pt(body.boundingBox.minPoint), pt(body.boundingBox.maxPoint)])
    shells = []
    try:
        for s in body.shells:
            d = {}
            call(d, "isClosed", lambda: s.isClosed)
            call(d, "isVoid", lambda: s.isVoid)
            call(d, "faces", lambda: s.faces.count)
            shells.append(d)
    except Exception as e:
        shells.append({"error": err_text(e)})
    t["shell_detail"] = shells
    return t


def surface_params(g):
    st = g.surfaceType
    d = {"type": SURF.get(st, "other:%d" % st)}
    if st == 0:
        p = adsk.core.Plane.cast(g)
        d.update(origin_cm=pt(p.origin), normal=vec(p.normal), uDirection=vec(p.uDirection), vDirection=vec(p.vDirection))
    elif st == 1:
        p = adsk.core.Cylinder.cast(g)
        d.update(origin_cm=pt(p.origin), axis=vec(p.axis), radius_cm=p.radius)
    elif st == 2:
        p = adsk.core.Cone.cast(g)
        d.update(origin_cm=pt(p.origin), axis=vec(p.axis), radius_cm=p.radius, halfAngle_rad=p.halfAngle)
    elif st == 3:
        p = adsk.core.Sphere.cast(g)
        d.update(origin_cm=pt(p.origin), radius_cm=p.radius)
    elif st == 4:
        p = adsk.core.Torus.cast(g)
        d.update(origin_cm=pt(p.origin), axis=vec(p.axis), majorRadius_cm=p.majorRadius, minorRadius_cm=p.minorRadius)
    elif st == 5:
        p = adsk.core.EllipticalCylinder.cast(g)
        d.update(origin_cm=pt(p.origin), axis=vec(p.axis), majorAxis=vec(p.majorAxis), majorRadius_cm=p.majorRadius,
                 minorRadius_cm=p.minorRadius)
    elif st == 6:
        p = adsk.core.EllipticalCone.cast(g)
        ok, o, ax, ma, mr, rr, ha = p.getData()
        d.update(origin_cm=pt(o), axis=vec(ax), majorAxisDirection=vec(ma), majorRadius_cm=mr, radiusRatio=rr,
                 halfAngle_rad=ha)
    elif st == 7:
        p = adsk.core.NurbsSurface.cast(g)
        ok, du, dv, cu, cv, cps, ku, kv, w, pu, pv = p.getData()
        d.update(degreeU=du, degreeV=dv, controlPointCountU=cu, controlPointCountV=cv,
                 controlPoints_cm=[pt(q) for q in cps], knotsU=list(ku), knotsV=list(kv), weights=list(w),
                 propertiesU=pu, propertiesV=pv)
    return d


def curve_params(g):
    if g is None:
        return {"type": "none"}
    ct = g.curveType
    d = {"type": CURVE.get(ct, "other:%d" % ct)}
    if ct == 0:
        p = adsk.core.Line3D.cast(g)
        d.update(startPoint_cm=pt(p.startPoint), endPoint_cm=pt(p.endPoint))
    elif ct == 1:
        p = adsk.core.Arc3D.cast(g)
        d.update(center_cm=pt(p.center), normal=vec(p.normal), radius_cm=p.radius, startAngle_rad=p.startAngle,
                 endAngle_rad=p.endAngle, referenceVector=vec(p.referenceVector), startPoint_cm=pt(p.startPoint),
                 endPoint_cm=pt(p.endPoint))
    elif ct == 2:
        p = adsk.core.Circle3D.cast(g)
        d.update(center_cm=pt(p.center), normal=vec(p.normal), radius_cm=p.radius)
    elif ct == 3:
        p = adsk.core.Ellipse3D.cast(g)
        d.update(center_cm=pt(p.center), normal=vec(p.normal), majorAxis=vec(p.majorAxis), majorRadius_cm=p.majorRadius,
                 minorRadius_cm=p.minorRadius)
    elif ct == 4:
        p = adsk.core.EllipticalArc3D.cast(g)
        d.update(center_cm=pt(p.center), normal=vec(p.normal), majorAxis=vec(p.majorAxis), majorRadius_cm=p.majorRadius,
                 minorRadius_cm=p.minorRadius, startAngle_rad=p.startAngle, endAngle_rad=p.endAngle,
                 startPoint_cm=pt(p.startPoint), endPoint_cm=pt(p.endPoint))
    elif ct == 6:
        p = adsk.core.NurbsCurve3D.cast(g)
        ok, cps, deg, knots, rat, w, per = p.getData()
        d.update(degree=deg, controlPointCount=len(cps), controlPoints_cm=[pt(q) for q in cps], knots=list(knots),
                 isRational=rat, weights=list(w), isPeriodic=per)
    return d


def details(body):
    faces = list(body.faces)
    edges = list(body.edges)
    verts = list(body.vertices)
    fidx = {f.tempId: i for i, f in enumerate(faces)}
    eidx = {e.tempId: i for i, e in enumerate(edges)}
    vidx = {v.tempId: i for i, v in enumerate(verts)}
    fd = []
    for i, f in enumerate(faces):
        d = {"index": i}
        call(d, "tempId", lambda: f.tempId)
        call(d, "surface", lambda: surface_params(f.geometry))
        call(d, "isParamReversed", lambda: f.isParamReversed)
        call(d, "area_cm2", lambda: f.area)
        pof = call(d, "pointOnFace_cm", lambda: pt(f.pointOnFace))

        def _n():
            ok, n = f.evaluator.getNormalAtPoint(f.pointOnFace)
            return vec(n) if ok else None
        call(d, "normal_at_pointOnFace", _n)
        call(d, "shell_index", lambda: [k for k, s in enumerate(body.shells) if any(x.tempId == f.tempId for x in s.faces)])

        def _loops():
            out = []
            for l in f.loops:
                out.append({"isOuter": l.isOuter,
                            "coedges": [{"edge": eidx.get(ce.edge.tempId), "isOpposedToEdge": ce.isOpposedToEdge}
                                        for ce in l.coEdges]})
            return out
        call(d, "loops", _loops)
        fd.append(d)
    ed = []
    for i, e in enumerate(edges):
        d = {"index": i}
        call(d, "tempId", lambda: e.tempId)
        call(d, "curve", lambda: curve_params(e.geometry))
        call(d, "isDegenerate", lambda: e.isDegenerate)
        call(d, "startVertex", lambda: vidx.get(e.startVertex.tempId) if e.startVertex else None)
        call(d, "endVertex", lambda: vidx.get(e.endVertex.tempId) if e.endVertex else None)

        def _len():
            ev = e.evaluator
            ok, p0, p1 = ev.getParameterExtents()
            ok2, L = ev.getLengthAtParameter(p0, p1)
            ok3, pm = ev.getPointAtParameter(0.5 * (p0 + p1))
            return {"ok": ok and ok2, "p0": p0, "p1": p1, "length_cm": L, "midpoint_cm": pt(pm)}
        call(d, "length", _len)
        call(d, "faces", lambda: [fidx.get(ce.loop.face.tempId) for ce in e.coEdges])
        call(d, "coedge_opposed", lambda: [ce.isOpposedToEdge for ce in e.coEdges])
        call(d, "isTolerant", lambda: e.isTolerant)
        call(d, "tolerance_cm", lambda: e.tolerance)
        ed.append(d)
    vd = []
    for i, v in enumerate(verts):
        d = {"index": i}
        call(d, "tempId", lambda: v.tempId)
        call(d, "position_cm", lambda: pt(v.geometry))
        call(d, "isTolerant", lambda: v.isTolerant)
        call(d, "tolerance_cm", lambda: v.tolerance)
        call(d, "edges", lambda: [eidx.get(x.tempId) for x in v.edges])
        vd.append(d)
    return fd, ed, vd


def phys(pp_fn):
    r = {}
    pp = call(r, "get", pp_fn)
    if pp is None:
        r["get"]["value"] = None
        return r
    r["get"]["value"] = desc(pp)
    call(r, "volume_cm3", lambda: pp.volume)
    call(r, "area_cm2", lambda: pp.area)
    call(r, "density_kg_per_cm3", lambda: pp.density)
    call(r, "mass_kg", lambda: pp.mass)
    call(r, "accuracy", lambda: pp.accuracy)
    call(r, "centerOfMass_cm", lambda: pt(pp.centerOfMass))
    call(r, "xyz_moments_kgcm2", lambda: list(pp.getXYZMomentsOfInertia()))
    call(r, "principal_moments_kgcm2", lambda: list(pp.getPrincipalMomentsOfInertia()))
    return r


def exports(rec, design, root, body, case_id):
    out = {}
    path = os.path.join(EXPORT, case_id + ".step")
    em = design.exportManager
    call(out, "step", lambda: em.execute(em.createSTEPExportOptions(path, root)))
    out["step_exists"] = os.path.exists(path)
    tb = call(out, "tbm_copy", lambda: tbm.copy(body))
    if tb is not None:
        out["tbm_copy"]["value"] = desc(tb)
        for ext in ("smt", "sat"):
            p = os.path.join(EXPORT, case_id + "." + ext)
            d = {}
            call(d, "exportToFile", lambda: tbm.exportToFile([tb], p))
            d["exists"] = os.path.exists(p)
            d["bytes"] = os.path.getsize(p) if d["exists"] else 0
            out[ext] = d
    rec["exports"] = out


def run_case(c, by_id):
    rec = {"id": c["id"], "input": c}
    t0 = time.time()
    doc = None
    fa = {}
    rec["feature_api"] = fa
    try:
        doc = app.documents.add(adsk.core.DocumentTypes.FusionDesignDocumentType)
        design = adsk.fusion.Design.cast(app.activeProduct)
        design.designType = adsk.fusion.DesignTypes.ParametricDesignType
        rec["designType"] = design.designType
        root = design.rootComponent
        feat = None
        try:
            if c["kind"] == "extrude":
                feat = build_extrude(root, c, fa)
            elif c["kind"] == "revolve":
                feat = build_revolve(root, c, fa)
            else:
                feat = build_modifier(root, c, by_id[c["base"]], fa)
            rec["build_ok"] = feat is not None
        except Exception as e:
            rec["build_ok"] = False
            rec["build_error"] = err_text(e)
            rec["build_trace"] = traceback.format_exc(limit=6)
        if feat is not None:
            h = {}
            feature_health(feat, h)
            call(h, "objectType", lambda: feat.objectType)
            call(h, "feature_bodies", lambda: feat.bodies.count)
            call(h, "faces_created", lambda: feat.faces.count)
            rec["feature_health"] = h
        rec["bodies"] = root.bRepBodies.count
        rec["timeline_count"] = safe(lambda: design.timeline.count)
        rec["body_records"] = []
        for bi in range(root.bRepBodies.count):
            body = root.bRepBodies.item(bi)
            br = {"index": bi, "name": safe(lambda: body.name)}
            br["topology"] = topology(body)
            try:
                fd, ed, vd = details(body)
                br["faces_detail"], br["edges_detail"], br["vertices_detail"] = fd, ed, vd
            except Exception as e:
                br["details_error"] = err_text(e)
                br["details_trace"] = traceback.format_exc(limit=4)
            br["phys"] = phys(lambda: body.getPhysicalProperties(VERY_HIGH))
            if bi == root.bRepBodies.count - 1:
                exports(br, design, root, body, c["id"])
            rec["body_records"].append(br)
        rec["component_phys"] = phys(lambda: root.getPhysicalProperties(VERY_HIGH))
    except Exception as e:
        rec["case_error"] = err_text(e)
        rec["case_trace"] = traceback.format_exc(limit=6)
    finally:
        if doc is not None:
            try:
                doc.close(False)
            except Exception as e:
                rec["close_error"] = err_text(e)
    rec["seconds"] = round(time.time() - t0, 3)
    log("case", **rec)


# --------------------------------------------------------------------------- steps
def s_start():
    env = {"version": app.version, "python": sys.version, "documents": app.documents.count}
    call(env, "pointTolerance_cm", lambda: app.pointTolerance)
    call(env, "vectorAngleTolerance_rad", lambda: app.vectorAngleTolerance)
    log("environment", **env)
    sigs = {}
    import inspect
    for cls, names in [
        (adsk.fusion.ExtrudeFeatures, ["createInput", "add"]),
        (adsk.fusion.ExtrudeFeatureInput, ["setOneSideExtent", "setSymmetricExtent", "setTwoSidesExtent"]),
        (adsk.fusion.DistanceExtentDefinition, ["create"]),
        (adsk.fusion.RevolveFeatures, ["createInput", "add"]),
        (adsk.fusion.RevolveFeatureInput, ["setAngleExtent"]),
        (adsk.fusion.FilletFeatures, ["createInput", "add"]),
        (adsk.fusion.FilletEdgeSetInputs, ["addConstantRadiusEdgeSet"]),
        (adsk.fusion.ChamferFeatures, ["createInput2", "add"]),
        (adsk.fusion.ChamferEdgeSets, ["addEqualDistanceChamferEdgeSet"]),
        (adsk.fusion.ShellFeatures, ["createInput", "add"]),
        (adsk.fusion.ConstructionPlaneInput, ["setByOffset", "setByAngle"]),
        (adsk.fusion.Sketch, ["modelToSketchSpace", "sketchToModelSpace"]),
        (adsk.fusion.SketchArcs, ["addByCenterStartSweep"]),
        (adsk.fusion.SketchLines, ["addByTwoPoints"]),
        (adsk.fusion.SketchCircles, ["addByCenterRadius"]),
        (adsk.fusion.Profile, ["areaProperties"]),
        (adsk.fusion.Component, ["findBRepUsingPoint", "getPhysicalProperties"]),
        (adsk.fusion.BRepBody, ["getPhysicalProperties"]),
        (adsk.fusion.TemporaryBRepManager, ["copy", "exportToFile"]),
        (adsk.fusion.ExportManager, ["createSTEPExportOptions"]),
        (adsk.fusion.PhysicalProperties, ["getXYZMomentsOfInertia"]),
    ]:
        for n in names:
            fn = getattr(cls, n, None)
            k = cls.__name__ + "." + n
            try:
                s = str(inspect.signature(fn))
            except Exception as e:
                s = "inspect.signature failed: " + str(e)
            sigs[k] = {"present": fn is not None, "signature": s, "doc": getattr(fn, "__doc__", None)}
    log("api_signatures", signatures=sigs)


def make_case_step(c, by_id):
    def step():
        run_case(c, by_id)
    step.__name__ = "case_" + c["id"]
    return step


def s_finish():
    for i in range(app.documents.count - 1, -1, -1):
        doc = app.documents.item(i)
        try:
            # Fusion was started fresh for this pass, so every open document is ours: close without saving.
            doc.close(False)
        except Exception:
            log("close_failed", err=traceback.format_exc())
    log("done", open_documents=app.documents.count)
    with open(os.path.join(STAGE, "DONE"), "w") as f:
        f.write("done")


def build_steps():
    with open(CASES_PATH, encoding="utf-8") as f:
        cat = json.load(f)
    by_id = {c["id"]: c for c in cat["cases"]}
    steps = [s_start]
    for c in cat["cases"]:
        steps.append(make_case_step(c, by_id))
    steps.append(s_finish)
    return steps


# --------------------------------------------------------------------------- event plumbing
class StepHandler(adsk.core.CustomEventHandler):
    def notify(self, args):
        idx = int(args.additionalInfo)
        fn = state["steps"][idx]
        try:
            log("step_begin", step=fn.__name__)
            fn()
            log("step_end", step=fn.__name__)
        except Exception:
            log("step_error", step=fn.__name__, err=traceback.format_exc())
        finally:
            step_done.set()


def worker():
    for _ in range(45):
        if stop_flag.wait(1):
            return
    for i in range(len(state["steps"])):
        if stop_flag.is_set():
            return
        step_done.clear()
        app.fireCustomEvent(EVENT_ID, str(i))
        if not step_done.wait(300):
            log("step_timeout", index=i, step=state["steps"][i].__name__)
        stop_flag.wait(1)


def run(context):
    global app, ui, tbm
    try:
        app = adsk.core.Application.get()
        ui = app.userInterface
        if not os.path.exists(SENTINEL):
            return
        os.remove(SENTINEL)
        os.makedirs(EXPORT, exist_ok=True)
        tbm = adsk.fusion.TemporaryBRepManager.get()
        state["steps"] = build_steps()
        log("heartbeat", version=app.version, steps=[s.__name__ for s in state["steps"]])
        ev = app.registerCustomEvent(EVENT_ID)
        h = StepHandler()
        ev.add(h)
        handlers.append(h)
        threading.Thread(target=worker, daemon=True).start()
    except Exception:
        try:
            os.makedirs(STAGE, exist_ok=True)
            log("run_error", err=traceback.format_exc())
        except Exception:
            pass


def stop(context):
    stop_flag.set()
    try:
        app.unregisterCustomEvent(EVENT_ID)
    except Exception:
        pass
