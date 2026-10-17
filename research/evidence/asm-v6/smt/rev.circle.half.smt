23200 0 2 2           
16 Autodesk Neutron 20 ASM 232.5.0.65535 NT 24 Sat Oct 17 19:09:11 2026 
10 9.999999999999999547e-07 1.000000000000000036e-10 
asmheader $-1 -1 @13 232.5.0.65535 #
body $2 -1 $-1 $3 $-1 $4 #
ATTRIB_CUSTOM-attrib $-1 -1 $5 $-1 $1 @20 Timestamp_attrib_def 1 1792238950932136 #
lump $-1 -1 $-1 $-1 $6 $1 #
transform $-1 -1 1 0 0 0 1 0 0 0 1 0 0 0 1 no_rotate no_reflect no_shear #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $2 $1 @22 generic_tag_attrib_def 3 3 -1 @23 generic_tag_attrib_def  1 3 @3 301 4 0 0 #
shell $-1 -1 $-1 $-1 $-1 $7 $-1 $3 #
face $8 -1 $-1 $9 $10 $6 $-1 $11 forward single #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $-1 $7 @22 generic_tag_attrib_def 3 3 -1 @23 generic_tag_attrib_def  1 1 @1 3 0 1 301 0 #
face $12 -1 $-1 $13 $14 $6 $-1 $15 forward single #
loop $-1 -1 $-1 $16 $17 $7 #
torus-surface $-1 -1 $-1 0 0 0 0 0 1 2 0.5 1 0 0 forward_v I I I I #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $-1 $9 @22 generic_tag_attrib_def 3 3 -1 @23 generic_tag_attrib_def  1 1 @1 2 0 1 301 0 #
face $18 -1 $-1 $-1 $19 $6 $-1 $20 reversed single #
loop $-1 -1 $-1 $-1 $21 $9 #
plane-surface $-1 -1 $-1 -2 2.449293598294706414e-16 0 -1.224646799147353207e-16 -1 0 -1 1.224646799147353207e-16 0 forward_v I I I I #
loop $-1 -1 $-1 $-1 $22 $7 #
coedge $-1 -1 $-1 $17 $17 $21 $23 reversed $10 0 $-1 #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $-1 $13 @22 generic_tag_attrib_def 3 3 -1 @23 generic_tag_attrib_def  1 1 @1 1 0 1 301 0 #
loop $-1 -1 $-1 $-1 $24 $13 #
plane-surface $-1 -1 $-1 2 0 0 0 1 0 1 0 0 forward_v I I I I #
coedge $-1 -1 $-1 $21 $21 $17 $23 forward $14 0 $-1 #
coedge $25 -1 $-1 $22 $22 $24 $26 forward $16 0 $-1 #
edge $27 -1 $-1 $28 -3.141592653589793116 $28 3.141592653589793116 $17 $29 forward @7 unknown #
coedge $30 -1 $-1 $24 $24 $22 $26 reversed $19 0 $-1 #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $-1 $22 @17 sketch_attrib_def 1 1 3 @13 102 0 0 0 1 1 #
edge $31 -1 $-1 $32 -3.141592653589793116 $32 3.141592653589793116 $22 $33 forward @7 unknown #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $-1 $23 @22 generic_tag_attrib_def 3 3 -1 @23 generic_tag_attrib_def  1 2 @2 -1 0 1 301 0 #
vertex $-1 -1 $-1 $23 2 $34 #
ellipse-curve $-1 -1 $-1 -2 2.449293598294706414e-16 0 -1.224646799147353207e-16 -1 0 -0.5 6.123233995736766036e-17 0 1 I I #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $-1 $24 @17 sketch_attrib_def 1 1 3 @13 102 0 0 0 1 1 #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $-1 $26 @22 generic_tag_attrib_def 3 3 -1 @23 generic_tag_attrib_def  1 4 @4 -102 0 1 301 0 #
vertex $-1 -1 $-1 $26 0 $35 #
ellipse-curve $-1 -1 $-1 2 0 0 0 1 0 0.5 0 0 1 I I #
point $-1 -1 $-1 -1.5 1.836970198721029934e-16 5.551115123125782702e-17 #
point $-1 -1 $-1 1.5 0 6.123233995736766036e-17 #
End-of-ASM-data 