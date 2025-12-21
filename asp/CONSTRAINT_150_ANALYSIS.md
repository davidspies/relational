# Constraint 150 Analysis

## Overview

Constraint 150 is a loop constraint for the sorting network problem.
It has 18 terms with bound ≥ 1.

**Constraint**: ¬Atom(152) + 17 active_head variables ≥ 1

**Meaning**: Either Atom(152) is false, OR at least one of the 17 rules must be "active" (providing external support).

---

## Term 1: The Chosen Atom

**¬Atom(152)** = ¬__atom_152

Traced derivation:
```
152 :- 268
268 :- #1{265, 266, 267}
265 :- comp(3,0,3), val(2,0,1)
266 :- comp(3,2,3), val(2,2,1)
267 :- comp(3,1,3), val(2,1,1)
```

**Meaning**: Wire 3 gets value 1 at layer 3 via a comparator (I,3) where wire I had value 1 at layer 2.

---

## Terms 2-13: Level 1 Active Head Variables

These rules have single-atom bodies (bound=1). Level=1 because no body atoms are in the UFS.

| Term | Rule | Head Atom | Head Name | Body Atom | Body Traced |
|------|------|-----------|-----------|-----------|-------------|
| 2 | 99 | 95 | val(1,2,1) | 118 | #1{comp(1,1,2)&val(0,1,1), comp(1,0,2)&val(0,0,1)} |
| 3 | 103 | 95 | val(1,2,1) | 122 | (similar - wire 2 gets 1 at layer 1) |
| 4 | 166 | 96 | val(0,2,1) | 13 | ⊤ (choice: initial input bit) |
| 5 | 107 | 101 | val(1,1,0) | 123 | (wire 1 gets 0 at layer 1 via comp) |
| 6 | 165 | 102 | val(0,1,0) | 12 | ⊤ (choice: initial input bit) |
| 7 | 167 | 106 | val(0,3,0) | 14 | ⊤ (choice: initial input bit) |
| 8 | 101 | 107 | val(1,0,1) | 120 | (wire 0 gets 1 at layer 1 via comp) |
| 9 | 164 | 108 | val(0,0,1) | 11 | ⊤ (choice: initial input bit) |
| 10 | 130 | 110 | val(2,2,1) | 140 | (wire 2 gets 1 at layer 2 via comp) |
| 11 | 134 | 115 | val(2,1,0) | 144 | (wire 1 gets 0 at layer 2 via comp) |
| 12 | 143 | 130 | val(3,2,1) | 153 | (wire 2 gets 1 at layer 3 via comp) |
| 13 | 150 | 133 | val(3,1,0) | 160 | (wire 1 gets 0 at layer 3 via comp) |

**Note**: Terms 2 and 3 have the SAME head (val(1,2,1)) but different rules.

---

## Terms 14-18: Level 2 Active Head Variables

These rules have cardinality bodies (2-3 atoms with bound≥1). Level=2 because 1 body atom is in the UFS.

| Term | Rule | Head Atom | Body Atoms | Body Meaning |
|------|------|-----------|------------|--------------|
| 14 | 247 | 216 (__atom_216) | {213,214,215}≥1 | Wire 3 gets 0 at layer 1 via comp(1,I,3) |
| 15 | 258 | 224 (__atom_224) | {222,223}≥1 | Wire 1 gets 0 at layer 1 via comp(1,I,1) |
| 16 | 267 | 231 (__atom_231) | {229,230}≥1 | Wire 2 gets 1 at layer 2 via comp(2,I,2) |
| 17 | 302 | 256 (__atom_256) | {254,255}≥1 | Wire 1 gets 0 at layer 2 via comp(2,1,J) |
| 18 | 318 | 268 (__atom_268) | {265,266,267}≥1 | Wire 3 gets 1 at layer 3 via comp(3,I,3) |

### Detailed body atom traces:

**Rule 247 body atoms**:
- 213 = val(0,2,0) & val(0,3,0) & comp(1,2,3)
- 214 = val(0,1,0) & val(0,3,0) & comp(1,1,3)
- 215 = val(0,0,0) & val(0,3,0) & comp(1,0,3)

**Rule 318 body atoms** (same structure as Term 1):
- 265 = comp(3,0,3) & val(2,0,1)
- 266 = comp(3,2,3) & val(2,2,1)
- 267 = comp(3,1,3) & val(2,1,1)

---

## Key Observations

1. **Level 1 heads are all named val() predicates**
2. **Level 2 heads are all internal (__atom_N) auxiliary atoms**
3. **Level 2 rules have cardinality bodies** - that's why they get level=bound+overlap=1+1=2 when one body atom is in the UFS
4. **Terms 2&3 share the same head** - two different rules can derive val(1,2,1)
5. **Term 18 (Rule 318) and Term 1 (Atom 152)** are closely related - both involve wire 3 getting value 1 at layer 3

---

## The UFS (Unfounded Set)

36 atoms total:
```
[95, 96, 101, 102, 105, 106, 107, 108, 110, 115, 125, 128, 130, 133, 138, 148,
 150, 152, 155, 163, 164, 167, 168, 214, 216, 222, 224, 229, 231, 254, 256,
 260, 266, 268, 273, 290]
```

This includes many val() atoms at various layers.
