# Autodesk ShapeManager (ASM): public-source research

Status: research input for `crates/cad-geom` (the pure-Rust B-rep kernel) and for the Fusion experiment probe (v5).
Date: 2026-10-17. Sources: `research/evidence/public-sources.md`, cited here as [S#].

## Method and limits

- **Black-box only.** Every statement below comes from public documentation, public articles, patents, or the open literature. No ASM or Fusion binary was disassembled, decompiled, string-dumped, opened in a disassembler, or otherwise analysed as code. Autodesk's licence forbids this, and the project brief requires legitimate sources only.
- **Local binary inspection was limited to Windows version resources**, the metadata Explorer shows under File > Properties > Details, read with PowerShell `(Get-Item).VersionInfo` and `Get-FileHash`. These were used **for identity only** (§0).
  - **Disclosed scope extension:** besides the `ASM*232.dll` files, the version resources of three other files in the same folder were read: `Fusion360.exe`, `tsplines14.dll` and `SMTK.dll`.
  - Those three are recorded in §0 for context. They are **not** used as evidence for any claim about ASM or T-Splines integration. The coordinator may drop them.
- **Evidence labels.** Every claim is one of the following:
  - **documented [S#]**, from a source that was opened. Where a source was seen only as a search snippet, the evidence list says so.
  - **hypothesis**, marked *H:*, which has to be measured by probe v5 before anyone relies on it.
  - **unknown**.
- **Version caveat for ACIS documentation.** The only openly readable ACIS manuals found are the **ACIS R10** online docs [S10–S12], which are later than the ACIS 7.0 fork point.
  - Statements from them describe the ACIS lineage, not ASM as verified fact.
  - Their presence in today's ASM 232 is a hypothesis unless Fusion documentation confirms it.
  - The tolerant-modelling options in particular (`res_near_tangent`, `lop_tolerant_hot`, `bl_tolerant_hot`, `api_hh_make_tolerant`) may postdate the fork.
- Spatial's current documentation (doc.spatial.com) requires a customer login and was not used. Several Autodesk pages (the investor press release, the Fusion blog, forums.autodesk.com) returned HTTP 403. Those are cited as snippet-only. They are never the sole support for a documented claim. The single exception is [S28], which is labelled unverified and used only as a hypothesis.

---

## 0. Local identity evidence: ASM DLL version resources

Location: `%LOCALAPPDATA%\Autodesk\webdeploy\production\<build>\` (Fusion `Fusion360.exe` FileVersion **2705.1.30**, FileDescription "Autodesk Fusion").

All 37 `ASM*232.dll` files share these values:

| Field | Value |
|---|---|
| FileVersion | **232.5.0.65535** |
| ProductVersion | **232** |
| ProductName | Autodesk ShapeManager |
| CompanyName | Autodesk, Inc. |
| LegalCopyright (ASMKERN) | "1989-2001 by Spatial, 2026 Autodesk, Inc." |
| InternalName / OriginalFilename (ASMKERN) | "ASM - KERN" / "ASMkern232.dll" |

The copyright string, Spatial 1989–2001 followed by Autodesk, is independent identity evidence consistent with a 2001 fork of Spatial's code.

Main modules:

| File | FileDescription | SHA-256 |
|---|---|---|
| ASMKERN232.dll | ASM Kernel | `1FEFBD4C084A34821D92330647BD38F36552C26DD4052A58297D8FEE2A0D561E` |
| ASMBASE232.dll | ASM Base | `E3A30DE6ED8B7733CBAD22E7F7B1BB37A03D940A15FD1732AB207AFD38EFB026` |
| ASMBOOL232.dll | ASM Boolean | `85E8DF9AF9D42E3B8D789F19705579A38B5C2FB6003721D4F7C81FC55D659962` |
| ASMBLND232.dll | ASM Blend | `A299A0EA3F73F9677FE7B7BA7F2EE8EB7A54B6E06DC4B2D02098D2AA743C1D3D` |
| ASMHEAL232.dll | ASM Healing | `A3E161FB64ED121875B92E736AEEFF42B7C190FEF2E37301FE39CAD15CDB00AE` |
| ASMINTR232.dll | ASM Intersections | `6DD58EB8CE6AE38DA23702FBC4AF41965F3C8CDF8325B6989FECCF350307993C` |

All other `ASM*232.dll` modules, with their FileDescription:

- AHL: Autodesk Hidden Line Manager
- COVR: Cover
- CSTR: Construction
- CT: Cellular Topology
- DATAX: Data Exchange Tools
- DEFM: Deform
- EULR: Euler
- FCT: Faceter
- FREC: Feature recognition
- GA: Generalized Attributes
- IMPORT: Import
- LAW: Law
- LOP: Local Operations
- LOPT: Local Operations Tools
- NPCH: n-Sided Patch
- OFST: Offset
- OPER: Operator
- PID: Persistent ID
- RBASE: Rendering
- RBI: Repair Body Intersections
- REM: Remove Topology
- SASM: Shareable ASM
- SBAP: Selective Boolean Applications
- SBOOL: Selective Boolean
- SHL: Shelling
- SKIN: Skin
- SWP: Sweep
- TOPT: Topology Tools
- TWK: Tweak
- UFLD: Unfolder
- WELD: Weldment

The module names give a functional inventory only. They come from the version resources, not from code.

Context only (scope extension, not used as evidence):

- `tsplines14.dll`: FileVersion 14.2.0.0, "T-Splines", Autodesk, Inc.
- `SMTK.dll`: 7.0.5.0, "Sheet Metal Toolkit".

---

## 1. Identity and history

- **Origin.** ASM was forked from Spatial's **ACIS 7.0** in November 2001 [S1][S2]. This is verified, not assumed:
  - Machine Design (Feb 2002) quotes Autodesk: "ShapeManager will be based on the ACIS 7.0 kernel licensed from Spatial Corp, a division of Dassault Systemes" [S2].
  - The same quote appears in the press-release snippet [S6].
  - Wikipedia gives the fork date as November 2001 and says the first version shipped in **Inventor 5.3 in February 2002** [S1].
- **Legal basis.** A 1990 Autodesk–Spatial contract made Autodesk a joint developer, with a buy-out option ($6.4M) [S1].
  - Dassault (Spatial's owner since July 2000) sued Autodesk and D-Cubed in December 2001.
  - A jury found for Autodesk on 2 October 2003, and an appeal was lost in 2006 [S1].
- **Compatibility promise.** "The kernel will maintain compatibility with ACIS V7.0 and below" [S2]. Consistent third-party evidence:
  - AutoCAD cannot handle ACIS data newer than 7.0 [S9].
  - Revit does not import SAT newer than 7.0 [S8].
- **Products.**
  - Inventor [S1][S2]; AutoCAD, Inventor and Fusion [S3]; Fusion "works with the ShapeManager kernel (now multi-threaded) and with the Autodesk T-Splines technology" [S4].
  - Fusion ships ASM modules (§0).
  - **Revit is not established** by any opened source. It only shares the ACIS 7.0 SAT ceiling [S8], so Revit is *unknown*.
- **T-Splines.** Autodesk acquired T-Splines technology assets, not the company, in December 2011 [S7].
  - Fusion uses T-Splines alongside ASM [S4].
  - Autodesk holds T-spline patents, including one on converting trimmed NURBS B-rep faces into trim-free T-splines [S35] and Sederberg's T-spline patent, originally BYU's [S36].
  - How Fusion's T-spline (Form) bodies become ASM B-rep is not publicly documented: *unknown*.
- **File formats.**
  - ACIS: **SAT** is text and **SAB** is binary, with identical model data [S12]. The header records units, SPAresabs and SPAresnor [S10 0006].
  - ASM: the Fusion API reads "SMT, SMB, SAT, or SAB" files (`createFromFile`) and writes ".sat" and ".smt" (`exportToFile`) [S15].
  - SMT and SMB are ASM's own text and binary save formats. FileInfo, a third-party source, describes SMT as "ASCII text" and SMB as binary [S47]. The internal differences between SMT and SAT are not publicly documented (*unknown*).
  - *H:* the SAT written by Fusion is ACIS-7.0-compatible. The probe should read the SAT header version.

## 2. Modelling semantics relevant to a kernel

### 2.1 Topology hierarchy
- **ACIS** (SAT 7.0 lineage) [S11][S13]:
  - BODY → LUMP → SHELL (→ SUBSHELL) → FACE → LOOP → COEDGE → EDGE → VERTEX, plus WIRE.
  - A coedge belongs to one face loop, links to its partner coedge on the adjacent face, and carries an optional pcurve [S13].
  - The ACIS manual's block example has 6 faces, 12 edges and 24 coedges [S11].
- **Fusion API** [S20][S26]: BRepBody → BRepLump → BRepShell → BRepFace → BRepLoop → BRepCoEdge → BRepEdge → BRepVertex, plus BRepWire.
  - "Fusion enforces that a body will always only contain a single lump", and a disjoint result becomes several bodies [S26]. This is an **application-level** rule, not a kernel one.
  - A shell can be a void (`BRepShell.isVoid`). For example, a sphere hollowed by a Shell feature gives one lump with two shells [S26].
  - "All faces have one outer loop and can have zero or more inner loops" [S26].
  - Coedges run counter-clockwise on the outer loop and clockwise on inner loops ("the material is always to the left"). Each coedge is defined in the face's 2D parameter space [S26].
  - A BRepEdge has up to two coedges [S26].
  - A cone apex is a "degenerate edge" (`BRepEdge.isDegenerate`) [S20]. Sphere poles and seams are **not documented**.
- **Periodicity** is exposed through `SurfaceEvaluator.getParamAnomaly` (period and principal-period start, singularities) and `parametricRange` (principal period; unbounded directions give 0..0) [S27]. A cylinder's angular parameter runs from −π to π [S26].

### 2.2 Geometry types (Fusion API) [S25]
- `SurfaceTypes`:
  - Plane = 0, Cylinder = 1, Cone = 2, Sphere = 3, Torus = 4
  - EllipticalCylinder = 5, EllipticalCone = 6, Nurbs = 7
  - There is no separate type for procedural, sweep or offset surfaces. *H:* those are reported as NURBS. `BRepBody.convert(ProceduralToNURBSConversion)` mentions "procedurally calculated faces and edges" [S17][S18].
- `Curve3DTypes`:
  - Line3D = 0, Arc3D = 1, Circle3D = 2, Ellipse3D = 3, EllipticalArc3D = 4
  - InfiniteLine3D = 5, NurbsCurve3D = 6, Polyline3D = 7
- `Curve2DTypes`: the same layout with 2D names, 0–7.

### 2.3 Tolerances
- **ACIS lineage (R10 docs)** [S10]:
  - `SPAresabs` default is **1e-6**: "the distance below which ACIS considers two points to be coincident".
  - `SPAresnor` default is **1e-10**: the ratio of smallest to largest quantity, which "should not be changed".
  - `SPAresfit` default is **1e-3** (approximation fit).
  - The defaults assume the units are roughly millimetres. *H:* ASM inherited the same defaults.
- **Size box / dynamic range** [S10 0005]:
  - The largest representable quantity is resabs/resnor = **1e4**.
  - The practical dynamic range is **1e-5 to 1e4** model units.
  - Scaling resabs shifts the range but does not widen it.
- **Fusion** [S21]:
  - `Application.pointTolerance` is "the modeling tolerance used internally when comparing two points. The value is in centimeters." `Application.vectorAngleTolerance` is in radians.
  - **Neither page gives a numeric value.**
  - The only documented number is in `createProjectedBodyOutline`: "A positive tolerance must be greater than the point tolerance (0.000001)" [S15]. Every length on that page is in cm.
  - So the documented point tolerance is **1e-6 cm (1e-5 mm)**.
  - A forum snippet [S28] (unverified) claims 1e-9 cm for "BRep operations".
  - *H (key open question):* ASM in Fusion runs with resabs = 1e-6 in cm units. That would make it 10× looser in real-world terms than the ACIS mm default, with an ACIS-style size ceiling of about 1e4 cm = 100 m. The probe must measure `pointTolerance` and `vectorAngleTolerance` directly, and probe the size limit.
- **Tolerant modelling** [S11] (ACIS R10; the subset ASM shares is a hypothesis):
  - TEDGE and TVERTEX store per-entity tolerances.
  - Two tolerant entities are coincident if their distance is below max(tolerance, SPAresabs).
  - A tolerant edge's tolerance is the maximum gap between its coedges' equiparametric points.
  - Healing can convert edges it fails to heal into tolerant edges.
  - Fusion exposes `BRepEdge.isTolerant`/`.tolerance` and `BRepVertex.isTolerant`/`.tolerance`, "only useful when isTolerant is true", with units unstated [S20]. Faces have no tolerance [S20].

### 2.4 Operations
- **Booleans:** `TemporaryBRepManager.booleanOperation` modifies the target in place. Inputs "need not be solid but can be faces that are combined or trimmed" [S15].
- **Imprint:** `imprintOverlapBodies` returns paired lists of overlapping faces and edges, with an optional tolerance; 0 means the internal tolerance [S15].
- **Healing:** `BRepBodyDefinition.doFullHealing` defaults to true. `outcomeInfo` returns JSON warnings with internal codes [S24].
- **Coplanar faces:** `BRepEdgeDefinition.isMergeable` defaults to true, so coplanar connected planar faces may be merged into one [S24].
- **Not in the temporary API:** blending (fillet/chamfer), shelling, offsetting and stitching are absent from TemporaryBRepManager [S14]. They are reachable only through document features, which are application-level.
- **The ASM module list** (§0) confirms that dedicated blend, shell, offset, heal, local-ops, sweep and skin modules exist. It says nothing about their behaviour.

## 3. Fusion API surface that drives ASM directly (input to probe v5)

General notes:

- Internal units are **cm** and **radians**.
- "null" means `None` in Python.
- Every TemporaryBRepManager method was introduced in December 2017 [S14][S15], except `createProjectedBodyOutline`, introduced in November 2024.
- Get the manager with `tbm = adsk.fusion.TemporaryBRepManager.get()` (static) [S15].

### 3.1 TemporaryBRepManager [S14][S15]
| Method (Python) | Parameters (type, units) | Returns / failure | Documented notes |
|---|---|---|---|
| `booleanOperation(targetBody, toolBody, booleanType)` | BRepBody, BRepBody, BooleanTypes | bool | Target is modified and "must always be a temporary BRepBody"; tool is unchanged. BooleanTypes: Difference = 0, Intersection = 1, Union = 2 [S16]. |
| `copy(bRepEntity)` | BRepBody, BRepFace, **BRepLoop** or BRepEdge (parametric or temporary) | BRepBody | A face gives a one-face body; a loop or edge gives a wire body. Failure value not stated. |
| `createBox(box)` | OrientedBoundingBox3D (cm, via create) | BRepBody or null | |
| `createCylinderOrCone(pointOne, pointOneRadius, pointTwo, pointTwoRadius)` | Point3D, double cm, Point3D, double cm | BRepBody or null | Cylinder when the two radii are equal. |
| `createEllipticalCylinderOrCone(pointOne, pointOneMajorRadius, pointOneMinorRadius, pointTwo, pointTwoMajorRadius, majorAxisDirection)` | Point3D, cm, cm, Point3D, cm, Vector3D | BRepBody or null | The minor radius at point two keeps the point-one ratio. |
| `createSphere(center, radius)` | Point3D, cm | BRepBody or null | |
| `createTorus(center, axis, majorRadius, minorRadius)` | Point3D, Vector3D, cm, cm | BRepBody or null | Radius constraints not stated. |
| `createHelixWire(axisPoint, axisVector, startPoint, pitch, turns, taperAngle)` | Point3D, Vector3D, Point3D, cm, count, radians | wire BRepBody or null | |
| `createWireFromCurves(curves)` → `(body, edgeMap)` (documented form); `createWireFromCurves(curves, edgeMap, allowSelfIntersections)` (documented optional form; allowSelfIntersections defaults to False) | Curve3D[]: **only** Arc3D, Circle3D, Ellipse3D, EllipticalArc3D, Line3D (no NURBS); edgeMap is an output BRepEdge[] | BRepBody or null; edgeMap follows input order | *H:* `allowSelfIntersections=` passed by keyword still returns the tuple. |
| `createFaceFromPlanarWires(wireBodies)` | BRepBody[] of closed, coplanar, non-overlapping wires | BRepBody or null | One outer wire, the rest become holes. |
| `createRuledSurface(sectionOne, sectionTwo)` | **BRepWire**, BRepWire | BRepBody | Failure value not stated. |
| `createSilhouetteCurves(face, viewDirection, returnCoincidentSilhouettes)` | BRepFace, Vector3D, bool | BRepBody of wires, or null when there is no silhouette | |
| `createProjectedBodyOutline(body, projectionPlane, tolerance)` → `(body, containsApproximation)` | BRepBody, Plane, cm (negative uses the default 0.001 × bbox diagonal; positive must be > point tolerance 0.000001) | BRepBody of planar faces | Introduced Nov 2024. |
| `planeIntersection(body, plane)` | BRepBody, Plane | wire BRepBody | Empty or failure behaviour not stated. |
| `imprintOverlapBodies(bodyOne, bodyTwo, imprintCoincidentEdges, tolerance=0.0)` → `(ok, resultBodyOne, resultBodyTwo, bodyOneOverlappingFaces, bodyTwoOverlappingFaces, bodyOneOverlappingEdges, bodyTwoOverlappingEdges)` | BRepBody, BRepBody, bool, cm (0 = internal tolerance) | bool plus outputs; inputs unchanged | Faces and edges are paired by index. *H:* the tuple form with `tolerance=` passed by keyword. The docs show no such form. |
| `deleteFaces(faces, deleteSpecifiedFaces)` | BRepFace[] (all from one temporary body), bool | bool | Which boolean value keeps or deletes the faces is **not documented**. |
| `transform(body, transform)` | BRepBody, Matrix3D | bool, applied in place | Whether a temporary body is required is not stated. |
| `exportToFile(bodies, filename)` | BRepBody[], path; extension **".sat" or ".smt"** only | bool | |
| `createFromFile(filename)` | path to **SMT, SMB, SAT or SAB** | BRepBodies or null | |

### 3.2 Bodies, properties, measurement
- **BRepBody** [S17]:
  - `area` (cm²) and `volume` (cm³; **0 if the body is not solid**)
  - `isSolid` ("closed (solid) or not"), `isTemporary`, `isTransient`
  - `boundingBox` (BoundingBox3D), `preciseBoundingBox`, `orientedMinimumBoundingBox` (Jan 2024)
  - `lumps`, `shells`, `faces`, `edges`, `vertices`, `wires`, `concaveEdges`, `convexEdges`, `pointContainment(point)`
- **Physical properties** [S17][S19]:
  - `physicalProperties` always uses **LowCalculationAccuracy**.
  - `getPhysicalProperties(accuracy=LowCalculationAccuracy)` (Sep 2020).
  - `Design.physicalProperties(inputs, accuracy)` sums several inputs.
- **BRepBody.convert(options)** → BRepBody or null [S17][S18]:
  - `BRepConvertOptions` are documented as bitwise: ProceduralToNURBS = 0, AnalyticsToNURBS = 1 (all analytics except planes), PlanesToNURBS = 2, SplitPeriodicFaces = 4.
  - Note for the probe: because ProceduralToNURBS is **0**, it cannot be OR-combined. `1 | 0` is just AnalyticsToNURBS. *H:* procedural conversion is always applied, or 0 means "procedural only".
  - Element tempIds are preserved, and split faces map many-to-one.
- **PhysicalProperties** [S19]:
  - Properties: `area` (cm²), `volume` (cm³), `mass` (kg), `density` (kg/cm³), `centerOfMass` (Point3D), `accuracy`.
  - `getXYZMomentsOfInertia()` → `(ok, xx, yy, zz, xy, yz, xz)` in **kg·cm²**, about world axes. The sign convention for the off-diagonal terms is **not stated**.
  - `getPrincipalMomentsOfInertia()` → `(ok, i1, i2, i3)` in kg·cm². The ordering is not stated, so the probe should sort before comparing.
  - `getPrincipalAxes()` → `(ok, x, y, z)` as Vector3D.
  - `getRadiusOfGyration()` → `(ok, kx, ky, kz)` in cm. **The docs contradict themselves**: the description says "about the principal axes", but the parameters say "X/Y/Z partial". The probe should resolve this.
  - `getRotationToPrincipal()` → `(ok, rx, ry, rz)`, units unstated.
  - **There is no** getXYZProductsOfInertia.
  - `CalculationAccuracy`: Low = 0 (±1 %), Medium = 1 (±0.5 %), High = 2 (±0.1 %), VeryHigh = 3 (±0.01 %).
- **Edge and vertex tolerance** [S20]:
  - `BRepEdge.tolerance` and `BRepVertex.tolerance` are doubles "only useful when isTolerant is true". Units are not stated; *H:* cm.
  - `BRepEdge.isDegenerate`, `isParamReversed`, `length` (cm).
  - `BRepFace.isParamReversed`.
- **Measurement** [S22]:
  - `MeasureManager.measureMinimumDistance(geometryOne, geometryTwo)` → MeasureResults (`value` in cm, `positionOne`, `positionTwo`).
    - Accepted inputs: Occurrence, BRepBody, BRepFace, BRepEdge, BRepVertex, construction geometry, sketch entities, plus temporary Plane and Point3D.
  - `measureAngle(geometryOne, geometryTwo, geometryThree=None)` → value in radians. The third argument is used only for three-point angles, with the apex at the second point.
  - `getOrientedBoundingBox(geometry, lengthVector, widthVector)` → OrientedBoundingBox3D. widthVector must be perpendicular to lengthVector.
- **OrientedBoundingBox3D** [S23]:
  - `OrientedBoundingBox3D.create(centerPoint, lengthDirection, widthDirection, length, width, height)`: sizes in cm; direction magnitudes are ignored; height = length × width (right-hand rule).
  - Methods: `contains(point)`, `copy()`, `setOrientation(...)`. There is no intersects or combine.
- **BRepBodyDefinition** (Sep 2020) [S24]:
  - Construction steps:
    - `BRepBodyDefinition.create()`.
    - Add vertices with `createVertexDefinition(Point3D)`.
    - Add edges with `createEdgeDefinitionByCurve(startV, endV, curve)`. Valid curves are Arc3D, NurbsCurve3D, Circle3D, Ellipse3D, EllipticalArc3D and Line3D. **A closed curve uses the same vertex definition at both ends.**
    - Add lumps, then shells (`.add()`), then faces (`faceDefinitions.add(surface, isParamReversed)`, with surfaces NurbsSurface, Cone, Cylinder, EllipticalCone, EllipticalCylinder, Plane, Sphere, Torus), then loops (`.add()`), then coedges (`.add(edgeDef, isOpposedToEdge)`).
    - Build the body with `createBody()`, which returns BRepBody or null.
  - `doFullHealing` defaults to true; `outcomeInfo` is a JSON string array.
  - No pcurve input is documented. *H:* ASM computes the pcurves.
  - This is the natural route for feeding **cad-geom-built topology into ASM** for cross-validation.

## 4. Patents and literature

Patents. Only the ones actually opened are listed. "Active" is Google's estimate, not a legal conclusion, and nothing here is legal advice.

| Patent | Assignee | Relevance |
|---|---|---|
| US 9,269,189 B1 [S35] | Autodesk (active, est. ~2034) | Converts trimmed NURBS/T-spline B-rep faces into watertight trim-free T-splines. Relevant to T-spline ↔ B-rep. **Avoid the claimed method.** |
| US 7,274,364 B2 [S36] | BYU, now Autodesk (expired) | T-spline / T-NURCC definition with local refinement. Background for Form bodies. |
| US 11,886,165 B2 [S37] | Autodesk (active, est. ~2040) | Generative-design mesh → editable watertight B-rep. Out of scope for v1; **avoid the claimed method.** |
| US 5,649,084 [S38] | Hewlett-Packard, now PTC (expired) | B-rep booleans with epsilon-consistent intersection and vertex relocation. A free prior-art pattern for tolerance-consistent booleans. |
| EP 0 649 103 A1 [S40] | Hewlett-Packard GmbH (expired) | Edge blending that removes or trims topology the blend consumes. Free prior art for fillets. |
| US 7,236,168 B2 [S39] | Geometric Software, now HCL (expired) | Blend-chain recognition and removal (defeaturing). |

**Negative result:** no patent assigned to Spatial or Autodesk covering ASM's booleans, blending or tolerant modelling was found among the opened results. This does not mean none exist. A dedicated assignee search on Google Patents or the USPTO is still open.

Clean-room literature:

- Piegl & Tiller, *The NURBS Book*, 2nd ed., 1997 [S41]: NURBS evaluation, knot insertion, degree elevation, fitting.
- Mäntylä, *An Introduction to Solid Modeling*, 1988 [S42]: half-edge structure, Euler operators, the splitting-based boolean.
- Hoffmann, *Geometric and Solid Modeling*, 1989 [S43]: robustness and accuracy of booleans, surface intersection.
- Patrikalakis & Maekawa, *Shape Interrogation for CAD/CAM*, 2002 [S44]: curve/surface and surface/surface intersection, distance, offsets.
- Requicha & Voelcker, *Proc. IEEE* 73(1), 1985 [S45]: boundary evaluation by set-membership classification.
- The OCCT boolean specification [S31] is an openly licensed, detailed description of a production tolerance-based boolean.

## 5. ASM-documented vs OCCT-documented behaviour

The OCCT version is 7.8.1, the version used in `crates/cad-kernel`. The OCCT column is also measurable locally.

| Behaviour | ASM / Fusion | OCCT 7.8.1 |
|---|---|---|
| Default linear tolerance | Documented: Fusion point tolerance "0.000001" in a cm context [S15]. `Application.pointTolerance` exists, but its value is undocumented [S21]. ACIS lineage resabs 1e-6 [S10] (presence in ASM is a hypothesis). | Documented: `Precision::Confusion()` = 1e-7 [S29] |
| Default angular tolerance | `vectorAngleTolerance` exists, value undocumented [S21]. ACIS resnor 1e-10 is a ratio, not an angle [S10]. **To be measured by probe v5.** | Documented: `Precision::Angular()` = 1e-12 [S29] |
| Model-size limit | ACIS lineage: dynamic range 1e-5 to 1e4 units [S10] (hypothesis for ASM). **To be measured by probe v5.** | Unknown: no documented hard limit was found |
| Per-edge and per-vertex tolerance | Documented: only on tolerant edges and vertices (`isTolerant`) [S20]. ACIS TEDGE/TVERTEX [S11]. | Documented: every vertex, edge and face carries a tolerance [S32]. Vertex merging uses tolerance spheres [S31]. |
| Seam edges on periodic faces (cylinder, sphere, torus) | Unknown, not documented. **To be measured by probe v5**: edge and coedge counts per face, and whether one edge has two coedges on the same face. | Documented: a "2D curve on a closed surface" edge representation exists [S32]. Counts are **to be measured** locally. |
| Degenerate edges at poles and apex | Documented: cone apex = degenerate edge [S20]. Sphere poles **to be measured by probe v5**. | Documented: degenerated-edge flag [S32] and its handling in booleans [S31]. Counts **to be measured** locally. |
| Sphere topology counts (faces, edges, vertices, loops) | **To be measured by probe v5** | Not documented in the headers [S33]. **To be measured** locally. |
| Torus topology counts | **To be measured by probe v5** | Not documented [S33]. **To be measured** locally. |
| Boolean with coincident faces | Documented: the imprint API pairs overlapping faces and edges [S15]. `isMergeable` merges coplanar faces in BRepBodyDefinition [S24]. Union result **to be measured by probe v5**. | Documented: same-domain faces unified (FillSameDomainFaces); fuzzy value defaults to Confusion; glue options [S31] |
| Fillet failure behaviour | Unknown: not in TemporaryBRepManager [S14]. Feature-level only (application). **To be measured by probe v5** through document features. | Documented: IsDone, NbFaultyContours, FaultyContour, NbFaultyVertices, HasResult/BadShape (partial result), StripeStatus [S30] |
| Mass-property accuracy | Documented: four accuracy levels, ±1 % down to ±0.01 % [S19] | Unknown here: GProp is not researched in this pass |

## 6. Implications for the Rust kernel (`crates/cad-geom`)

What to replicate first, in order. Each step can be checked against the Fusion probe.

1. **Topology model mirroring ACIS and Fusion:**
   - Body → lump → shell → face → loop → coedge → edge → vertex, plus wire bodies [S13][S20].
   - A coedge carries an optional pcurve and its orientation relative to the edge; void shells; the material-on-the-left loop convention [S26].
   - Keep multiple lumps per body possible in the kernel. Single lump per body is a Fusion **application** rule [S26].
2. **Tolerance model:**
   - Global resabs and resnor-style constants in kernel units.
   - Per-edge and per-vertex optional tolerances with the max(tol, resabs) coincidence rule [S11].
   - Make the numeric defaults configurable until probe v5 measures Fusion's `pointTolerance` and `vectorAngleTolerance` and the size limit.
3. **Geometry set:**
   - Exactly the Fusion `SurfaceTypes` (plane, cylinder, cone, sphere, torus, elliptical cylinder and cone, NURBS) and `Curve3DTypes` [S25].
   - NURBS from Piegl & Tiller [S41].
   - Periodicity and singularity queries equivalent to `getParamAnomaly` [S27].
4. **Primitives matching TemporaryBRepManager:**
   - box from an oriented box, cylinder/cone, elliptical cylinder/cone, sphere, torus, helix wire, wire from curves, planar face from wires, ruled surface [S15].
   - Topology counts are matched only after the probe measures them.
5. **Queries:**
   - area, volume (0 for non-solids), mass properties with the same output conventions (kg·cm² etc. at the API boundary) [S17][S19].
   - point containment, bounding and oriented boxes, minimum distance, plane intersection.
6. **Booleans** (difference, intersection, union; target modified, tool kept) and **imprint** with overlapping-face pairing [S15][S16].
   - Follow Requicha & Voelcker classification [S45], Hoffmann robustness [S43], Patrikalakis & Maekawa intersections [S44], and OCCT-style tolerance handling as open prior art [S31][S38].
7. **SAT 7.0 text read/write** [S12][S13]:
   - Needed for interchange with ACIS 7.0-limited tools [S2][S8][S9].
   - It is also the **oracle channel**: the probe exports ASM bodies with `exportToFile(.sat)`, and cad-geom parses them to compare topology and geometry exactly. That is black-box use of a documented public format.
8. **Later:** blending, shelling, offsetting, healing and stitching, informed by expired prior art [S38][S39][S40]. Their ASM behaviour is observable only through Fusion document features.

**Application-level, not kernel** (do not put in cad-geom):

- the Fusion timeline and parametric recompute
- user parameters
- naming and persistence: `associativeID`, `tempId`, entity tracking
- the one-lump-per-body rule
- components and occurrences
- the Form/T-spline environment, which is a separate T-Splines library [S4]
- the default physical-property accuracy choice
- the decision to auto-merge coplanar faces (`isMergeable` is an input choice) [S24]

**Patent hygiene:** do not implement the claimed methods of the active Autodesk patents US 9,269,189 and US 11,886,165 [S35][S37].

## 7. Open questions for probe v5 (priority order)
1. The numeric `Application.pointTolerance` and `vectorAngleTolerance`, and whether ASM works in cm (pointTolerance 1e-6 cm?) [S15][S21].
2. Face, edge, vertex, loop and coedge counts for `createSphere`, `createTorus`, `createCylinderOrCone` (cylinder and cone) and `createBox`:
   - seams on periodic faces,
   - degenerate edges at poles and apex (`isDegenerate`),
   - the `getParamAnomaly` output per face.
   Export each body to `.sat` and record the header.
3. The size limit: create and boolean primitives at 1e3, 1e4 and 1e5 cm, and record failures and null returns.
4. Coincident-face booleans: union of two boxes sharing a face (merged or split faces?), plus `imprintOverlapBodies` outputs.
5. `BRepEdge` and `BRepVertex` `isTolerant` and `tolerance` values on primitives, after booleans, and after a SAT round-trip (`exportToFile` → `createFromFile`).
