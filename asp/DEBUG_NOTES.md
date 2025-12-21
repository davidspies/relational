# ASP Solver Bug Investigation

## Problem
Our solver finds 7 of 9 sorting network models that clasp finds. Missing 2 models.

## Root Cause Identified
**Constraint 150** (a loop/UFS constraint) incorrectly blocks clasp answer #6.

## Verification Results

### Clasp answer #6
- 203 atoms
- Is a valid stable model (verified by adding integrity constraints and running clasp → SATISFIABLE)
- **VIOLATED by our constraint #150**

### Clasp answer #1 (which we do find)
- 184 atoms
- All 158 of our UFS constraints are satisfied

## Constraint 150 Details

The constraint that blocks clasp answer #6:

```
Constraint 150: ¬Atom(152) + 17 active_head variables >= 1
Terms: [(Lit(-152), 1), (Lit(2254), 1), (Lit(2258), 1), (Lit(2324), 1), (Lit(2262), 1),
        (Lit(2321), 1), (Lit(2325), 1), (Lit(2256), 1), (Lit(2320), 1), (Lit(2285), 1),
        (Lit(2289), 1), (Lit(2298), 1), (Lit(2305), 1), (Lit(2414), 1), (Lit(2429), 1),
        (Lit(2441), 1), (Lit(2485), 1), (Lit(2505), 1)]
Size: 18 terms
```

BUT when checking clasp answer #6, the verification shows it's **constraint 150 with Atom(216)** that's violated:

```
UFS Constraint 150 VIOLATED: sum=0 < bound=1
  Lit(-216): Cand(Atom(216)), value=true, lit_sat=false, weight=1
  [17 active_head variables all false]
```

## Why Constraint 150 Violates Clasp Answer #6

In clasp answer #6:
- Atom(216) is TRUE (so ¬Atom(216) contributes 0)
- All 17 active_head variables are FALSE (contribute 0)
- Sum = 0 < bound = 1 → VIOLATED

The constraint says: "Either Atom(216) is false, OR at least one rule that derives it must be active."

But in clasp's valid answer #6, Atom(216) is true and none of the listed rules are "active" (by our definition).

## Key Question

Is our loop constraint too strong? The constraint requires that if an atom is true, at least one supporting rule must be "active" (have active_head set). But clasp thinks this model is valid.

Possible issues:
1. Our definition of "active_head" is too restrictive
2. Our unfounded set computation is including atoms that shouldn't be there
3. The loop constraint generation is wrong

## Relevant Rules for Atom(216)

From verification output:
```
Rule 247: Disjunctive(DisjunctiveRule {
  heads: [Atom(216)],
  body: [WeightedLit { atom: Atom(213), positive: true, weight: 1 },
         WeightedLit { atom: Atom(214), positive: true, weight: 1 },
         WeightedLit { atom: Atom(215), positive: true, weight: 1 }],
  bound: 1
})
  body_in_u: true
  body_satisfied_in_target: true
```

So the body IS satisfied in the target model, but `body_in_u: true` means the body atoms are in the unfounded set.

## Files/Commands for Reproduction

```bash
# Verify clasp answer 6 against our constraints
python3 asp/verify_constraint.py asp/sorting_network.lp 6

# Verify clasp answer 1 (should pass)
python3 asp/verify_constraint.py asp/sorting_network.lp 1

# Get constraint 150 details
gringo --output=smodels asp/sorting_network.lp | ASP_DUMP_CONSTRAINT=150 ./target/release/asp 0 2>&1 | grep -E "CONSTRAINT|CANDIDATE"

# Test if candidate model at constraint 150 is a valid stable model
python3 asp/test_candidate_model.py asp/sorting_network.lp 150
```

## Next Steps

1. Investigate why rule 247's body being "in U" (unfounded set) causes the constraint to block a valid model
2. Check if our active_head expansion is correct
3. Compare our UFS computation with what clasp would compute
