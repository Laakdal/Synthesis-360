23200 0 2 2           
16 Autodesk Neutron 20 ASM 232.5.0.65535 NT 24 Sat Oct 17 19:08:56 2026 
10 9.999999999999999547e-07 1.000000000000000036e-10 
asmheader $-1 -1 @13 232.5.0.65535 #
body $2 -1 $-1 $3 $-1 $4 #
ATTRIB_CUSTOM-attrib $-1 -1 $5 $-1 $1 @20 Timestamp_attrib_def 1 1792238936018429 #
lump $-1 -1 $-1 $-1 $6 $1 #
transform $-1 -1 1 0 0 0 1 0 0 0 1 0 0 0 1 no_rotate no_reflect no_shear #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $2 $1 @22 generic_tag_attrib_def 3 3 -1 @23 generic_tag_attrib_def  1 3 @3 301 6 0 0 #
shell $-1 -1 $-1 $-1 $-1 $7 $-1 $3 #
face $8 -1 $-1 $9 $10 $6 $-1 $11 forward single #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $-1 $7 @22 generic_tag_attrib_def 3 3 -1 @23 generic_tag_attrib_def  1 1 @1 5 0 1 301 0 #
face $12 -1 $-1 $13 $14 $6 $-1 $15 forward single #
loop $-1 -1 $-1 $-1 $16 $7 #
plane-surface $-1 -1 $-1 1 0 0 -0 -0 -1 -1 0 0 forward_v I I I I #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $-1 $9 @22 generic_tag_attrib_def 3 3 -1 @23 generic_tag_attrib_def  1 1 @1 4 0 1 301 0 #
face $17 -1 $-1 $-1 $18 $6 $-1 $19 forward single #
loop $-1 -1 $-1 $20 $21 $9 #
cone-surface $-1 -1 $-1 0 0 1 0 0 1 1 0 0 1 I I 0 1 1 forward I I I I #
coedge $-1 -1 $-1 $16 $16 $21 $22 reversed $10 0 $-1 #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $-1 $13 @22 generic_tag_attrib_def 3 3 -1 @23 generic_tag_attrib_def  1 1 @1 3 0 1 301 0 #
loop $-1 -1 $-1 $-1 $23 $13 #
plane-surface $-1 -1 $-1 0 0 2 0 0 1 1 0 0 forward_v I I I I #
loop $-1 -1 $-1 $-1 $24 $9 #
coedge $-1 -1 $-1 $21 $21 $16 $22 forward $14 0 $-1 #
edge $-1 -1 $-1 $25 0 $25 6.283185307179586232 $21 $26 forward @7 unknown #
coedge $-1 -1 $-1 $23 $23 $24 $27 forward $18 0 $-1 #
coedge $-1 -1 $-1 $24 $24 $23 $27 reversed $20 0 $-1 #
vertex $-1 -1 $-1 $22 2 $28 #
ellipse-curve $-1 -1 $-1 0 0 0 0 0 1 1 0 0 1 I I #
edge $-1 -1 $-1 $29 0 $29 6.283185307179586232 $23 $30 forward @7 unknown #
point $-1 -1 $-1 1 0 0 #
vertex $-1 -1 $-1 $27 2 $31 #
ellipse-curve $-1 -1 $-1 0 0 2 0 0 1 1 0 0 1 I I #
point $-1 -1 $-1 1 0 2 #
End-of-ASM-data 