//! Command registry. Every ribbon button, menu item, marking-menu entry, shortcut and scripted
//! scenario step goes through the same `CommandId` (CADCraft principle: "everything is a
//! command"). Tools CAD360 does not implement yet are still registered so the ribbon mirrors
//! Fusion's organisation, but they are shown disabled with "Not yet supported in CAD360" — no
//! control is inert.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommandId {
    // file / edit
    NewDesign,
    Open,
    Save,
    SaveAs,
    ImportStep,
    ExportStep,
    ExportStl,
    Undo,
    Redo,
    Delete,
    ComputeAll,
    // sketch
    CreateSketch,
    FinishSketch,
    Line,
    Rectangle2Pt,
    RectangleCenter,
    Rectangle3Pt,
    CircleCenter,
    Circle2Pt,
    Arc3Pt,
    ArcCenter,
    Point,
    Polygon,
    Spline,
    Slot,
    SketchFillet,
    Trim,
    Extend,
    SketchOffset,
    SketchMirror,
    Project,
    SketchDimension,
    ToggleConstruction,
    ConstraintCoincident,
    ConstraintCollinear,
    ConstraintConcentric,
    ConstraintMidpoint,
    ConstraintFix,
    ConstraintParallel,
    ConstraintPerpendicular,
    ConstraintHorizontalVertical,
    ConstraintTangent,
    ConstraintSmooth,
    ConstraintEqual,
    ConstraintSymmetry,
    // solid create
    Extrude,
    Revolve,
    Sweep,
    Loft,
    Rib,
    Hole,
    Thread,
    BoxPrimitive,
    RectangularPattern,
    CircularPattern,
    MirrorFeature,
    // modify
    PressPull,
    Fillet,
    Chamfer,
    Shell,
    Draft,
    Combine,
    Move,
    Appearance,
    ChangeParameters,
    // assemble
    NewComponent,
    Joint,
    // construct
    OffsetPlane,
    PlaneAtAngle,
    MidPlane,
    AxisThroughCylinder,
    // inspect
    Measure,
    SectionAnalysis,
    Interference,
    CenterOfMass,
    // insert
    InsertMesh,
    InsertSvg,
    // ===== surface =====
    // SURFACE ▸ Create
    CreateForm,
    SurfaceExtrude,
    SurfaceRevolve,
    SurfaceSweep,
    SurfaceLoft,
    Patch,
    Ruled,
    OffsetSurface,
    Thicken,
    BoundaryFill,
    // SURFACE ▸ Modify
    SurfaceTrim,
    Untrim,
    SurfaceExtend,
    Stitch,
    Unstitch,
    ReverseNormal,
    // MESH ▸ Create / Prepare / Modify
    Tessellate,
    MeshSectionSketch,
    BaseMeshFeature,
    MeshRepair,
    GenerateFaceGroups,
    CombineFaceGroups,
    MeshDirectEdit,
    Remesh,
    MeshReduce,
    MeshPlaneCut,
    MeshShell,
    MeshCombine,
    MeshSmooth,
    MeshReverseNormal,
    MeshSeparate,
    MeshScale,
    ConvertMesh,
    // UTILITIES
    ThreeDPrint,
    NestPreparation,
    ScriptsAddIns,
    ManageMaterials,
    // view
    Fit,
    ViewHome,
    ViewTop,
    ViewFront,
    ViewRight,
    ViewBack,
    ViewLeft,
    ViewBottom,
    ToggleProjection,
    LookAt,
    ToggleGrid,
    OrbitMode,
    PanMode,
    ZoomMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    File,
    Create,
    Modify,
    Assemble,
    Construct,
    Inspect,
    Insert,
    Select,
    SketchCreate,
    SketchModify,
    SketchConstraints,
    View,
    // ===== surface ===== (panels of the SURFACE, MESH and UTILITIES tabs)
    SurfaceCreate,
    SurfaceModify,
    MeshCreate,
    MeshPrepare,
    MeshModify,
    MeshExport,
    Make,
    Nest,
    AddIns,
    Utility,
}

pub struct CommandSpec {
    pub id: CommandId,
    pub label: &'static str,
    pub tooltip: &'static str,
    pub shortcut: Option<&'static str>,
    pub panel: Panel,
    /// Promoted to the ribbon (otherwise only in the panel's drop-down).
    pub promoted: bool,
    pub supported: bool,
}

macro_rules! specs {
    ($( $id:ident, $label:expr, $tip:expr, $sc:expr, $panel:ident, $promoted:expr, $sup:expr; )*) => {
        pub const COMMANDS: &[CommandSpec] = &[
            $( CommandSpec { id: CommandId::$id, label: $label, tooltip: $tip, shortcut: $sc, panel: Panel::$panel, promoted: $promoted, supported: $sup }, )*
        ];
    };
}

// Labels and tooltips are CAD360's own wording; panel placement and shortcuts follow Fusion
// (probe ribbon screenshots; Autodesk keyboard-shortcut reference).
specs! {
    NewDesign, "New Design", "Creates a new, empty design.", Some("Ctrl+N"), File, false, true;
    Open, "Open...", "Opens a CAD360 design (.c360).", Some("Ctrl+O"), File, false, true;
    Save, "Save", "Saves the design with its full feature history.", Some("Ctrl+S"), File, false, true;
    SaveAs, "Save As...", "Saves the design under a new name.", Some("Ctrl+Shift+S"), File, false, true;
    ImportStep, "Import STEP...", "Inserts the solids of a STEP file as a base feature (no history).", None, Insert, true, true;
    ExportStep, "Export STEP...", "Exports the visible bodies as exact B-rep solids (STEP AP214). History is not preserved.", None, File, false, true;
    ExportStl, "Export STL...", "Exports the visible bodies as a triangle mesh (STL). History is not preserved.", None, File, false, true;
    Undo, "Undo", "Undoes the last operation.", Some("Ctrl+Z"), File, false, true;
    Redo, "Redo", "Redoes the last undone operation.", Some("Ctrl+Y"), File, false, true;
    Delete, "Delete", "Deletes the selection.", Some("Delete"), Modify, false, true;
    ComputeAll, "Compute All", "Recomputes every feature of the design.", Some("Ctrl+B"), Modify, false, true;

    CreateSketch, "Create Sketch", "Creates a sketch on a plane or planar face. Select a plane to start.", None, Create, true, true;
    FinishSketch, "Finish Sketch", "Exits the sketch and recomputes the design.", None, SketchCreate, false, true;
    Line, "Line", "Creates lines. Click to place points; double-click or Esc to end the chain.", Some("L"), SketchCreate, true, true;
    Rectangle2Pt, "2-Point Rectangle", "Creates a rectangle from two opposite corners. Adds horizontal and vertical constraints.", Some("R"), SketchCreate, true, true;
    RectangleCenter, "Center Rectangle", "Creates a rectangle from its centre and a corner.", None, SketchCreate, false, true;
    Rectangle3Pt, "3-Point Rectangle", "Creates a rectangle at any angle: click two corners of the first edge, then the height. Adds perpendicular and parallel constraints.", None, SketchCreate, false, true;
    CircleCenter, "Center Diameter Circle", "Creates a circle from its centre and a point on it.", Some("C"), SketchCreate, true, true;
    Circle2Pt, "2-Point Circle", "Creates a circle from the two ends of a diameter.", None, SketchCreate, false, true;
    Arc3Pt, "3-Point Arc", "Creates an arc through a start point, a point on the arc and an end point.", None, SketchCreate, true, true;
    ArcCenter, "Center Point Arc", "Creates an arc from its centre, start point and end. The arc follows the direction the cursor moves.", None, SketchCreate, false, true;
    Point, "Point", "Creates a sketch point.", None, SketchCreate, false, true;
    Polygon, "Polygon", "Creates a regular polygon from its centre and a vertex (inscribed) or edge midpoint (circumscribed). Set the number of sides in the Sketch Palette.", None, SketchCreate, false, true;
    Spline, "Fit Point Spline", "Creates a spline through points.", None, SketchCreate, false, false;
    Slot, "Slot", "Creates a centre-to-centre slot: click both arc centres, then the width.", None, SketchCreate, false, true;
    SketchFillet, "Sketch Fillet", "Rounds the corner of two lines or arcs with a tangent arc. Click the corner (or both curves), then enter the radius.", None, SketchModify, true, true;
    Trim, "Trim", "Removes the clicked curve segment up to the nearest intersections. A curve with no intersections is deleted.", Some("T"), SketchModify, true, true;
    Extend, "Extend", "Extends the clicked end of a line or arc to the nearest curve.", None, SketchModify, false, true;
    SketchOffset, "Offset", "Copies a connected chain of curves at a distance. Select a curve, click the side, then enter the distance.", Some("O"), SketchModify, true, true;
    SketchMirror, "Mirror", "Mirrors sketch curves about a line: select the curves, then click the mirror line (Shift+click adds lines to the objects; Enter also moves on to the mirror line). Adds symmetry constraints.", None, SketchModify, false, true;
    Project, "Project", "Projects model edges into the sketch.", Some("P"), SketchCreate, false, false;
    SketchDimension, "Sketch Dimension", "Adds a dimension. The type follows the selection: line/two points -> distance, circle -> diameter, arc -> radius, two lines -> angle.", Some("D"), SketchCreate, true, true;
    ToggleConstruction, "Normal / Construction", "Toggles the selected curves between normal and construction geometry.", Some("X"), SketchModify, false, true;
    ConstraintCoincident, "Coincident", "Makes two points coincide, or puts a point on a curve.", None, SketchConstraints, true, true;
    ConstraintCollinear, "Collinear", "Makes two lines lie on the same infinite line.", None, SketchConstraints, true, true;
    ConstraintConcentric, "Concentric", "Gives circles or arcs the same centre.", None, SketchConstraints, true, true;
    ConstraintMidpoint, "Midpoint", "Puts a point at the midpoint of a line.", None, SketchConstraints, true, true;
    ConstraintFix, "Fix/UnFix", "Locks a point at its current position.", None, SketchConstraints, true, true;
    ConstraintParallel, "Parallel", "Makes two lines parallel.", None, SketchConstraints, true, true;
    ConstraintPerpendicular, "Perpendicular", "Makes two lines perpendicular.", None, SketchConstraints, true, true;
    ConstraintHorizontalVertical, "Horizontal/Vertical", "Makes a line horizontal or vertical, whichever is closer.", None, SketchConstraints, true, true;
    ConstraintTangent, "Tangent", "Makes a line or circle tangent to a circle or arc.", None, SketchConstraints, true, true;
    ConstraintSmooth, "Curvature", "G2 curvature continuity (splines).", None, SketchConstraints, false, false;
    ConstraintEqual, "Equal", "Makes two lines equal length, or two circles/arcs equal radius.", None, SketchConstraints, true, true;
    ConstraintSymmetry, "Symmetry", "Makes two points symmetric about a line.", None, SketchConstraints, true, true;

    Extrude, "Extrude", "Adds depth to closed sketch profiles. Select profiles, then set the distance and operation.", Some("E"), Create, true, true;
    Revolve, "Revolve", "Revolves profiles around an axis.", None, Create, true, true;
    Sweep, "Sweep", "Sweeps a profile along a path.", None, Create, false, false;
    Loft, "Loft", "Blends between profiles.", None, Create, false, false;
    Rib, "Rib", "Creates a rib from an open profile.", None, Create, false, false;
    Hole, "Hole", "Creates a simple drilled hole on a planar face.", Some("H"), Create, true, true;
    Thread, "Thread", "Adds a thread to a cylindrical face.", None, Create, false, false;
    BoxPrimitive, "Box", "Creates a box primitive.", None, Create, false, false;
    RectangularPattern, "Rectangular Pattern", "Copies bodies in rows along one or two origin axes. Set the quantity (including the original) and the spacing.", None, Create, true, true;
    CircularPattern, "Circular Pattern", "Copies bodies around an origin axis. Set the quantity (including the original) and the total angle.", None, Create, false, true;
    MirrorFeature, "Mirror", "Creates mirrored copies of bodies across an origin plane, construction plane or planar face.", None, Create, false, true;

    PressPull, "Press Pull", "Offsets faces / edits fillets directly.", Some("Q"), Modify, true, false;
    Fillet, "Fillet", "Rounds the selected edges with a constant radius.", Some("F"), Modify, true, true;
    Chamfer, "Chamfer", "Bevels the selected edges with an equal distance.", None, Modify, true, true;
    Shell, "Shell", "Hollows a body: select the faces to remove and set the inside wall thickness.", None, Modify, true, true;
    Draft, "Draft", "Tapers faces.", None, Modify, false, false;
    Combine, "Combine", "Joins, cuts or intersects tool bodies with a target body. Keep Tools leaves the tool bodies in place.", None, Modify, true, true;
    Move, "Move/Copy", "Moves or copies bodies: rotate about an origin axis and translate by X/Y/Z distances.", Some("M"), Modify, true, true;
    Appearance, "Appearance", "Changes the colour of the selected bodies.", Some("A"), Modify, false, true;
    ChangeParameters, "Change Parameters", "Shows the model and user parameters; edit expressions to drive the design.", None, Modify, true, true;

    NewComponent, "New Component", "Creates a component and activates it; new features go into the active component. Bodies selected first are moved into the new component.", None, Assemble, true, true;
    Joint, "Joint", "Positions one component relative to another: select a planar face or circular edge on each component, then choose Rigid, Revolute or Slider and set the angle / offset.", Some("J"), Assemble, true, true;

    OffsetPlane, "Offset Plane", "Creates a construction plane offset from a plane or planar face.", None, Construct, true, true;
    PlaneAtAngle, "Plane at Angle", "Creates a plane at an angle about an edge.", None, Construct, false, false;
    MidPlane, "Midplane", "Creates a plane midway between two faces.", None, Construct, false, false;
    AxisThroughCylinder, "Axis Through Cylinder", "Creates a construction axis.", None, Construct, false, false;

    Measure, "Measure", "Measures the selected faces, edges or bodies (length, area, volume, distance).", Some("I"), Inspect, true, true;
    SectionAnalysis, "Section Analysis", "Cuts the view with a plane (XY / XZ / YZ, offset, flip) to look inside the bodies. Display only; the model is unchanged.", None, Inspect, true, true;
    Interference, "Interference", "Reports the volume shared by every pair of the selected solid bodies (or of all visible bodies when none are selected). Select bodies, then click Compute.", None, Inspect, false, true;
    CenterOfMass, "Center of Mass", "Shows the centre of mass of the selected solid bodies (or of all visible bodies), with their volume and surface area. Uniform density.", None, Inspect, false, true;

    InsertMesh, "Insert Mesh", "Inserts an STL mesh (embedded in the design) as a mesh body. Convert to B-rep sews the triangles into a solid so solid features work on it.", None, Insert, true, true;
    InsertSvg, "Insert SVG", "Inserts SVG curves into a sketch.", None, Insert, false, false;

    // ===== surface =====
    CreateForm, "Create Form", "Enters the Form (T-spline) environment.", None, SurfaceCreate, true, false;
    SurfaceExtrude, "Extrude", "Extrudes sketch curves (open or closed) into a surface body. Select a sketch, choose its curve chains, then set the distance.", None, SurfaceCreate, true, true;
    SurfaceRevolve, "Revolve", "Revolves sketch curves around an axis into a surface body.", None, SurfaceCreate, true, true;
    SurfaceSweep, "Sweep", "Sweeps curves along a path into a surface.", None, SurfaceCreate, true, false;
    SurfaceLoft, "Loft", "Lofts a surface between curves.", None, SurfaceCreate, false, false;
    Patch, "Patch", "Fills a closed boundary with a surface: select closed sketch profiles or a closed loop of body edges (planar or not).", None, SurfaceCreate, true, true;
    Ruled, "Ruled", "Creates a ruled surface from edges.", None, SurfaceCreate, false, false;
    OffsetSurface, "Offset", "Offsets faces along their normals into a new surface body.", None, SurfaceCreate, false, true;
    Thicken, "Thicken", "Thickens faces or surface bodies into a solid (one side or symmetric).", None, SurfaceCreate, false, true;
    BoundaryFill, "Boundary Fill", "Creates solids from the cells enclosed by surfaces and bodies.", None, SurfaceCreate, false, false;
    SurfaceTrim, "Trim", "Trims surfaces with a cutting tool.", None, SurfaceModify, true, false;
    Untrim, "Untrim", "Removes trims and fills holes in surfaces.", None, SurfaceModify, false, false;
    SurfaceExtend, "Extend", "Extends surface edges.", None, SurfaceModify, true, false;
    Stitch, "Stitch", "Sews surface bodies along edges within the tolerance. A closed result becomes a solid body.", None, SurfaceModify, true, true;
    Unstitch, "Unstitch", "Separates faces of a body into surface bodies.", None, SurfaceModify, true, false;
    ReverseNormal, "Reverse Normal", "Flips the normal direction of surface faces.", None, SurfaceModify, false, false;
    Tessellate, "Tessellate", "Converts B-rep bodies into mesh bodies.", None, MeshCreate, true, false;
    MeshSectionSketch, "Create Mesh Section Sketch", "Creates a sketch from a planar section of a mesh body.", None, MeshCreate, true, false;
    BaseMeshFeature, "Create Base Mesh Feature", "Creates an empty base mesh feature for direct mesh edits.", None, MeshCreate, false, false;
    MeshRepair, "Repair", "Closes holes and fixes defects of a mesh body.", None, MeshPrepare, true, false;
    GenerateFaceGroups, "Generate Face Groups", "Splits a mesh body into face groups.", None, MeshPrepare, true, false;
    CombineFaceGroups, "Combine Face Groups", "Merges face groups of a mesh body.", None, MeshPrepare, true, false;
    MeshDirectEdit, "Direct Edit", "Edits mesh faces directly.", None, MeshModify, true, false;
    Remesh, "Remesh", "Regenerates the triangles of a mesh body.", None, MeshModify, true, false;
    MeshReduce, "Reduce", "Reduces the triangle count of a mesh body.", None, MeshModify, true, false;
    MeshPlaneCut, "Plane Cut", "Cuts a mesh body with a plane.", None, MeshModify, true, false;
    MeshShell, "Shell", "Hollows a mesh body.", None, MeshModify, true, false;
    MeshCombine, "Combine", "Joins, cuts or intersects mesh bodies.", None, MeshModify, true, false;
    MeshSmooth, "Smooth", "Smooths a mesh body.", None, MeshModify, false, false;
    MeshReverseNormal, "Reverse Normal", "Flips the normals of a mesh body.", None, MeshModify, false, false;
    MeshSeparate, "Separate", "Separates disconnected shells of a mesh body.", None, MeshModify, false, false;
    MeshScale, "Scale Mesh", "Scales a mesh body.", None, MeshModify, true, false;
    ConvertMesh, "Convert Mesh", "Converts a mesh body into a B-rep body.", None, MeshModify, false, false;
    ThreeDPrint, "3D Print", "Exports the selected bodies (or all visible bodies) as an STL file for 3D printing, with a coarse / medium / fine mesh refinement.", None, Make, true, true;
    NestPreparation, "Nest Preparation", "Prepares bodies for nesting.", None, Nest, true, false;
    ScriptsAddIns, "Scripts and Add-Ins...", "Runs scripts and add-ins.", None, AddIns, true, false;
    ManageMaterials, "Manage Materials", "Opens the material library.", None, Utility, true, false;

    Fit, "Fit", "Fits the whole design in the view.", Some("F6"), View, false, true;
    ViewHome, "Home", "Returns to the home (isometric) view.", None, View, false, true;
    ViewTop, "Top", "Views from the top.", None, View, false, true;
    ViewFront, "Front", "Views from the front.", None, View, false, true;
    ViewRight, "Right", "Views from the right.", None, View, false, true;
    ViewBack, "Back", "Views from the back.", None, View, false, true;
    ViewLeft, "Left", "Views from the left.", None, View, false, true;
    ViewBottom, "Bottom", "Views from the bottom.", None, View, false, true;
    ToggleProjection, "Perspective / Orthographic", "Switches the camera projection.", None, View, false, true;
    LookAt, "Look At", "Looks straight at the selected face or the active sketch plane.", None, View, false, true;
    ToggleGrid, "Grid", "Shows or hides the layout grid.", None, View, false, true;
    OrbitMode, "Orbit", "Left-drag orbits the view (Shift + middle mouse also orbits).", None, View, false, true;
    PanMode, "Pan", "Left-drag pans the view (middle mouse also pans).", None, View, false, true;
    ZoomMode, "Zoom", "Left-drag zooms the view (the wheel also zooms).", None, View, false, true;
}

pub fn spec(id: CommandId) -> &'static CommandSpec {
    COMMANDS.iter().find(|c| c.id == id).expect("every command is registered")
}

pub const NOT_SUPPORTED: &str = "Not yet supported in CAD360";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_is_unique() {
        for (i, a) in COMMANDS.iter().enumerate() {
            assert!(COMMANDS[i + 1..].iter().all(|b| b.id != a.id), "{:?} registered twice", a.id);
        }
    }
}
