CAD360 @VERSION@ for Windows (64-bit)
=====================================

CAD360 is a native, Fusion 360-inspired parametric CAD application: constrained 2D sketches,
B-rep solid modelling on Open CASCADE Technology, an editable feature timeline, STEP/STL
exchange. It runs locally; no account, browser or network connection is needed.

CAD360 is an independent implementation. It is not affiliated with Autodesk and contains no
Autodesk code or assets.

Getting started
---------------
1. Extract the whole folder anywhere (for example to Documents\CAD360). Nothing is installed.
2. Run cad360.exe.
3. Open samples\mounting-plate.c360 (File > Open, or drag it onto cad360.exe) to see a design
   with a dimensioned sketch, an extrusion, fillets and a slot cut. Double-click a feature in
   the timeline to edit it; Modify > Change Parameters edits the dimensions.

Command line
------------
  cad360.exe [design.c360]
  cad360.exe --scenario steps.json [--screenshot-dir DIR]   (scripted automation / testing)

System requirements
-------------------
* Windows 10 or 11, 64-bit.
* A GPU with Direct3D 12 or Vulkan support (integrated graphics are fine).
* @RUNTIME_NOTE@

Files
-----
  cad360.exe             the application
  samples\               example designs (.c360)
  LICENSE.txt            CAD360 licence
  THIRD-PARTY-NOTICES\   licences of Open CASCADE Technology and the Rust libraries used
  SHA256SUMS.txt         checksums of the files in this folder

Source code
-----------
CAD360's source code, including the modified Open CASCADE bindings it is built from, is
available at @SOURCE_URL@ (see LICENSE.txt and THIRD-PARTY-NOTICES\OpenCASCADE-NOTICE.txt).
