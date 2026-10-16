# Official Autodesk documentation evidence

Collected 2026-10-16 by a research sub-task using WebFetch against Autodesk-hosted pages.
All citations use `help.autodesk.com/cloudhelp/ENU/<book>/files/<ID>.htm` (the `view/fusion360/?guid=` URLs
only return an empty shell to automated fetches). Base `B = https://help.autodesk.com/cloudhelp/ENU/`.

**Caveat:** quotes were extracted through WebFetch's summarising model with an instruction to copy text
exactly; early paraphrased fetches were discarded. Spot-check quotes in a browser before relying on wording.
Rows marked *courseware* come from an Autodesk-hosted 2021 training PDF, which is weaker than the help pages
(it even contradicts the current pan binding).

| # | Finding | URL | Verbatim quote | Confidence |
|---|---|---|---|---|
| 1a | Default mouse: Pan = hold MMB, Zoom = roll MMB, Orbit = Shift+MMB; Shift+Click+MMB orbit around point | B+`Fusion-GetStarted/files/GUID-F0491540-0324-470A-B651-2238D0EFAC30.htm` | "Hold Middle Mouse Button" (Pan); "Roll Middle Mouse Button" (Zoom); "Hold Shift+Middle Mouse Button" (Orbit) | documented |
| 1b | Preferences > General "Mouse setup"; Fusion preset zoom = roll MMB or Ctrl+Shift+MMB | B+`Fusion-GetStarted/files/GUID-878489CD-3A23-4303-8450-C2F4F8E410B1.htm` | "Zoom: Roll the middle mouse button or Ctrl + Shift + middle mouse button" | documented |
| 1c | Other presets (Alias, Inventor, SolidWorks, Tinkercad, PowerMill); F6 = fit | same | "you can press F6 to resize the view to fit in the current window." | documented |
| 1d | Zoom direction / reverse zoom preference | same | — | not found |
| 2a | ViewCube: face → orthographic view; corner → isometric; Home icon → home view | B+`Fusion-GetStarted/files/GS-VIEWCUBE.htm` | "Click a named face of the ViewCube (for example, Front, Top, or Right) to rotate the camera to that orthographic view." | documented |
| 2b | ViewCube menu camera: Orthographic / Perspective / Perspective with Ortho Faces | same | "Sets the projection used to render the canvas." | documented |
| 2c | ViewCube edge click | — | — | not found |
| 3a | Navigation bar: zoom/pan/orbit + display settings | B+`Fusion-GetStarted/files/GS-THE-FUSION-INTERFACE.htm` | "Contains commands used to zoom, pan, and orbit your design, as well as display settings to control the appearance of the interface" | documented |
| 3b | Nav bar: Orbit, Look At, Pan, Zoom, Fit, Display Settings, Viewports | Autodesk courseware PDF (files.upskill-dev.autodesk.com …M4-01_Navigation-and-display-settings.pdf) | "Look At: Zooms and rotates the model to display selected elements planar to the screen" | courseware |
| 4 | Browser nodes: Document Settings, Named Views, Origin (point, axes, planes); visibility icon | B+`Fusion-Designs/files/GUID-38F2A3B4-CE01-4375-A21D-4CC9A743B2A1.htm` | "Contains the origin point, X/Y/Z axes, and XY/XZ/YZ planes" | documented |
| 5a | Timeline: double-click to edit; drag to reorder; suppress affects dependents | B+`Fusion-Designs/files/ASM-USE-TIMELINE.htm` | "Suppressing features makes the features and all dependent features temporarily unavailable." | documented |
| 5b | Drag timeline Marker to view design at that point | B+`Fusion-Designs/files/ASM-DESIGN-MODELING-MODES.htm` | "You can drag the timeline Marker around in the the timeline to view the design at that point." | documented |
| 5c | Timeline error red / warning yellow (API guide) | B+`Fusion-360-API/files/CustomFeatures_UM.htm` | "shown with a red background for an error or a yellow background for a warning." | partially documented |
| 5d | Direct Modeling Mode removes the Timeline | B+`Fusion-Designs/files/ASM-DO-NOT-CAPTURE-DESIGN-HISTORY.htm` | "The Timeline and any features that were created in Parametric Modeling Mode are removed from the design." | documented |
| 6a | Sketch on plane or planar face; closed profile shaded blue | B+`Fusion-Sketch/files/SKT-3D-SKETCH.htm` | "A sketch profile is shaded blue when it is closed." | documented |
| 6b | Finish Sketch via contextual tab / marking menu | B+`Fusion-Sketch/files/SKT-FINISH-SKETCH.htm` | "Right-click to display the Marking Menu, then select Sketch > Finish Sketch" | documented |
| 6c | Sketch Palette options (Linetype, Look At, Sketch Grid, Snap, Slice, Show Profile/Points/Dimensions/Constraints/Construction/Projected, 3D Sketch) | B+`Fusion-Sketch/files/GUID-4183A4B7-E002-4396-AD5A-7FF3C8B2F33A.htm` | "Rotates the camera to look directly at the active sketch plane." | documented |
| 7a | Curve colours: unconstrained solid blue; construction dashed orange; projected purple; fixed green; constrained black | B+`Fusion-Sketch/files/SKT-3D-SKETCH.htm` | "Displays as a solid blue line when unconstrained." | documented |
| 7b | Fully constrained → black + red lock on browser sketch icon | B+`Fusion-Sketch/files/SKT-FULLY-DEFINE-CONSTRAIN-SKETCH.htm` | "a red lock displays on the icon next to the sketch" | documented |
| 7c | Over-constraining dimension can be made Driven; driven value in parentheses | B+`Fusion-Sketch/files/SKT-SKETCH-CREATE-DIMENSIONS.htm` | "When a sketch is over-constrained, you can choose to make a dimension Driven instead of Driving." | partially documented |
| 8 | Constraints: Horizontal/Vertical, Coincident, Tangent, Equal, Parallel, Perpendicular, Fix/UnFix, Midpoint, Concentric, Collinear, Symmetry, Curvature | B+`Fusion-Sketch/files/SKT-CONSTRAINTS.htm` | "locks the size and location of a point or object." | documented |
| 9 | Sketch Dimension infers type from selection; value or expression; Enter accepts | B+`Fusion-Sketch/files/SKT-CREATE-DIMENSIONS.htm` | "Circles create diameter dimensions. Arcs create radius dimensions." | documented |
| 10a | 2-Point / 3-Point / Center rectangle definitions | B+`Fusion-Sketch/files/SKT-SKETCH-CREATE-RECTANGLES.htm` | "The 2 points are opposite corners of the rectangle." | documented |
| 10b | Automatic "logical constraints" when snapping (rectangle H/V not stated) | B+`Fusion-Sketch/files/SKT-CREATE-RECTANGLES.htm` | "If you snap to a specific point, the logical constraints are automatically added to the sketch." | partially documented |
| 12 | Extrude inputs: Type, Start, Direction (One Side/Two Sides/Symmetric), Extent (Distance/To Object/All), Taper, Operation (Join/Cut/Intersect/New Body/New Component) | B+`Fusion-Model/files/SLD-REF-EXTRUDE.htm` | "Extrudes through all visible bodies along the extrusion direction." | documented |
| 13a | Revolve inputs: Profile, Axis, Extent (Partial/To Object/Full), Direction, Angle, Operation | B+`Fusion-Model/files/SLD-REVOLVE-SOLID.htm` | "Revolves the profile 360 degrees around the axis." | documented |
| 13b | Fillet inputs: Type, Radius Type (Constant/Chord/Variable), Tangent Chain, Corner Type | B+`Fusion-Model/files/SLD-REF-FILLET.htm` | "Check to automatically select tangent geometry." | documented |
| 13c | Chamfer types: Equal Distance / Two Distance / Distance And Angle | B+`Fusion-Model/files/SLD-REF-CHAMFER.htm` | "Bevels edges based on a single distance and an angle." | documented |
| 13d | Hole inputs: Placement, Extents, Hole Type (Simple/Counterbore/Countersink), Tap Type | B+`Fusion-Model/files/GUID-3A76B269-8C8D-437B-8F4A-85D0B2BBA492.htm` | "Select Simple, Counterbore, or Countersink." | documented |
| 14 | Model parameters auto-created; user parameters; unit types must match | B+`Fusion-Model/files/SLD-MODIFY-PARAMETERS.htm` | "Model parameters are automatically created when you create timeline features or define dimensions in your design." | documented |
| 15a | Export formats incl. f3d/f3z, 3MF, IGES, OBJ, SAT, SMT, STEP, STL, USD; exports lose associativity | B+`Fusion-Designs/files/ASM-EXPORT-DESIGN.htm` | "An exported design does not maintain associativity with the original Fusion design." | documented |
| 16a | Marking menu on right-click in canvas | B+`Fusion-GetStarted/files/GUID-6514ABC1-CB75-4F0B-AB0E-316FAD36BA93.htm` | "Right-click anywhere in the canvas, and the Marking menu will appear around your cursor." | documented |
| 16b | Shortcuts: E Extrude, L Line, R 2-Point Rectangle, C Circle, D Sketch Dimension, F Fillet, H Hole, Q Press Pull, M Move, T Trim, X Construction, P Project, O Offset, J Joint, A Appearance, I Measure, V Visibility, S Model Toolbox, Ctrl+B Compute All | B+`Fusion-GetStarted/files/GUID-F0491540-0324-470A-B651-2238D0EFAC30.htm` | "Normal / Construction  X" | documented |
| 17a | Window (L→R, enclosed) vs crossing (R→L) selection | B+`Fusion-Model/files/SLD-SELECTION-MODES.htm` | "from right to left to select all objects that are crossed" | documented |
| 17b | Selection priority: components/bodies/faces/edges | B+`Fusion-Model/files/SLD-SELECTION-PRIORITY-FILTERS.htm` | "limit selection to components, bodies, faces, or edges" | documented |
| 18 | Default modeling orientation Z up | B+`Fusion-GetStarted/files/GUID-878489CD-3A23-4303-8450-C2F4F8E410B1.htm` | "The default modeling orientation is Z up." | documented |

## Not established from documentation
ViewCube edge clicks; zoom direction preference; Grid and Snaps menu contents; whether the rectangle
tools add H/V constraints automatically; deleting timeline features; "overlapping curves create multiple
profiles"; that f3d preserves history while STEP does not (support articles returned HTTP 403).

## Failed URLs
`help.autodesk.com/view/fusion360/ENU/?guid=*` (empty shell); several `cloudhelp` IDs returned 404
(ASM-TIMELINE, SLD-REF-REVOLVE, TPD-DESIGN-EXPORT, …); `autodesk.com/support/*` and the Fusion blog returned 403.
