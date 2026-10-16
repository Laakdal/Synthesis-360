"""CAD360 investigation probe for Autodesk Fusion.

Runs a scripted, reproducible observation of Fusion's public API and UI:
  * dumps the ribbon / toolbar / palette hierarchy,
  * performs a sketch -> constrain -> extrude -> edit -> fillet -> cut workflow,
  * records geometry measurements, timeline state and health/error messages,
  * captures viewport images and full-window screenshots at each state,
  * exports STEP / STL to the evidence folder.

Safety: it does nothing unless OUT/RUN_PROBE exists (deleted on start), never saves
documents to the cloud, and closes every document it opens without saving.
Fusion's API uses centimetres internally; values are logged raw (cm) and in mm.
"""
import adsk.core, adsk.fusion, traceback, json, os, threading, time, ctypes, ctypes.wintypes as wt

OUT = r"C:\Mudrock's Headquarters\Project\CAD360\docs\evidence\fusion-probe\out"
SENTINEL = os.path.join(OUT, "RUN_PROBE")
EVENT_ID = "CAD360ProbeStepEvent"

app = None
ui = None
handlers = []
stop_flag = threading.Event()
step_done = threading.Event()
state = {}


# --------------------------------------------------------------------------- logging
def log(kind, **data):
    rec = {"t": time.strftime("%Y-%m-%dT%H:%M:%S"), "kind": kind}
    rec.update(data)
    with open(os.path.join(OUT, "probe-log.jsonl"), "a", encoding="utf-8") as f:
        f.write(json.dumps(rec, default=str) + "\n")


def write_json(name, obj):
    with open(os.path.join(OUT, name), "w", encoding="utf-8") as f:
        json.dump(obj, f, indent=1, default=str)


# --------------------------------------------------------------------------- window capture (GDI PrintWindow)
user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32
user32.GetWindowRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
user32.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.POINTER(wt.DWORD)]
user32.IsWindowVisible.argtypes = [wt.HWND]
user32.GetWindowDC.argtypes = [wt.HWND]
user32.GetWindowDC.restype = wt.HDC
user32.ReleaseDC.argtypes = [wt.HWND, wt.HDC]
user32.PrintWindow.argtypes = [wt.HWND, wt.HDC, wt.UINT]
user32.ShowWindow.argtypes = [wt.HWND, ctypes.c_int]
user32.GetWindowTextW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
gdi32.CreateCompatibleDC.argtypes = [wt.HDC]
gdi32.CreateCompatibleDC.restype = wt.HDC
gdi32.CreateCompatibleBitmap.argtypes = [wt.HDC, ctypes.c_int, ctypes.c_int]
gdi32.CreateCompatibleBitmap.restype = wt.HBITMAP
gdi32.SelectObject.argtypes = [wt.HDC, wt.HGDIOBJ]
gdi32.SelectObject.restype = wt.HGDIOBJ
gdi32.DeleteObject.argtypes = [wt.HGDIOBJ]
gdi32.DeleteDC.argtypes = [wt.HDC]
gdi32.GetDIBits.argtypes = [wt.HDC, wt.HBITMAP, wt.UINT, wt.UINT, ctypes.c_void_p, ctypes.c_void_p, wt.UINT]


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [("biSize", wt.DWORD), ("biWidth", wt.LONG), ("biHeight", wt.LONG), ("biPlanes", wt.WORD),
                ("biBitCount", wt.WORD), ("biCompression", wt.DWORD), ("biSizeImage", wt.DWORD),
                ("biXPelsPerMeter", wt.LONG), ("biYPelsPerMeter", wt.LONG), ("biClrUsed", wt.DWORD),
                ("biClrImportant", wt.DWORD)]


def main_hwnd():
    pid = os.getpid()
    best = [None, 0]
    proto = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)

    def cb(h, _):
        p = wt.DWORD()
        user32.GetWindowThreadProcessId(h, ctypes.byref(p))
        if p.value == pid and user32.IsWindowVisible(h):
            r = wt.RECT()
            user32.GetWindowRect(h, ctypes.byref(r))
            a = (r.right - r.left) * (r.bottom - r.top)
            if a > best[1]:
                best[0], best[1] = h, a
        return True

    user32.EnumWindows(proto(cb), 0)
    return best[0]


def capture_window(name):
    h = main_hwnd()
    if not h:
        log("capture_failed", name=name, reason="no window")
        return
    r = wt.RECT()
    user32.GetWindowRect(h, ctypes.byref(r))
    w, ht = r.right - r.left, r.bottom - r.top
    wdc = user32.GetWindowDC(h)
    mdc = gdi32.CreateCompatibleDC(wdc)
    bmp = gdi32.CreateCompatibleBitmap(wdc, w, ht)
    old = gdi32.SelectObject(mdc, bmp)
    ok = user32.PrintWindow(h, mdc, 2)  # PW_RENDERFULLCONTENT
    bih = BITMAPINFOHEADER()
    bih.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    bih.biWidth, bih.biHeight, bih.biPlanes, bih.biBitCount, bih.biCompression = w, ht, 1, 32, 0
    buf = ctypes.create_string_buffer(w * ht * 4)
    gdi32.GetDIBits(mdc, bmp, 0, ht, buf, ctypes.byref(bih), 0)
    gdi32.SelectObject(mdc, old)
    gdi32.DeleteObject(bmp)
    gdi32.DeleteDC(mdc)
    user32.ReleaseDC(h, wdc)
    size = 14 + 40 + len(buf.raw)
    header = b"BM" + size.to_bytes(4, "little") + b"\0\0\0\0" + (54).to_bytes(4, "little")
    with open(os.path.join(OUT, "window-" + name + ".bmp"), "wb") as f:
        f.write(header + bytes(bih) + buf.raw)
    log("window_capture", name=name, width=w, height=ht, printwindow_ok=bool(ok))


user32.GetDC.argtypes = [wt.HWND]
user32.GetDC.restype = wt.HDC
user32.SetForegroundWindow.argtypes = [wt.HWND]
gdi32.BitBlt.argtypes = [wt.HDC, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, wt.HDC, ctypes.c_int,
                         ctypes.c_int, wt.DWORD]


def capture_screen_rect(name):
    """Copy the on-screen pixels of the main window rect (includes floating Qt palettes and dialogs)."""
    h = main_hwnd()
    if not h:
        return
    user32.SetForegroundWindow(h)
    r = wt.RECT()
    user32.GetWindowRect(h, ctypes.byref(r))
    x, y, w, ht = r.left, r.top, r.right - r.left, r.bottom - r.top
    sdc = user32.GetDC(None)
    mdc = gdi32.CreateCompatibleDC(sdc)
    bmp = gdi32.CreateCompatibleBitmap(sdc, w, ht)
    old = gdi32.SelectObject(mdc, bmp)
    gdi32.BitBlt(mdc, 0, 0, w, ht, sdc, x, y, 0x00CC0020 | 0x40000000)  # SRCCOPY | CAPTUREBLT
    bih = BITMAPINFOHEADER()
    bih.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    bih.biWidth, bih.biHeight, bih.biPlanes, bih.biBitCount, bih.biCompression = w, ht, 1, 32, 0
    buf = ctypes.create_string_buffer(w * ht * 4)
    gdi32.GetDIBits(mdc, bmp, 0, ht, buf, ctypes.byref(bih), 0)
    gdi32.SelectObject(mdc, old)
    gdi32.DeleteObject(bmp)
    gdi32.DeleteDC(mdc)
    user32.ReleaseDC(None, sdc)
    size = 14 + 40 + len(buf.raw)
    header = b"BM" + size.to_bytes(4, "little") + b"\0\0\0\0" + (54).to_bytes(4, "little")
    with open(os.path.join(OUT, "screen-" + name + ".bmp"), "wb") as f:
        f.write(header + bytes(bih) + buf.raw)
    log("screen_capture", name=name, rect=[x, y, w, ht])


def shot(name):
    try:
        vp = app.activeViewport
        if vp:
            vp.saveAsImageFile(os.path.join(OUT, "viewport-" + name + ".png"), 1280, 800)
    except Exception:
        log("viewport_capture_failed", name=name, err=traceback.format_exc())
    capture_window(name)
    if state.get("screen_capture"):
        capture_screen_rect(name)


# --------------------------------------------------------------------------- synthesized mouse input
user32.SetCursorPos.argtypes = [ctypes.c_int, ctypes.c_int]
user32.mouse_event.argtypes = [wt.DWORD, wt.DWORD, wt.DWORD, wt.DWORD, ctypes.c_void_p]


def model_to_screen(p3):
    vp = app.activeViewport
    v = vp.modelToViewSpace(p3)
    s = vp.viewToScreen(v)
    return int(round(s.x)), int(round(s.y))


def click_model_point(p3, label):
    x, y = model_to_screen(p3)
    user32.SetForegroundWindow(main_hwnd())
    user32.SetCursorPos(x, y)
    time.sleep(0.15)
    user32.SetCursorPos(x + 1, y)
    time.sleep(0.1)
    user32.SetCursorPos(x, y)
    user32.mouse_event(0x0002, 0, 0, 0, None)  # LEFTDOWN
    time.sleep(0.05)
    user32.mouse_event(0x0004, 0, 0, 0, None)  # LEFTUP
    log("synthetic_click", label=label, model=[p3.x, p3.y, p3.z], screen=[x, y])


# --------------------------------------------------------------------------- UI hierarchy dump
def dump_controls(controls):
    out = []
    for c in controls:
        d = {"id": c.id, "type": c.objectType.split("::")[-1], "isVisible": c.isVisible}
        try:
            d["isPromoted"] = c.isPromoted
        except Exception:
            pass
        cc = adsk.core.CommandControl.cast(c)
        dd = adsk.core.DropDownControl.cast(c)
        if cc:
            try:
                cd = cc.commandDefinition
                d.update(command=cd.id, name=cd.name, tooltip=cd.tooltip)
            except Exception:
                pass
        elif dd:
            d["name"] = safe(lambda: dd.name, "")
            d["children"] = safe(lambda: dump_controls(dd.controls), [])
        out.append(d)
    return out


def safe(fn, default=None):
    try:
        return fn()
    except Exception:
        return default


def dump_ui(tag):
    data = {"activeWorkspace": safe(lambda: ui.activeWorkspace.id), "workspaces": [],
            "toolbars": [], "palettes": []}
    for i in range(ui.workspaces.count):
        ws = safe(lambda: ui.workspaces.item(i))
        if ws is None:
            continue
        wd = {"id": safe(lambda: ws.id), "name": safe(lambda: ws.name), "productType": safe(lambda: ws.productType),
              "isActive": safe(lambda: ws.isActive)}
        if wd["id"] == "FusionSolidEnvironment" or wd["isActive"]:
            wd["tabs"] = []
            try:
                for tab in ws.toolbarTabs:
                    td = {"id": tab.id, "name": tab.name, "isVisible": tab.isVisible, "isActive": tab.isActive,
                          "panels": []}
                    for p in tab.toolbarPanels:
                        td["panels"].append({"id": p.id, "name": p.name, "isVisible": p.isVisible,
                                             "controls": dump_controls(p.controls)})
                    wd["tabs"].append(td)
            except Exception:
                wd["tabs_error"] = traceback.format_exc()
        data["workspaces"].append(wd)
    for tb in ui.toolbars:
        try:
            data["toolbars"].append({"id": tb.id, "controls": dump_controls(tb.controls)})
        except Exception:
            data["toolbars"].append({"id": tb.id, "error": traceback.format_exc()})
    for p in ui.palettes:
        try:
            data["palettes"].append({"id": p.id, "name": p.name, "isVisible": p.isVisible,
                                     "dockingState": p.dockingState})
        except Exception:
            pass
    write_json("ui-" + tag + ".json", data)
    log("ui_dump", tag=tag, workspaces=len(data["workspaces"]))


def dump_command_definitions():
    rows = []
    for cd in ui.commandDefinitions:
        try:
            rows.append({"id": cd.id, "name": cd.name, "tooltip": cd.tooltip})
        except Exception:
            pass
    write_json("command-definitions.json", rows)
    state["cmd_by_name"] = {}
    for r in rows:
        state["cmd_by_name"].setdefault(r["name"], []).append(r["id"])
    log("command_definitions", count=len(rows))


def find_cmd(*candidates):
    for c in candidates:
        if ui.commandDefinitions.itemById(c):
            return c
    for c in candidates:
        ids = state.get("cmd_by_name", {}).get(c)
        if ids:
            return ids[0]
    return None


def run_cmd(*candidates, preselect=None):
    cid = find_cmd(*candidates)
    if not cid:
        log("command_not_found", candidates=candidates)
        return None
    if preselect is not None:
        ui.activeSelections.clear()
        for e in (preselect if isinstance(preselect, list) else [preselect]):
            ui.activeSelections.add(e)
    ui.commandDefinitions.itemById(cid).execute()
    log("command_executed", id=cid, preselected=preselect is not None)
    return cid


def end_cmd():
    try:
        ui.terminateActiveCommand()
    except Exception:
        log("terminate_failed", err=traceback.format_exc())
    ui.activeSelections.clear()


# --------------------------------------------------------------------------- model measurements
def design():
    return adsk.fusion.Design.cast(app.activeProduct)


def body_info(b):
    bb = b.boundingBox
    pp = b.physicalProperties
    return {"name": b.name, "isSolid": b.isSolid, "faces": b.faces.count, "edges": b.edges.count,
            "vertices": b.vertices.count, "volume_cm3": pp.volume, "volume_mm3": pp.volume * 1000.0,
            "area_cm2": pp.area,
            "bbox_mm": [[round(v * 10, 4) for v in (bb.minPoint.x, bb.minPoint.y, bb.minPoint.z)],
                        [round(v * 10, 4) for v in (bb.maxPoint.x, bb.maxPoint.y, bb.maxPoint.z)]]}


HEALTH = {0: "Healthy", 1: "Warning", 2: "Error", 3: "Suppressed", 4: "RolledBack", 5: "Unknown"}


def timeline_info():
    tl = design().timeline
    items = []
    for i in range(tl.count):
        t = tl.item(i)
        d = {"index": t.index, "name": t.name, "isSuppressed": t.isSuppressed, "isRolledBack": t.isRolledBack,
             "isGroup": t.isGroup}
        try:
            ent = t.entity
            d["entityType"] = ent.objectType.split("::")[-1]
            d["health"] = HEALTH.get(t.healthState, t.healthState)
            d["message"] = t.errorOrWarningMessage
        except Exception:
            pass
        items.append(d)
    return {"count": tl.count, "markerPosition": tl.markerPosition, "items": items}


def params_info():
    rows = []
    for p in design().allParameters:
        rows.append({"name": p.name, "expression": p.expression, "value": p.value, "unit": p.unit,
                     "type": p.objectType.split("::")[-1],
                     "createdBy": (p.createdBy.objectType.split("::")[-1] if getattr(p, "createdBy", None) else None)
                     if adsk.fusion.ModelParameter.cast(p) else "user"})
    return rows


def model_snapshot(label):
    root = design().rootComponent
    snap = {"label": label, "bodies": [body_info(b) for b in root.bRepBodies], "timeline": timeline_info(),
            "parameters": params_info()}
    state.setdefault("snapshots", []).append(snap)
    log("snapshot", **snap)
    return snap


def sketch_info(sk):
    cons = {}
    for c in sk.geometricConstraints:
        k = c.objectType.split("::")[-1]
        cons[k] = cons.get(k, 0) + 1
    dims = []
    for d in sk.sketchDimensions:
        try:
            dims.append({"type": d.objectType.split("::")[-1], "param": d.parameter.name,
                         "expression": d.parameter.expression, "isDriving": d.isDriving})
        except Exception:
            dims.append({"type": d.objectType.split("::")[-1]})
    return {"name": sk.name, "curves": sk.sketchCurves.count, "points": sk.sketchPoints.count,
            "profiles": sk.profiles.count, "isFullyConstrained": sk.isFullyConstrained,
            "constraints": cons, "dimensions": dims,
            "health": HEALTH.get(sk.healthState, sk.healthState), "message": sk.errorOrWarningMessage}


def attempt(label, fn):
    try:
        r = fn()
        log("attempt_ok", label=label, result=r)
        return r
    except Exception as e:
        log("attempt_error", label=label, error=str(e))
        return None


# --------------------------------------------------------------------------- probe steps
def s_start():
    h = main_hwnd()
    if h:
        user32.ShowWindow(h, 3)  # maximize
    log("environment", version=app.version, user=bool(app.currentUser), os=os.name,
        documents=app.documents.count)
    shot("00_startup")


def s_dump_ui_initial():
    dump_command_definitions()
    dump_ui("initial")


def s_new_doc():
    doc = app.documents.add(adsk.core.DocumentTypes.FusionDesignDocumentType)
    state["doc"] = doc
    d = design()
    d.designType = adsk.fusion.DesignTypes.ParametricDesignType
    log("new_document", name=doc.name, designType=d.designType,
        defaultLengthUnits=d.unitsManager.defaultLengthUnits, workspace=ui.activeWorkspace.id)
    app.activeViewport.fit()


def s_shot_new():
    dump_ui("design-workspace")
    shot("01_new_design")


def s_sketch_prompt():
    run_cmd("SketchCreate", "Create Sketch")


def s_shot_sketch_prompt():
    shot("02_create_sketch_select_plane")
    end_cmd()


def s_sketch_mode_ui():
    run_cmd("SketchCreate", "Create Sketch", preselect=design().rootComponent.xYConstructionPlane)


def s_shot_sketch_mode():
    shot("03_sketch_mode")
    dump_ui("sketch-mode")
    log("sketch_mode_state", activeEditObject=app.activeEditObject.objectType if app.activeEditObject else None)


def s_finish_sketch_ui():
    cid = find_cmd("SketchStop", "FinishSketch")
    if cid:
        ui.commandDefinitions.itemById(cid).execute()
        log("command_executed", id=cid)
    else:
        end_cmd()


def s_sketch_api():
    d = design()
    root = d.rootComponent
    # delete any sketch left by the UI step so the workflow starts clean
    for i in range(root.sketches.count - 1, -1, -1):
        root.sketches.item(i).deleteMe()
    sk = root.sketches.add(root.xYConstructionPlane)
    state["sk"] = sk
    lines = sk.sketchCurves.sketchLines
    rect = lines.addTwoPointRectangle(adsk.core.Point3D.create(1, 1, 0), adsk.core.Point3D.create(6, 4, 0))
    state["rect"] = rect
    log("sketch_after_rectangle", **sketch_info(sk))
    dims = sk.sketchDimensions
    H, V = adsk.fusion.DimensionOrientations.HorizontalDimensionOrientation, \
        adsk.fusion.DimensionOrientations.VerticalDimensionOrientation
    dw = dims.addDistanceDimension(rect[0].startSketchPoint, rect[0].endSketchPoint, H,
                                   adsk.core.Point3D.create(3.5, 0.2, 0))
    dh = dims.addDistanceDimension(rect[1].startSketchPoint, rect[1].endSketchPoint, V,
                                   adsk.core.Point3D.create(7, 2.5, 0))
    dw.parameter.expression = "50 mm"
    dh.parameter.expression = "30 mm"
    state["dw"], state["dh"] = dw, dh
    log("sketch_after_dimensions", **sketch_info(sk))
    # fix the corner to the origin -> should become fully constrained
    sk.geometricConstraints.addCoincident(rect[0].startSketchPoint, sk.originPoint)
    log("sketch_after_origin_coincident", **sketch_info(sk),
        corner_mm=[rect[0].startSketchPoint.geometry.x * 10, rect[0].startSketchPoint.geometry.y * 10])
    # over-constraint attempt: a third driving dimension on the opposite side
    attempt("overconstrain_driving_dimension", lambda: dims.addDistanceDimension(
        rect[2].startSketchPoint, rect[2].endSketchPoint, H, adsk.core.Point3D.create(3.5, 4.5, 0)).parameter.name)
    attempt("overconstrain_extra_horizontal", lambda: sk.geometricConstraints.addHorizontal(rect[0]).objectType)
    log("sketch_after_overconstraint_attempts", **sketch_info(sk))
    app.activeViewport.fit()


def s_shot_sketch():
    shot("04_sketch_rectangle_dimensioned")


def s_extrude_dialog():
    prof = state["sk"].profiles.item(0)
    run_cmd("Extrude", preselect=prof)


def s_shot_extrude_dialog():
    shot("05_extrude_dialog")
    end_cmd()


def s_extrude():
    root = design().rootComponent
    prof = state["sk"].profiles.item(0)
    attempt("extrude_zero_distance", lambda: root.features.extrudeFeatures.addSimple(
        prof, adsk.core.ValueInput.createByString("0 mm"),
        adsk.fusion.FeatureOperations.NewBodyFeatureOperation).name)
    ext = root.features.extrudeFeatures.addSimple(prof, adsk.core.ValueInput.createByString("10 mm"),
                                                  adsk.fusion.FeatureOperations.NewBodyFeatureOperation)
    state["ext"] = ext
    log("extrude", name=ext.name, distanceParam=adsk.fusion.DistanceExtentDefinition.cast(ext.extentOne).distance.name)
    model_snapshot("after_extrude_10mm")
    app.activeViewport.fit()


def s_shot_extrude():
    shot("06_extruded_box")


def s_edit_extrude():
    dist = adsk.fusion.DistanceExtentDefinition.cast(state["ext"].extentOne).distance
    dist.expression = "20 mm"
    model_snapshot("after_extrude_distance_20mm")


def s_edit_sketch_dim():
    state["dw"].parameter.expression = "60 mm"
    log("sketch_after_width_edit", **sketch_info(state["sk"]))
    model_snapshot("after_sketch_width_60mm")
    app.activeViewport.fit()


def s_shot_edited():
    shot("07_after_parameter_edits")


def vertical_edges(body):
    out = []
    for e in body.edges:
        g = e.geometry
        ln = adsk.core.Line3D.cast(g)
        if ln and abs(ln.startPoint.x - ln.endPoint.x) < 1e-6 and abs(ln.startPoint.y - ln.endPoint.y) < 1e-6:
            out.append(e)
    return out


def s_fillet():
    root = design().rootComponent
    body = root.bRepBodies.item(0)
    edges = adsk.core.ObjectCollection.create()
    for e in vertical_edges(body):
        edges.add(e)
    fi = root.features.filletFeatures.createInput()
    fi.edgeSetInputs.addConstantRadiusEdgeSet(edges, adsk.core.ValueInput.createByString("5 mm"), True)
    fil = root.features.filletFeatures.add(fi)
    state["fil"] = fil
    log("fillet", name=fil.name, edges=edges.count)
    model_snapshot("after_fillet_4_vertical_edges_5mm")


def s_fillet_dialog():
    root = design().rootComponent
    body = root.bRepBodies.item(0)
    top = max(body.faces, key=lambda f: f.pointOnFace.z)
    run_cmd("FusionFilletEdgeCommand", "Fillet", preselect=top.edges.item(0))


def s_shot_fillet_dialog():
    shot("08_fillet_dialog")
    end_cmd()


def s_upstream_edit_with_fillet():
    state["dw"].parameter.expression = "70 mm"
    model_snapshot("after_width_70mm_with_fillet_downstream")
    state["fil_radius_param"] = None
    try:
        es = state["fil"].edgeSets.item(0)
        rp = adsk.fusion.ConstantRadiusFilletEdgeSet.cast(es).radius
        state["fil_radius_param"] = rp
        rp.expression = "40 mm"  # larger than half the 30 mm depth -> expected failure
        model_snapshot("after_fillet_radius_40mm_invalid")
    except Exception as e:
        log("fillet_radius_edit_error", error=str(e))
    app.activeViewport.fit()


def s_shot_invalid():
    shot("09_invalid_fillet_state")


def s_restore_and_cut():
    if state.get("fil_radius_param"):
        state["fil_radius_param"].expression = "5 mm"
    model_snapshot("after_fillet_radius_restored")
    root = design().rootComponent
    body = root.bRepBodies.item(0)
    top = max([f for f in body.faces if adsk.core.Plane.cast(f.geometry)], key=lambda f: f.pointOnFace.z)
    sk2 = root.sketches.add(top)
    state["sk2"] = sk2
    c = sk2.modelToSketchSpace(adsk.core.Point3D.create(3.5, 1.5, 2.0))
    circ = sk2.sketchCurves.sketchCircles.addByCenterRadius(c, 0.6)
    sk2.sketchDimensions.addDiameterDimension(circ, adsk.core.Point3D.create(c.x + 1, c.y + 1, 0))
    log("sketch_on_face", **sketch_info(sk2))
    prof = sk2.profiles.item(0)
    for i in range(sk2.profiles.count):
        p = sk2.profiles.item(i)
        if p.profileLoops.count == 1 and p.areaProperties().area < 2:
            prof = p
    ei = root.features.extrudeFeatures.createInput(prof, adsk.fusion.FeatureOperations.CutFeatureOperation)
    ei.setOneSideExtent(adsk.fusion.ThroughAllExtentDefinition.create(),
                        adsk.fusion.ExtentDirections.NegativeExtentDirection)
    cut = root.features.extrudeFeatures.add(ei)
    state["cut"] = cut
    log("cut", name=cut.name)
    model_snapshot("after_through_all_cut_12mm_hole")


def s_chamfer():
    root = design().rootComponent
    body = root.bRepBodies.item(0)
    bottom = min([f for f in body.faces if adsk.core.Plane.cast(f.geometry)], key=lambda f: f.pointOnFace.z)
    edges = adsk.core.ObjectCollection.create()
    edges.add(bottom.edges.item(0))
    ci = root.features.chamferFeatures.createInput2()
    ci.chamferEdgeSets.addEqualDistanceChamferEdgeSet(edges, adsk.core.ValueInput.createByString("2 mm"), True)
    ch = root.features.chamferFeatures.add(ci)
    log("chamfer", name=ch.name)
    model_snapshot("after_chamfer_2mm")
    app.activeViewport.fit()


def s_shot_cut():
    shot("10_cut_and_chamfer")


def s_timeline_ops():
    tl = design().timeline
    # roll the marker back to just before the cut
    cut_tl = state["cut"].timelineObject
    cut_tl.rollTo(True)
    model_snapshot("timeline_rolled_before_cut")
    shot("11_timeline_rolled_back")
    tl.moveToEnd()
    fil_tl = state["fil"].timelineObject
    fil_tl.isSuppressed = True
    model_snapshot("fillet_suppressed")
    fil_tl.isSuppressed = False
    model_snapshot("fillet_unsuppressed")


def s_revolve():
    root = design().rootComponent
    sk = root.sketches.add(root.xZConstructionPlane)
    lines = sk.sketchCurves.sketchLines
    axis = lines.addByTwoPoints(adsk.core.Point3D.create(-3, 0, 0), adsk.core.Point3D.create(-3, -3, 0))
    axis.isConstruction = True
    lines.addTwoPointRectangle(adsk.core.Point3D.create(-2.5, 0, 0), adsk.core.Point3D.create(-1.5, -2, 0))
    prof = sk.profiles.item(0)
    ri = root.features.revolveFeatures.createInput(prof, axis, adsk.fusion.FeatureOperations.NewBodyFeatureOperation)
    ri.setAngleExtent(False, adsk.core.ValueInput.createByString("360 deg"))
    rev = root.features.revolveFeatures.add(ri)
    log("revolve", name=rev.name)
    model_snapshot("after_revolve_ring")
    app.activeViewport.fit()


def s_shot_revolve():
    shot("12_revolve")


def s_views():
    vp = app.activeViewport
    for name, o in [("top", adsk.core.ViewOrientations.TopViewOrientation),
                    ("front", adsk.core.ViewOrientations.FrontViewOrientation),
                    ("right", adsk.core.ViewOrientations.RightViewOrientation),
                    ("iso", adsk.core.ViewOrientations.IsoTopRightViewOrientation)]:
        cam = vp.camera
        cam.viewOrientation = o
        cam.isFitView = True
        vp.camera = cam
        adsk.doEvents()
        vp.refresh()
        cam2 = vp.camera
        log("camera", view=name, type=cam2.cameraType, eye=[cam2.eye.x, cam2.eye.y, cam2.eye.z],
            target=[cam2.target.x, cam2.target.y, cam2.target.z],
            up=[cam2.upVector.x, cam2.upVector.y, cam2.upVector.z])
        vp.saveAsImageFile(os.path.join(OUT, "viewport-13_view_" + name + ".png"), 1024, 700)


def s_export():
    d = design()
    em = d.exportManager
    res = {}
    for kind, fn in [("step", lambda: em.createSTEPExportOptions(os.path.join(OUT, "probe-model.step"), d.rootComponent)),
                     ("stl", lambda: em.createSTLExportOptions(d.rootComponent, os.path.join(OUT, "probe-model.stl"))),
                     ("f3d", lambda: em.createFusionArchiveExportOptions(os.path.join(OUT, "probe-model.f3d"),
                                                                        d.rootComponent))]:
        try:
            res[kind] = em.execute(fn())
        except Exception as e:
            res[kind] = "error: " + str(e)
    log("export", **res)


def s_undo():
    before = design().timeline.count
    cid = run_cmd("UndoCommand", "Undo")
    state["undo_before"] = before
    log("undo_issued", command=cid, timeline_before=before)


def s_undo_check():
    after = design().timeline.count
    log("undo_result", timeline_before=state.get("undo_before"), timeline_after=after,
        timeline=timeline_info())
    shot("14_after_undo")
    cid = run_cmd("RedoCommand", "Redo")
    log("redo_issued", command=cid)


def s_redo_check():
    log("redo_result", timeline_after=design().timeline.count)
    write_json("snapshots.json", state.get("snapshots", []))


def s_close():
    for i in range(app.documents.count - 1, -1, -1):
        doc = app.documents.item(i)
        if doc.isModified or doc == state.get("doc"):
            try:
                doc.close(False)
            except Exception:
                log("close_failed", err=traceback.format_exc())
    log("done")
    with open(os.path.join(OUT, "DONE"), "w") as f:
        f.write("done")


# =========================================================================== v2 pass
P = adsk.core.Point3D.create


def v2_start():
    state["screen_capture"] = True
    s_start()
    dump_command_definitions()
    dump_ui("v2-startup")


def v2_new_doc():
    s_new_doc()


def v2_shot_new():
    dump_ui("v2-design-workspace")
    shot("v2_01_new_design")


def v2_enter_sketch():
    run_cmd("SketchCreate", preselect=design().rootComponent.xYConstructionPlane)


def v2_rect_tool():
    shot("v2_02_sketch_mode_with_palette")
    dump_ui("v2-sketch-mode")
    state["ui_sketch"] = adsk.fusion.Sketch.cast(app.activeEditObject)
    run_cmd("ShapeRectangleTwoPoint")


def v2_rect_click1():
    click_model_point(P(-4, -2, 0), "rect_corner_1")


def v2_rect_click2():
    click_model_point(P(3, 2.5, 0), "rect_corner_2")


def v2_rect_done():
    shot("v2_03_ui_rectangle_drawn")
    end_cmd()
    sk = state.get("ui_sketch")
    if sk:
        log("ui_rectangle_sketch", **sketch_info(sk))


def v2_line_tool():
    run_cmd("DrawPolyline")


def v2_line_click1():
    click_model_point(P(-4, 4, 0), "line_start")


def v2_line_click2():
    # deliberately ~1 degree off horizontal to observe inference / auto-horizontal snapping
    click_model_point(P(3, 4.12, 0), "line_end")


def v2_line_done():
    end_cmd()
    sk = state.get("ui_sketch")
    if sk:
        info = sketch_info(sk)
        lines = []
        for ln in sk.sketchCurves.sketchLines:
            a, b = ln.startSketchPoint.geometry, ln.endSketchPoint.geometry
            lines.append([round(a.x * 10, 3), round(a.y * 10, 3), round(b.x * 10, 3), round(b.y * 10, 3)])
        log("ui_line_sketch", lines_mm=lines, **info)
    shot("v2_04_ui_line_drawn")


def v2_finish_ui_sketch():
    cid = find_cmd("SketchStop")
    if cid:
        ui.commandDefinitions.itemById(cid).execute()


def v2_constrained_rect():
    d = design()
    root = d.rootComponent
    for i in range(root.sketches.count - 1, -1, -1):
        root.sketches.item(i).deleteMe()
    sk = root.sketches.add(root.xYConstructionPlane)
    state["sk"] = sk
    rect = sk.sketchCurves.sketchLines.addTwoPointRectangle(P(0, 0, 0), P(5, 3, 0))
    gc = sk.geometricConstraints
    gc.addHorizontal(rect[0])
    gc.addHorizontal(rect[2])
    gc.addVertical(rect[1])
    gc.addVertical(rect[3])
    log("v2_sketch_hv", **sketch_info(sk))
    gc.addCoincident(rect[0].startSketchPoint, sk.originPoint)
    H = adsk.fusion.DimensionOrientations.HorizontalDimensionOrientation
    V = adsk.fusion.DimensionOrientations.VerticalDimensionOrientation
    dw = sk.sketchDimensions.addDistanceDimension(rect[0].startSketchPoint, rect[0].endSketchPoint, H, P(2.5, -0.8, 0))
    log("v2_sketch_one_dim", **sketch_info(sk))
    dh = sk.sketchDimensions.addDistanceDimension(rect[1].startSketchPoint, rect[1].endSketchPoint, V, P(5.8, 1.5, 0))
    dw.parameter.expression = "50 mm"
    dh.parameter.expression = "30 mm"
    state["dw"], state["dh"], state["rect"] = dw, dh, rect
    log("v2_sketch_fully_dimensioned", **sketch_info(sk))
    attempt("v2_overconstrain_third_dimension", lambda: sk.sketchDimensions.addDistanceDimension(
        rect[2].startSketchPoint, rect[2].endSketchPoint, H, P(2.5, 3.8, 0)).parameter.name)
    attempt("v2_overconstrain_driven_dimension", lambda: (lambda dd: (dd.parameter.name, dd.isDriving, dd.parameter.expression))(
        sk.sketchDimensions.addDistanceDimension(rect[2].startSketchPoint, rect[2].endSketchPoint, H,
                                                 P(2.5, 4.2, 0), False)))
    attempt("v2_overconstrain_parallel", lambda: gc.addParallel(rect[0], rect[2]).objectType)
    log("v2_sketch_after_overconstraint", **sketch_info(sk))
    app.activeViewport.fit()


def v2_shot_sketch():
    shot("v2_05_constrained_rectangle")


def v2_extrude_dialog():
    run_cmd("Extrude", preselect=state["sk"].profiles.item(0))


def v2_shot_extrude_dialog():
    shot("v2_06_extrude_dialog")
    end_cmd()


def v2_extrude_and_edit():
    root = design().rootComponent
    ext = root.features.extrudeFeatures.addSimple(state["sk"].profiles.item(0),
                                                  adsk.core.ValueInput.createByString("10 mm"),
                                                  adsk.fusion.FeatureOperations.NewBodyFeatureOperation)
    state["ext"] = ext
    model_snapshot("v2_extrude_10")
    adsk.fusion.DistanceExtentDefinition.cast(ext.extentOne).distance.expression = "20 mm"
    model_snapshot("v2_extrude_20")
    state["dw"].parameter.expression = "60 mm"
    model_snapshot("v2_width_60")
    attempt("v2_negative_width", lambda: setattr(state["dw"].parameter, "expression", "-10 mm"))
    model_snapshot("v2_after_negative_width_attempt")
    state["dw"].parameter.expression = "60 mm"
    attempt("v2_expression_reference", lambda: setattr(adsk.fusion.DistanceExtentDefinition.cast(
        ext.extentOne).distance, "expression", state["dh"].parameter.name + " / 2"))
    model_snapshot("v2_extrude_expression_d_over_2")
    attempt("v2_bad_expression", lambda: setattr(adsk.fusion.DistanceExtentDefinition.cast(
        ext.extentOne).distance, "expression", "foo * 2"))
    attempt("v2_bad_unit_expression", lambda: setattr(adsk.fusion.DistanceExtentDefinition.cast(
        ext.extentOne).distance, "expression", "15 deg"))
    adsk.fusion.DistanceExtentDefinition.cast(ext.extentOne).distance.expression = "20 mm"
    model_snapshot("v2_extrude_20_restored")
    app.activeViewport.fit()


def v2_select_face():
    body = design().rootComponent.bRepBodies.item(0)
    top = max([f for f in body.faces if adsk.core.Plane.cast(f.geometry)], key=lambda f: f.pointOnFace.z)
    ui.activeSelections.clear()
    ui.activeSelections.add(top)


def v2_shot_selection():
    shot("v2_07_face_selected")
    ui.activeSelections.clear()


def v2_fillet_cases():
    root = design().rootComponent
    body = root.bRepBodies.item(0)
    edges = adsk.core.ObjectCollection.create()
    for e in vertical_edges(body):
        edges.add(e)
    fi = root.features.filletFeatures.createInput()
    fi.edgeSetInputs.addConstantRadiusEdgeSet(edges, adsk.core.ValueInput.createByString("5 mm"), True)
    fil = root.features.filletFeatures.add(fi)
    state["fil"] = fil
    model_snapshot("v2_fillet_4_vertical_r5")
    state["dw"].parameter.expression = "70 mm"
    model_snapshot("v2_width_70_fillet_downstream")
    rp = adsk.fusion.ConstantRadiusFilletEdgeSet.cast(fil.edgeSets.item(0)).radius
    state["fil_radius_param"] = rp
    attempt("v2_fillet_radius_20_too_big", lambda: setattr(rp, "expression", "20 mm"))
    model_snapshot("v2_fillet_radius_20")


def v2_shot_fillet_error():
    shot("v2_08_fillet_error_state")


def v2_cut_and_dependency():
    state["fil_radius_param"].expression = "5 mm"
    model_snapshot("v2_fillet_restored")
    root = design().rootComponent
    body = root.bRepBodies.item(0)
    top = max([f for f in body.faces if adsk.core.Plane.cast(f.geometry)], key=lambda f: f.pointOnFace.z)
    sk2 = root.sketches.add(top)
    log("v2_face_sketch_transform", origin=[sk2.origin.x, sk2.origin.y, sk2.origin.z],
        xDir=[sk2.xDirection.x, sk2.xDirection.y, sk2.xDirection.z],
        yDir=[sk2.yDirection.x, sk2.yDirection.y, sk2.yDirection.z])
    c = sk2.modelToSketchSpace(P(3.5, 1.5, 2.0))
    sk2.sketchCurves.sketchCircles.addByCenterRadius(c, 0.6)
    prof = None
    for i in range(sk2.profiles.count):
        p = sk2.profiles.item(i)
        if p.areaProperties().area < 2:
            prof = p
    log("v2_face_sketch_profiles", count=sk2.profiles.count,
        areas_mm2=[sk2.profiles.item(i).areaProperties().area * 100 for i in range(sk2.profiles.count)])
    ei = root.features.extrudeFeatures.createInput(prof, adsk.fusion.FeatureOperations.CutFeatureOperation)
    ei.setOneSideExtent(adsk.fusion.ThroughAllExtentDefinition.create(),
                        adsk.fusion.ExtentDirections.NegativeExtentDirection)
    cut = root.features.extrudeFeatures.add(ei)
    state["cut"], state["sk2"] = cut, sk2
    model_snapshot("v2_cut_through_all")
    # move the sketch's driving width so the hole falls outside the body; observe the cut feature health
    state["dw"].parameter.expression = "20 mm"
    model_snapshot("v2_width_20_hole_outside")
    state["dw"].parameter.expression = "70 mm"
    model_snapshot("v2_width_70_again")


def v2_open_profile():
    root = design().rootComponent
    sk = root.sketches.add(root.xZConstructionPlane)
    ls = sk.sketchCurves.sketchLines
    a = ls.addByTwoPoints(P(10, 0, 0), P(14, 0, 0))
    b = ls.addByTwoPoints(a.endSketchPoint, P(14, 3, 0))
    ls.addByTwoPoints(b.endSketchPoint, P(10, 3, 0))
    log("v2_open_profile_sketch", **sketch_info(sk))
    attempt("v2_extrude_open_profile_item0", lambda: sk.profiles.item(0))
    sk.deleteMe()


def v2_delete_dependency():
    sk2 = state["sk2"]
    attempt("v2_delete_sketch_used_by_cut", lambda: sk2.deleteMe())
    model_snapshot("v2_after_deleting_cut_sketch")


def v2_shot_dependency():
    shot("v2_09_after_delete_dependency")


def v2_browser_and_timeline():
    dump_ui("v2-final")
    shot("v2_10_final_model")


def v2_close():
    write_json("snapshots-v2.json", [s for s in state.get("snapshots", [])])
    s_close()


STEPS_V2 = [v2_start, v2_new_doc, v2_shot_new, v2_enter_sketch, v2_rect_tool, v2_rect_click1, v2_rect_click2,
            v2_rect_done, v2_line_tool, v2_line_click1, v2_line_click2, v2_line_done, v2_finish_ui_sketch,
            v2_constrained_rect, v2_shot_sketch, v2_extrude_dialog, v2_shot_extrude_dialog, v2_extrude_and_edit,
            v2_select_face, v2_shot_selection, v2_fillet_cases, v2_shot_fillet_error, v2_cut_and_dependency,
            v2_open_profile, v2_delete_dependency, v2_shot_dependency, v2_browser_and_timeline, v2_close]

# =========================================================================== v3 pass
def v3_start():
    s_start()
    dump_command_definitions()


def v3_plane_frames():
    s_new_doc()
    root = design().rootComponent
    frames = {}
    for name, pl in [("XY", root.xYConstructionPlane), ("XZ", root.xZConstructionPlane), ("YZ", root.yZConstructionPlane)]:
        sk = root.sketches.add(pl)
        m = sk.transform
        n = adsk.core.Vector3D.create(m.getCell(0, 2), m.getCell(1, 2), m.getCell(2, 2))
        frames[name] = {"origin": [sk.origin.x, sk.origin.y, sk.origin.z],
                        "xDir": [sk.xDirection.x, sk.xDirection.y, sk.xDirection.z],
                        "yDir": [sk.yDirection.x, sk.yDirection.y, sk.yDirection.z],
                        "normal": [n.x, n.y, n.z],
                        "sketch_point_2_3_in_model": (lambda p: [p.x, p.y, p.z])(sk.sketchToModelSpace(P(2, 3, 0)))}
        sk.deleteMe()
    log("v3_origin_plane_sketch_frames", **frames)
    dump_ui("v3-design-workspace")


def v3_enter_sketch():
    run_cmd("SketchCreate", preselect=design().rootComponent.xYConstructionPlane)


def v3_focus():
    h = main_hwnd()
    user32.SetForegroundWindow(h)
    state["ui_sketch"] = adsk.fusion.Sketch.cast(app.activeEditObject)
    dump_ui("v3-sketch-mode")


def v3_rect_tool():
    run_cmd("ShapeRectangleTwoPoint")


def v3_click1():
    click_model_point(P(-4, -2, 0), "rect_corner_1")


def v3_click2():
    click_model_point(P(3, 2.5, 0), "rect_corner_2")


def v3_rect_check():
    shot("v3_ui_rectangle")
    end_cmd()
    sk = state.get("ui_sketch")
    if sk:
        log("v3_ui_rectangle_sketch", **sketch_info(sk))


def v3_circle_tool():
    run_cmd("CircleCenterRadius")


def v3_circle_click1():
    click_model_point(P(-1, 0, 0), "circle_center")


def v3_circle_click2():
    click_model_point(P(0, 0, 0), "circle_radius")


def v3_circle_check():
    end_cmd()
    sk = state.get("ui_sketch")
    if sk:
        log("v3_ui_circle_sketch", **sketch_info(sk))
    shot("v3_ui_rectangle_and_circle")


def v3_close():
    cid = find_cmd("SketchStop")
    if cid:
        ui.commandDefinitions.itemById(cid).execute()
    s_close()


# =========================================================================== v4 pass: joints
def _occ_info(occ):
    m = occ.transform2
    bb = None
    try:
        b = occ.bRepBodies.item(0)
        bx = b.boundingBox
        bb = [[round(v * 10, 3) for v in (bx.minPoint.x, bx.minPoint.y, bx.minPoint.z)],
              [round(v * 10, 3) for v in (bx.maxPoint.x, bx.maxPoint.y, bx.maxPoint.z)]]
    except Exception:
        pass
    return {"name": occ.name, "translation_mm": [round(m.getCell(i, 3) * 10, 3) for i in range(3)],
            "rotation_row0": [round(m.getCell(0, j), 4) for j in range(3)],
            "isGrounded": safe(lambda: occ.isGrounded), "bbox_world_mm": bb}


def _box_component(root, transform, w, d, h):
    occ = root.occurrences.addNewComponent(transform)
    comp = occ.component
    sk = comp.sketches.add(comp.xYConstructionPlane)
    sk.sketchCurves.sketchLines.addTwoPointRectangle(P(0, 0, 0), P(w, d, 0))
    comp.features.extrudeFeatures.addSimple(sk.profiles.item(0), adsk.core.ValueInput.createByReal(h),
                                            adsk.fusion.FeatureOperations.NewBodyFeatureOperation)
    return occ


def _outward_normal(f):
    ok, n = f.evaluator.getNormalAtPoint(f.pointOnFace)
    return n if ok else None


def _planar_face(occ, nz):
    """Planar face of the first body of occ whose outward normal z equals nz (+1 top, -1 bottom)."""
    for f in occ.bRepBodies.item(0).faces:
        if not adsk.core.Plane.cast(f.geometry):
            continue
        n = _outward_normal(f)
        if n and abs(n.z - nz) < 1e-6:
            return f
    return None


def v4_joints():
    s_new_doc()
    d = design()
    root = d.rootComponent
    occA = _box_component(root, adsk.core.Matrix3D.create(), 4, 4, 2)
    t = adsk.core.Matrix3D.create()
    t.translation = adsk.core.Vector3D.create(10, 0, 0)
    occB = _box_component(root, t, 3, 1.5, 1)
    state["occA"], state["occB"] = occA, occB
    log("v4_before", A=_occ_info(occA), B=_occ_info(occB))
    topA = _planar_face(occA, 1)
    botB = _planar_face(occB, -1)
    g1 = adsk.fusion.JointGeometry.createByPlanarFace(topA, None, adsk.fusion.JointKeyPointTypes.CenterKeyPoint)
    g2 = adsk.fusion.JointGeometry.createByPlanarFace(botB, None, adsk.fusion.JointKeyPointTypes.CenterKeyPoint)
    ji = root.joints.createInput(g1, g2)  # geometryOne = A (top face), geometryTwo = B (bottom face)
    ji.setAsRevoluteJointMotion(adsk.fusion.JointDirections.ZAxisJointDirection)
    ji.angle = adsk.core.ValueInput.createByString("30 deg")
    j = root.joints.add(ji)
    state["j1"] = j
    log("v4_joint_A_then_B", joint=j.name, health=HEALTH.get(j.healthState, j.healthState), msg=j.errorOrWarningMessage,
        occOne=j.occurrenceOne.name if j.occurrenceOne else None, occTwo=j.occurrenceTwo.name if j.occurrenceTwo else None,
        A=_occ_info(occA), B=_occ_info(occB), timeline=timeline_info())
    app.activeViewport.fit()


def v4_shot1():
    state["screen_capture"] = True
    shot("v4_01_joint_A_then_B")


def v4_reverse():
    occA, occB = state["occA"], state["occB"]
    state["j1"].deleteMe()
    log("v4_after_delete", A=_occ_info(occA), B=_occ_info(occB))
    topA = _planar_face(occA, 1)
    botB = _planar_face(occB, -1)
    g1 = adsk.fusion.JointGeometry.createByPlanarFace(botB, None, adsk.fusion.JointKeyPointTypes.CenterKeyPoint)
    g2 = adsk.fusion.JointGeometry.createByPlanarFace(topA, None, adsk.fusion.JointKeyPointTypes.CenterKeyPoint)
    ji = design().rootComponent.joints.createInput(g1, g2)  # geometryOne = B, geometryTwo = A
    ji.setAsRigidJointMotion()
    j = design().rootComponent.joints.add(ji)
    state["j2"] = j
    log("v4_joint_B_then_A", health=HEALTH.get(j.healthState, j.healthState), msg=j.errorOrWarningMessage,
        A=_occ_info(occA), B=_occ_info(occB))


def v4_conflict():
    occA, occB = state["occA"], state["occB"]
    # a second joint between the same pair using side faces: over-constrains the pair
    sideA = None
    sideB = None
    for f in occA.bRepBodies.item(0).faces:
        n = _outward_normal(f) if adsk.core.Plane.cast(f.geometry) else None
        if n and abs(n.x - 1) < 1e-6:
            sideA = f
    for f in occB.bRepBodies.item(0).faces:
        n = _outward_normal(f) if adsk.core.Plane.cast(f.geometry) else None
        if n and abs(n.x + 1) < 1e-6:
            sideB = f
    res = {}
    try:
        g1 = adsk.fusion.JointGeometry.createByPlanarFace(sideA, None, adsk.fusion.JointKeyPointTypes.CenterKeyPoint)
        g2 = adsk.fusion.JointGeometry.createByPlanarFace(sideB, None, adsk.fusion.JointKeyPointTypes.CenterKeyPoint)
        ji = design().rootComponent.joints.createInput(g2, g1)
        ji.setAsRigidJointMotion()
        j = design().rootComponent.joints.add(ji)
        res = {"created": True, "health": HEALTH.get(j.healthState, j.healthState), "msg": j.errorOrWarningMessage,
               "first_joint_health": HEALTH.get(state["j2"].healthState, state["j2"].healthState),
               "first_joint_msg": state["j2"].errorOrWarningMessage}
    except Exception as e:
        res = {"created": False, "error": str(e)}
    log("v4_conflicting_second_joint", A=_occ_info(occA), B=_occ_info(occB), timeline=timeline_info(), **res)


def v4_shot2():
    shot("v4_02_after_conflict")


def v4_close():
    write_json("snapshots-v4.json", state.get("snapshots", []))
    s_close()


STEPS_V4 = [v3_start, v4_joints, v4_shot1, v4_reverse, v4_conflict, v4_shot2, v4_close]

STEPS_V3 = [v3_start, v3_plane_frames, v3_enter_sketch, v3_focus, v3_rect_tool, v3_click1, v3_click2, v3_rect_check,
            v3_circle_tool, v3_circle_click1, v3_circle_click2, v3_circle_check, v3_close]

STEPS = [s_start, s_dump_ui_initial, s_new_doc, s_shot_new, s_sketch_prompt, s_shot_sketch_prompt,
         s_sketch_mode_ui, s_shot_sketch_mode, s_finish_sketch_ui, s_sketch_api, s_shot_sketch,
         s_extrude_dialog, s_shot_extrude_dialog, s_extrude, s_shot_extrude, s_edit_extrude, s_edit_sketch_dim,
         s_shot_edited, s_fillet, s_fillet_dialog, s_shot_fillet_dialog, s_upstream_edit_with_fillet,
         s_shot_invalid, s_restore_and_cut, s_chamfer, s_shot_cut, s_timeline_ops, s_revolve, s_shot_revolve,
         s_views, s_export, s_undo, s_undo_check, s_redo_check, s_close]


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
    # wait for the UI to settle after startup
    for _ in range(45):
        if stop_flag.wait(1):
            return
    for i in range(len(state["steps"])):
        if stop_flag.is_set():
            return
        step_done.clear()
        app.fireCustomEvent(EVENT_ID, str(i))
        step_done.wait(180)
        stop_flag.wait(4)


def run(context):
    global app, ui
    try:
        app = adsk.core.Application.get()
        ui = app.userInterface
        if not os.path.exists(SENTINEL):
            return
        with open(SENTINEL) as f:
            mode = f.read().strip()
        os.remove(SENTINEL)
        state["steps"] = {"v2": STEPS_V2, "v3": STEPS_V3, "v4": STEPS_V4}.get(mode, STEPS)
        log("heartbeat", version=app.version, mode=mode)
        ev = app.registerCustomEvent(EVENT_ID)
        h = StepHandler()
        ev.add(h)
        handlers.append(h)
        threading.Thread(target=worker, daemon=True).start()
    except Exception:
        try:
            log("run_error", err=traceback.format_exc())
        except Exception:
            pass


def stop(context):
    stop_flag.set()
    try:
        app.unregisterCustomEvent(EVENT_ID)
    except Exception:
        pass
