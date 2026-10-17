"""CAD360 ASM reference probe (pass v5) for Autodesk Fusion.

Builds every case of research/cases.json with adsk.fusion.TemporaryBRepManager (the public
API onto Autodesk ShapeManager), measures topology / geometry / mass properties / tolerances
and exports the results, so the Rust kernel (crates/cad-geom) can be compared against ASM.

Black-box only: everything goes through Fusion's public Python API.

Safety:
  * does nothing unless the sentinel file exists (it is deleted on start);
  * never saves to the cloud; every document it opens is closed without saving;
  * raw output goes to a local staging folder (%LOCALAPPDATA%/Temp/CAD360AsmProbe-out) and is
    scrubbed of user names / absolute user paths before anything is copied into research/.

Units: the Fusion API uses centimetres. Raw values are logged in cm; the post-processor
converts to mm.
"""
import adsk.core, adsk.fusion, traceback, json, os, threading, time, math, sys

STAGE = os.path.join(os.environ.get("LOCALAPPDATA", ""), "Temp", "CAD360AsmProbe-out")
SENTINEL = os.path.join(STAGE, "RUN_ASM_PROBE")
CASES_PATH = r"C:\Mudrock's Headquarters\Project\CAD360\research\cases.json"
EXPORT = os.path.join(STAGE, "export")
EVENT_ID = "CAD360AsmProbeStepEvent"
MM = 0.1  # mm -> cm
VERY_HIGH = adsk.fusion.CalculationAccuracy.VeryHighCalculationAccuracy

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
BOOL = {"union": adsk.fusion.BooleanTypes.UnionBooleanType,
        "cut": adsk.fusion.BooleanTypes.DifferenceBooleanType,
        "intersect": adsk.fusion.BooleanTypes.IntersectionBooleanType}


# --------------------------------------------------------------------------- logging
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


def P(xyz_mm):
    return adsk.core.Point3D.create(xyz_mm[0] * MM, xyz_mm[1] * MM, xyz_mm[2] * MM)


def pt(p):
    return None if p is None else [p.x, p.y, p.z]


def desc(v):
    """Describe an API return value."""
    if v is None:
        return None
    if isinstance(v, (bool, int, float, str)):
        return v
    try:
        return v.objectType
    except Exception:
        return repr(v)


# --------------------------------------------------------------------------- builders (input in mm)
def build_primitive(spec, calls):
    k = spec["kind"]
    if k == "box":
        c, s = spec["corner"], spec["size"]
        centre = [c[i] + s[i] / 2.0 for i in range(3)]
        calls.append({"api": "OrientedBoundingBox3D.create", "centre_cm": [v * MM for v in centre],
                      "lengthDirection": [1, 0, 0], "widthDirection": [0, 1, 0],
                      "length_cm": s[0] * MM, "width_cm": s[1] * MM, "height_cm": s[2] * MM})
        obb = adsk.core.OrientedBoundingBox3D.create(P(centre), adsk.core.Vector3D.create(1, 0, 0),
                                                     adsk.core.Vector3D.create(0, 1, 0),
                                                     s[0] * MM, s[1] * MM, s[2] * MM)
        calls[-1]["returned"] = desc(obb)
        b = tbm.createBox(obb)
        calls.append({"api": "TemporaryBRepManager.createBox", "returned": desc(b)})
        return b
    if k in ("cylinder", "cone"):
        a = spec["axis"]
        n = math.sqrt(sum(x * x for x in a))
        u = [x / n for x in a]
        base = spec["base"]
        top = [base[i] + u[i] * spec["h"] for i in range(3)]
        b = tbm.createCylinderOrCone(P(base), spec["r0"] * MM, P(top), spec["r1"] * MM)
        calls.append({"api": "TemporaryBRepManager.createCylinderOrCone", "pointOne_cm": [v * MM for v in base],
                      "pointOneRadius_cm": spec["r0"] * MM, "pointTwo_cm": [v * MM for v in top],
                      "pointTwoRadius_cm": spec["r1"] * MM, "returned": desc(b)})
        return b
    if k == "sphere":
        b = tbm.createSphere(P(spec["center"]), spec["r"] * MM)
        calls.append({"api": "TemporaryBRepManager.createSphere", "center_cm": [v * MM for v in spec["center"]],
                      "radius_cm": spec["r"] * MM, "returned": desc(b)})
        return b
    if k == "torus":
        a = spec["axis"]
        b = tbm.createTorus(P(spec["center"]), adsk.core.Vector3D.create(a[0], a[1], a[2]),
                            spec["major"] * MM, spec["minor"] * MM)
        calls.append({"api": "TemporaryBRepManager.createTorus", "center_cm": [v * MM for v in spec["center"]],
                      "axis": a, "majorRadius_cm": spec["major"] * MM, "minorRadius_cm": spec["minor"] * MM,
                      "returned": desc(b)})
        return b
    raise ValueError("unknown kind " + k)


def build(spec, calls):
    if spec["kind"] != "boolean":
        return build_primitive(spec, calls)
    target = build_primitive(spec["target"], calls)
    if target is None:
        raise RuntimeError("target creation returned None")
    for i, tool_spec in enumerate(spec["tools"]):
        tool = build_primitive(tool_spec, calls)
        rec = {"api": "TemporaryBRepManager.booleanOperation", "op": spec["op"], "booleanType": BOOL[spec["op"]],
               "tool_index": i}
        try:
            r = tbm.booleanOperation(target, tool, BOOL[spec["op"]])
            rec["returned"] = desc(r)
        except Exception as e:
            rec["error"] = err_text(e)
        try:
            rec["target_after"] = {"faces": target.faces.count, "edges": target.edges.count,
                                   "vertices": target.vertices.count, "lumps": target.lumps.count}
        except Exception as e:
            rec["target_after_error"] = err_text(e)
        calls.append(rec)
    return target


# --------------------------------------------------------------------------- measurement
def topology(body):
    t = {}
    call(t, "isSolid", lambda: body.isSolid)
    call(t, "isTemporary", lambda: body.isTemporary)
    call(t, "isTransient", lambda: body.isTransient)
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
    faces = []
    try:
        for f in body.faces:
            d = {}
            call(d, "tempId", lambda: f.tempId)
            call(d, "surface", lambda: SURF.get(f.geometry.surfaceType, "other:%d" % f.geometry.surfaceType))
            call(d, "loops", lambda: f.loops.count)
            call(d, "outer_flags", lambda: [l.isOuter for l in f.loops])
            call(d, "coedges_per_loop", lambda: [l.coEdges.count for l in f.loops])
            call(d, "edges", lambda: f.edges.count)
            call(d, "area_cm2", lambda: f.area)
            call(d, "isParamReversed", lambda: f.isParamReversed)
            faces.append(d)
    except Exception as e:
        faces.append({"error": err_text(e)})
    t["face_detail"] = faces
    edges = []
    try:
        for e in body.edges:
            d = {}
            call(d, "tempId", lambda: e.tempId)
            g = call(d, "geometry_present", lambda: e.geometry is not None)
            call(d, "curve", lambda: (CURVE.get(e.geometry.curveType, "other:%d" % e.geometry.curveType)
                                      if e.geometry is not None else "none"))
            call(d, "isDegenerate", lambda: e.isDegenerate)
            call(d, "startVertex", lambda: e.startVertex.tempId if e.startVertex else None)
            call(d, "endVertex", lambda: e.endVertex.tempId if e.endVertex else None)

            def _len():
                ev = e.evaluator
                ok, p0, p1 = ev.getParameterExtents()
                ok2, L = ev.getLengthAtParameter(p0, p1)
                return {"ok": ok and ok2, "p0": p0, "p1": p1, "length_cm": L}
            call(d, "length", _len)
            call(d, "coedge_faces", lambda: [c.loop.face.tempId for c in e.coEdges])
            call(d, "coedge_opposed", lambda: [c.isOpposedToEdge for c in e.coEdges])
            call(d, "isTolerant", lambda: e.isTolerant)
            call(d, "tolerance_cm", lambda: e.tolerance)
            edges.append(d)
    except Exception as ex:
        edges.append({"error": err_text(ex)})
    t["edge_detail"] = edges
    verts = []
    try:
        for v in body.vertices:
            d = {}
            call(d, "tempId", lambda: v.tempId)
            call(d, "geometry_cm", lambda: pt(v.geometry))
            call(d, "isTolerant", lambda: v.isTolerant)
            call(d, "tolerance_cm", lambda: v.tolerance)
            call(d, "edges", lambda: v.edges.count)
            verts.append(d)
    except Exception as ex:
        verts.append({"error": err_text(ex)})
    t["vertex_detail"] = verts
    return t


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
    call(r, "radius_of_gyration_cm", lambda: list(pp.getRadiusOfGyration()))
    call(r, "rotation_to_principal", lambda: list(pp.getRotationToPrincipal()))
    return r


def file_export(rec, body, case_id):
    out = {}
    for ext in ("smt", "sat", "smb", "sab", "step"):
        path = os.path.join(EXPORT, case_id + ".tbm." + ext)
        d = {}
        call(d, "exportToFile", lambda: tbm.exportToFile([body], path))
        d["exists"] = os.path.exists(path)
        d["bytes"] = os.path.getsize(path) if d["exists"] else 0
        out[ext] = d
    rec["exportToFile"] = out


def persist(rec, body, case_id, export_step):
    """Add the temporary body to a new (unsaved) design: base-feature path, Direct design as fallback."""
    out = {}
    for mode in ("base_feature", "direct"):
        doc = None
        att = {"mode": mode}
        try:
            doc = app.documents.add(adsk.core.DocumentTypes.FusionDesignDocumentType)
            d = adsk.fusion.Design.cast(app.activeProduct)
            root = d.rootComponent
            if mode == "base_feature":
                d.designType = adsk.fusion.DesignTypes.ParametricDesignType
                bf = root.features.baseFeatures.add()
                att["startEdit"] = bf.startEdit()
                nb = root.bRepBodies.add(body, bf)
                att["add_returned"] = desc(nb)
                att["finishEdit"] = bf.finishEdit()
                call(att, "baseFeature_health", lambda: bf.healthState)
                call(att, "baseFeature_message", lambda: bf.errorOrWarningMessage)
            else:
                d.designType = adsk.fusion.DesignTypes.DirectDesignType
                nb = root.bRepBodies.add(body)
                att["add_returned"] = desc(nb)
            att["bodies"] = root.bRepBodies.count
            att["body_detail"] = []
            for b in root.bRepBodies:
                bd = {}
                call(bd, "isSolid", lambda: b.isSolid)
                call(bd, "faces", lambda: b.faces.count)
                call(bd, "edges", lambda: b.edges.count)
                call(bd, "vertices", lambda: b.vertices.count)
                call(bd, "lumps", lambda: b.lumps.count)
                call(bd, "face_types", lambda: sorted(SURF.get(f.geometry.surfaceType, "other") for f in b.faces))
                bd["phys"] = phys(lambda: b.getPhysicalProperties(VERY_HIGH))
                att["body_detail"].append(bd)
            att["component_phys"] = phys(lambda: root.getPhysicalProperties(VERY_HIGH))
            if export_step and root.bRepBodies.count > 0:
                path = os.path.join(EXPORT, case_id + ".step")
                em = d.exportManager
                call(att, "step_export", lambda: em.execute(em.createSTEPExportOptions(path, root)))
                att["step_exists"] = os.path.exists(path)
            att["ok"] = root.bRepBodies.count > 0
        except Exception as e:
            att["ok"] = False
            att["error"] = err_text(e)
            att["trace"] = traceback.format_exc(limit=4)
        finally:
            if doc is not None:
                try:
                    doc.close(False)
                except Exception as e:
                    att["close_error"] = err_text(e)
        out[mode] = att
        if att.get("ok"):
            break
    rec["persist"] = out


def measure_case(case_id, spec, builder, export=True, do_persist=True, export_step=True):
    rec = {"id": case_id, "input": spec}
    t0 = time.time()
    calls = []
    body = None
    try:
        body = builder(calls)
        rec["build_ok"] = body is not None
        if body is None:
            rec["build_error"] = "API returned None (null) for the created body"
    except Exception as e:
        rec["build_ok"] = False
        rec["build_error"] = err_text(e)
        rec["build_trace"] = traceback.format_exc(limit=4)
    rec["calls"] = calls
    if body is not None:
        rec["topology"] = topology(body)
        rec["temp_phys"] = phys(lambda: body.getPhysicalProperties(VERY_HIGH))
        rec["temp_phys_property"] = phys(lambda: body.physicalProperties)
        if export:
            file_export(rec, body, case_id)
        if do_persist:
            persist(rec, body, case_id, export_step)
    rec["seconds"] = round(time.time() - t0, 3)
    log("case", **rec)
    return rec


# --------------------------------------------------------------------------- steps
def s_start():
    env = {"version": app.version, "python": sys.version, "documents": app.documents.count}
    call(env, "pointTolerance_cm", lambda: app.pointTolerance)
    call(env, "vectorAngleTolerance_rad", lambda: app.vectorAngleTolerance)
    log("environment", **env)
    sigs = {}
    for cls, names in [(adsk.fusion.TemporaryBRepManager, ["createBox", "createCylinderOrCone", "createSphere",
                                                            "createTorus", "booleanOperation", "exportToFile"]),
                       (adsk.core.OrientedBoundingBox3D, ["create"]),
                       (adsk.fusion.BRepBody, ["getPhysicalProperties"]),
                       (adsk.fusion.PhysicalProperties, ["getXYZMomentsOfInertia"]),
                       (adsk.fusion.BRepBodies, ["add"])]:
        for n in names:
            fn = getattr(cls, n, None)
            k = cls.__name__ + "." + n
            try:
                import inspect
                s = str(inspect.signature(fn))
            except Exception as e:
                s = "inspect.signature failed: " + str(e)
            sigs[k] = {"present": fn is not None, "signature": s, "doc": getattr(fn, "__doc__", None)}
    log("api_signatures", signatures=sigs)


def make_case_step(c):
    def step():
        measure_case(c["id"], c, lambda calls: build(c, calls))
    step.__name__ = "case_" + c["id"]
    return step


def extra_box(eid, desc_text, size_mm, persist_too=True):
    spec = {"kind": "box", "corner": [0, 0, 0], "size": size_mm}

    def builder(calls):
        rec = {"api": "OrientedBoundingBox3D.create", "size_cm": [v * MM for v in size_mm]}
        c = [v / 2.0 for v in size_mm]
        try:
            obb = adsk.core.OrientedBoundingBox3D.create(P(c), adsk.core.Vector3D.create(1, 0, 0),
                                                         adsk.core.Vector3D.create(0, 1, 0),
                                                         size_mm[0] * MM, size_mm[1] * MM, size_mm[2] * MM)
            rec["returned"] = desc(obb)
            try:
                rec["obb_dims_cm"] = [obb.length, obb.width, obb.height]
            except Exception as e:
                rec["obb_dims_error"] = err_text(e)
        except Exception as e:
            rec["error"] = err_text(e)
            calls.append(rec)
            raise
        calls.append(rec)
        if obb is None:
            return None
        r2 = {"api": "TemporaryBRepManager.createBox"}
        try:
            b = tbm.createBox(obb)
            r2["returned"] = desc(b)
        except Exception as e:
            r2["error"] = err_text(e)
            calls.append(r2)
            raise
        calls.append(r2)
        return b

    def step():
        r = measure_case(eid, spec, builder, export=True, do_persist=persist_too, export_step=False)
        log("extra", id=eid, description=desc_text, case_ref=eid)
    step.__name__ = "extra_" + eid
    return step


def extra_spec(eid, desc_text, spec):
    def step():
        measure_case(eid, spec, lambda calls: build(spec, calls), export=True, do_persist=True, export_step=False)
        log("extra", id=eid, description=desc_text, case_ref=eid)
    step.__name__ = "extra_" + eid
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
    steps = [s_start]
    for c in cat["cases"]:
        steps.append(make_case_step(c))
    # (a) degenerate inputs
    steps.append(extra_box("extra.box.zero_height", "createBox with a zero dimension (10 x 10 x 0 mm)", [10, 10, 0]))
    steps.append(extra_box("extra.box.negative_height", "createBox with a negative dimension (10 x 10 x -5 mm)",
                           [10, 10, -5]))
    steps.append(extra_spec("extra.sphere.zero_radius", "createSphere with r = 0",
                            {"kind": "sphere", "center": [0, 0, 0], "r": 0}))
    # (b) self-intersecting torus
    steps.append(extra_spec("extra.torus.minor_gt_major", "torus with minor (20 mm) > major (5 mm): spindle torus",
                            {"kind": "torus", "center": [0, 0, 0], "axis": [0, 0, 1], "major": 5, "minor": 20}))
    steps.append(extra_spec("extra.torus.minor_eq_major", "torus with minor == major (10 mm): horn torus",
                            {"kind": "torus", "center": [0, 0, 0], "axis": [0, 0, 1], "major": 10, "minor": 10}))
    # (c) size sweep, smallest first, the huge ones last (most likely to misbehave)
    for e in (1e-6, 1e-5, 1e-4, 1e-3, 1e6, 1e7):
        steps.append(extra_box("extra.size.cube_%gmm" % e, "size sweep: cube of edge %g mm" % e, [e, e, e]))
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
