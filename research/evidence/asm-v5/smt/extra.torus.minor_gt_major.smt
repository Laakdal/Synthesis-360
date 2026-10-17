23200 0 2 2           
16 Autodesk Neutron 20 ASM 232.5.0.65535 NT 24 Sat Oct 17 15:39:51 2026 
10 9.999999999999999547e-07 1.000000000000000036e-10 
asmheader $-1 -1 @13 232.5.0.65535 #
body $-1 -1 $-1 $2 $-1 $3 #
lump $-1 -1 $-1 $-1 $4 $1 #
transform $-1 -1 1 0 0 0 1 0 0 0 1 0 0 0 1 no_rotate no_reflect no_shear #
shell $-1 0 $-1 $-1 $-1 $5 $-1 $2 #
face $6 1 $-1 $-1 $7 $4 $-1 $8 forward single #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $-1 $5 @22 generic_tag_attrib_def 3 3 -1 @23 generic_tag_attrib_def  1 1 @1 1 0 0 0 #
loop $-1 2 $-1 $9 $10 $5 #
torus-surface $-1 -1 $-1 0 0 0 0 0 1 0.5 2 1 0 0 forward_v I I I I #
loop $-1 3 $-1 $-1 $11 $5 #
coedge $-1 6 $-1 $10 $10 $-1 $12 reversed $7 0 $-1 #
coedge $-1 9 $-1 $11 $11 $-1 $13 reversed $9 0 $-1 #
edge $14 4 $-1 $15 1 $15 0 $10 $-1 forward @7 unknown #
edge $16 7 $-1 $17 1 $17 0 $11 $-1 forward @7 unknown #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $-1 $12 @22 generic_tag_attrib_def 3 3 -1 @23 generic_tag_attrib_def  1 1 @1 2 0 0 0 #
vertex $-1 5 $-1 $12 0 $18 #
ATTRIB_CUSTOM-attrib $-1 -1 $-1 $-1 $13 @22 generic_tag_attrib_def 3 3 -1 @23 generic_tag_attrib_def  1 1 @1 3 0 0 0 #
vertex $-1 8 $-1 $13 0 $19 #
point $-1 -1 $-1 0 0 1.936491673103708511 #
point $-1 -1 $-1 0 0 -1.936491673103708511 #
End-of-ASM-data 