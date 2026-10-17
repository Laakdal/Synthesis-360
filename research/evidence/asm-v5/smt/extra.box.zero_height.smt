23200 0 2 2           
16 Autodesk Neutron 20 ASM 232.5.0.65535 NT 24 Sat Oct 17 15:39:46 2026 
10 9.999999999999999547e-07 1.000000000000000036e-10 
asmheader $-1 -1 @13 232.5.0.65535 #
body $-1 -1 $-1 $2 $-1 $3 #
lump $-1 -1 $-1 $-1 $4 $1 #
transform $-1 -1 1 0 0 0 1 0 0 0 1 0 0 0 1 no_rotate no_reflect no_shear #
shell $-1 0 $-1 $-1 $-1 $5 $-1 $2 #
face $6 1 $-1 $-1 $7 $4 $-1 $8 reversed double out #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $-1 $5 @22 generic_tag_attrib_def 3 3 -1 @23 generic_tag_attrib_def  1 1 @1 1 0 0 0 #
loop $-1 2 $-1 $-1 $9 $5 #
plane-surface $-1 -1 $-1 0.5 0.5 0 0 0 1 1 0 0 forward_v I I I I #
coedge $-1 6 $-1 $10 $11 $-1 $12 forward $7 0 $-1 #
coedge $-1 9 $-1 $13 $9 $-1 $14 forward $7 0 $-1 #
coedge $-1 14 $-1 $9 $13 $-1 $15 forward $7 0 $-1 #
edge $16 3 $-1 $17 -0.5 $18 0.5 $9 $19 forward @7 unknown #
coedge $-1 12 $-1 $11 $10 $-1 $20 forward $7 0 $-1 #
edge $21 7 $-1 $18 -0.5 $22 0.5 $10 $23 forward @7 unknown #
edge $24 13 $-1 $25 -0.5 $17 0.5 $11 $26 forward @7 unknown #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $-1 $12 @22 generic_tag_attrib_def 3 3 -1 @23 generic_tag_attrib_def  1 1 @1 2 0 0 0 #
vertex $-1 4 $-1 $12 0 $27 #
vertex $-1 5 $-1 $12 1 $28 #
straight-curve $-1 -1 $-1 1 0.5 0 0 -1 0 I I #
edge $29 10 $-1 $22 -0.5 $25 0.5 $13 $30 forward @7 unknown #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $-1 $14 @22 generic_tag_attrib_def 3 3 -1 @23 generic_tag_attrib_def  1 1 @1 3 0 0 0 #
vertex $-1 8 $-1 $20 0 $31 #
straight-curve $-1 -1 $-1 0.5 0 0 -1 0 0 I I #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $-1 $15 @22 generic_tag_attrib_def 3 3 -1 @23 generic_tag_attrib_def  1 1 @1 5 0 0 0 #
vertex $-1 11 $-1 $15 0 $32 #
straight-curve $-1 -1 $-1 0.5 1 0 1 0 0 I I #
point $-1 -1 $-1 1 1 0 #
point $-1 -1 $-1 1 0 0 #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $-1 $20 @22 generic_tag_attrib_def 3 3 -1 @23 generic_tag_attrib_def  1 1 @1 4 0 0 0 #
straight-curve $-1 -1 $-1 0 0.5 0 0 1 0 I I #
point $-1 -1 $-1 0 0 0 #
point $-1 -1 $-1 0 1 0 #
End-of-ASM-data 