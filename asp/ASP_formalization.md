# ASP to PB Encoding Formalization

## Notation

- **Atoms**: Let $A = \{a_1, a_2, \ldots\}$ be the set of atoms in the ASP program
- **Rules**: Let $R = \{r_1, r_2, \ldots\}$ be the set of rules in the ASP program
- For a rule $r$: $heads(r)$ is the set of head atoms, $body^+(r)$ are positive body literals, $body^-(r)$ are negative body literals

**PB Constraint Notation**: Constraints are written as $\sum_i w_i \cdot l_i \geq k$ where $w_i$ are weights, $l_i$ are literals, and $k$ is the threshold. We use $\overline{x}$ to denote the negation of variable $x$.

## Rule Classification

ASP rules vary along two independent axes:

**Head type:**
- **Disjunctive rule**: Head is `h1 | h2 | ...`. If the rule fires, *at least one* head must be true.
- **Basic rule**: A disjunctive rule with exactly one head. If the rule fires, the head must be true.
- **Choice rule**: Head is `{h1; h2; ...}`. If the rule fires, each head *may* be true (independently).

Since basic rules are a special case of disjunctive rules (with $|heads(r)| = 1$), this formalization treats all non-choice rules uniformly as disjunctive rules.

**Integrity constraints** (`:- body.`) are non-choice rules with $|heads(r)| = 0$. Constraint 3 reduces to $\overline{active_{r,\text{cand}}} \geq 1$, forcing $active_r = 0$. If the body is satisfied, Constraint 2 forces $active_r = 1$, making the solver unsatisfiable—correctly rejecting models that violate the constraint.

**Body type:**
- **Basic body**: Conjunction of literals (e.g., `a, b, not c`)
- **Cardinality body**: Count constraint (e.g., `#count{a; b; c} >= 2`)
- **Weight body**: Weighted sum constraint (e.g., `#sum{1:a; 2:b; 3:c} >= 4`)

Since basic and cardinality bodies are special cases of weight bodies (with all weights = 1, and threshold = number of literals or cardinality bound respectively), this formalization treats all bodies uniformly as weight bodies.

## Weight Body Encoding

A disjunctive rule with a weight body has the form:
$$h_1 \mid \ldots \mid h_p \leftarrow \#sum\{w_1 : b_1; \ldots; w_n : b_n; w_{n+1} : \text{not } c_1; \ldots; w_m : \text{not } c_k\} \geq t$$

where $h_i$ are the head atoms, $b_i$ are positive body atoms, $c_j$ are negated body atoms, $w_i$ are weights, and $t$ is the threshold.

Let **$W_r = \sum_{i=1}^{m} w_i$** be the sum of all body weights.

Define the **falsification weight** for rule $r$:
$$F_r = W_r - t + 1$$

This is the minimum total weight of falsified literals needed to guarantee the body is not satisfied.

---

## Candidate Solver

### Variables
- $x_{\text{cand}}$ for each atom $x \in A$
- $active_{r,s,\text{cand}}$ for each rule $r \in R$ and each $s \in \{t_r, t_r+1, \ldots, W_r\}$ where $t_r$ is the threshold and $W_r = \sum w_i$ is the sum of all body weights
- $used_{r,s,\text{cand}}$ for each non-choice rule $r \in R$ and each $s \in \{t_r, \ldots, W_r\}$
- $active_{r,h,s,\text{cand}}$ for each non-choice rule $r \in R$, head $h \in heads(r)$, and each $s \in \{t_r, \ldots, W_r\}$

**Multi-level semantics**: $active_{r,s,\text{cand}}$ being true implies the body's weighted sum is at least $s$. This is a unidirectional implication: the variable may be false even when the sum is sufficient. For basic bodies (where $t_r = W_r$), there is only one level.

For conciseness, we write $active_{r,\text{cand}}$ to mean $active_{r,t_r,\text{cand}}$ (the base level).

### Constraint 1: Body Satisfaction (when active) — Base Level Only
For a rule $r$ with positive body atoms $B^+ = \{b_1, \ldots, b_n\}$ with weights $\{w_1, \ldots, w_n\}$, and negative body atoms $B^- = \{c_1, \ldots, c_k\}$ with weights $\{u_1, \ldots, u_k\}$:

$$F_r \cdot active_{r,\text{cand}} + \sum_{b_i \in B^+} w_i \cdot \overline{b_{i,\text{cand}}} + \sum_{c_j \in B^-} u_j \cdot c_{j,\text{cand}} \geq F_r$$

### Constraint 2: Body Falsification (when inactive)
For each level $s \in \{t_r, t_r+1, \ldots, W_r\}$:
$$s \cdot \overline{active_{r,s,\text{cand}}} + \sum_{b_i \in B^+} w_i \cdot b_{i,\text{cand}} + \sum_{c_j \in B^-} u_j \cdot \overline{c_{j,\text{cand}}} \geq s$$

This enforces: $active_{r,s,\text{cand}} \implies \text{body weight sum} \geq s$

### Constraint 3: Head Requirement — Non-Choice Rules Only
For a non-choice rule $r$:
$$\sum_{h \in heads(r)} h_{\text{cand}} + \overline{active_{r,\text{cand}}} \geq 1$$

If the rule is active, at least one head must be true.

### Constraint 4: Used Implies Active — Non-Choice Rules Only
For a non-choice rule $r$ and each level $s \in \{t_r, \ldots, W_r\}$:
$$\overline{used_{r,s,\text{cand}}} + active_{r,s,\text{cand}} \geq 1$$

This ensures that if a rule is "used" at level $s$ (i.e., it supports one of its heads with body weight $\geq s$), then it must be active at that level.

### Constraint 5: Head Selection — Non-Choice Rules Only
For a non-choice rule $r$ with $n = |heads(r)|$ heads and each level $s \in \{t_r, \ldots, W_r\}$:
$$\sum_{h \in heads(r)} \overline{active_{r,h,s,\text{cand}}} + used_{r,s,\text{cand}} \geq n$$

This ensures:
- If the rule is not used at level $s$, all head-activations at level $s$ must be false
- If the rule is used at level $s$, at most one head-activation can be true at level $s$

(We don't require at least one head-activation — these variables track which head a rule *supports*, not which heads are true.)

### Constraint 6: Exclusive Head — Non-Choice Rules Only (Base Level)
For a non-choice rule $r$ with $n = |heads(r)|$ heads:
$$\sum_{h \in heads(r)} \overline{h_{\text{cand}}} + (n-1) \cdot \overline{used_{r,\text{cand}}} \geq n - 1$$

(Uses base level $used_{r,\text{cand}} = used_{r,t_r,\text{cand}}$)

This ensures that if a rule is used, at most one of its heads can be true. (Combined with Constraint 5, if the rule supports head $h$, then $h$ is true and all other heads are false.)

### Constraint 7: Head Propagation — Non-Choice Rules Only (Base Level)
For each head $h$ of a non-choice rule $r$:
$$h_{\text{cand}} + \overline{active_{r,h,\text{cand}}} \geq 1$$

(Uses base level $active_{r,h,\text{cand}} = active_{r,h,t_r,\text{cand}}$)

*Note: Constraints 3–7 are omitted for choice rules. Constraints 3, 6, 7 are only at base level; Constraints 4, 5 are at each level.*

---

## Check Solver

### Variables
- $x_{\text{check}}$ for each atom $x \in A$
- $x_{\text{dim}}$ (diminished) for each atom $x \in A$
- $active_{r,\text{check}}$ for each rule $r \in R$

### Constraint 8: Subset Relationship
For each atom $x$:
$$\overline{x_{\text{check}}} + x_{\text{cand}} + \overline{x_{\text{dim}}} \geq 2$$

This enforces:
- $x_{\text{check}} \implies x_{\text{cand}}$ (check atoms must be candidate atoms)
- $x_{\text{dim}} \implies x_{\text{cand}}$ (diminished atoms must be candidate atoms)
- $x_{\text{check}} \land x_{\text{dim}}$ is false (an atom cannot be both checked and diminished)

### Constraint 9: Strict Subset
$$\sum_{x \in A} x_{\text{dim}} \geq 1$$

At least one atom must be diminished.

### Constraint 10: Reduct Body Satisfaction
If a rule is active in the candidate solver (at base level), its positive body must be satisfiable in the check (reduct) interpretation:

$$F_r \cdot \overline{active_{r,\text{cand}}} + F_r \cdot active_{r,\text{check}} + \sum_{b_i \in B^+} w_i \cdot \overline{b_{i,\text{check}}} + \sum_{c_j \in B^-} u_j \cdot c_{j,\text{cand}} \geq F_r$$

### Constraint 11: Reduct Head Implication — Non-Choice Rules Only
For a non-choice rule $r$:
$$\sum_{h \in heads(r)} h_{\text{check}} + \overline{active_{r,\text{check}}} \geq 1$$

If the rule is active in check, at least one head must be true in check.

### Constraint 12: Reduct Head Propagation — Choice Rules Only
For each head $h$ of a choice rule $r$:
$$\overline{h_{\text{cand}}} + h_{\text{check}} + \overline{active_{r,\text{check}}} \geq 1$$

If $h$ was in the candidate and the rule is active in check, then $h$ must be in check.

---

## Loop Constraints

### Constraint 13: Loop Constraints

When the candidate solver finds a solution $S_{\text{cand}}$ and the check solver finds a strict subset $S_{\text{check}} \subset S_{\text{cand}}$:

1. Compute the **unfounded set**: $U = S_{\text{cand}} \setminus S_{\text{check}}$
2. Find all **external support**: for each atom $x \in U$ and each rule $r$ where $x \in heads(r)$:
   - Let $overlap = \sum \{w_i : b_i \in body^+(r) \cap U\}$ (weight of positive body atoms in $U$)
   - Let $s = t_r + overlap$ (external support level)
   - If $s > W_r$: rule cannot provide external support (skip)
   - Let $W_\text{sat} = \sum \{w_i : b_i \in body^+(r), b_i \in S_\text{cand}\} + \sum \{u_j : c_j \in body^-(r), c_j \notin S_\text{cand}\}$
   - Define $\text{reason}_{x,r}$ based on current state:
     - If $W_\text{sat} < s$: set $\text{reason}_{x,r} = active_{r,x,s,\text{cand}}$ (non-choice) or $active_{r,s,\text{cand}}$ (choice)
     - Else if some $z \in heads(r) \setminus U$ is true: select one such $z$ at random and set $\text{reason}_{x,r} = \overline{z}$
     - Else: panic (bug — U is not unfounded)
3. Add to the candidate solver:
$$\sum_{x \in U} \overline{x_{\text{cand}}} + \sum_{\substack{x \in U,\, r : x \in heads(r) \\ s \leq W_r}} \text{reason}_{x,r} \geq 1$$

This is a simple disjunctive clause: either at least one atom in $U$ is false, or at least one external support rule is active at the appropriate level. The constraint only forces external support when ALL atoms in U would otherwise be true.

**Key insight**: For weight bodies, a rule can provide external support even if some positive body atoms are in $U$, as long as the remaining atoms can satisfy the bound. By using level $s = t_r + overlap$, we ensure the external support variable is only true when the body weight from atoms *outside* $U$ is at least $t_r$.

Repeat until the check solver returns UNSAT, indicating no unfounded set exists.

### Constraint 13 Initialization: Single-Atom Loop Constraints
As a special case, for each atom $x$ we add single-atom loop constraints upfront. For each rule $r$ with $x \in heads(r)$:
- If $x \in body^+(r)$: let $s = t_r + w_x$ (where $w_x$ is the weight of $x$ in the body)
- Otherwise: $s = t_r$ (base level)
- If $s > W_r$: skip (rule cannot provide external support for $x$)

$$\overline{x_{\text{cand}}} + \sum_{(r,s)} active_{r,x,s,\text{cand}} \geq 1$$

These are loop constraints where $U = \{x\}$, added to bootstrap supportedness without needing check solver iterations.

*Note: For choice rules, use $active_{r,s,\text{cand}}$ instead of $active_{r,x,s,\text{cand}}$.*

---

## Choice Rules

Choice rules (e.g., `{h1; h2} :- body.`) differ from non-choice (disjunctive) rules:
- **Omit Constraints 3–7** (head requirement, used, head selection, exclusive head, and head propagation in candidate solver)
- **Use Constraint 12 instead of 11** (head propagation instead of head implication in check solver)
- No $used_{r,\text{cand}}$ or $active_{r,h,\text{cand}}$ variables needed — choice rules don't require any head to be true
- Loop constraints use $active_{r,\text{cand}}$ directly (not head-specific)

---

## Example

Consider the rule:
```
h :- #sum{1:b1; 2:b2; 3:b3; 4:not b4} >= 3.
```

Here: $B^+ = \{b1, b2, b3\}$ with weights $\{1, 2, 3\}$, $B^- = \{b4\}$ with weight $4$, threshold $t_r = 3$, sum of weights $W_r = 10$.

Falsification weight: $F_r = W_r - t_r + 1 = 10 - 3 + 1 = 8$

Variable levels: $s \in \{3, 4, 5, 6, 7, 8, 9, 10\}$ (8 levels)

**Candidate Solver Constraints:**

Constraint 1 (base level only):
$F_r \cdot active_{r,3,\text{cand}} + 1 \cdot \overline{b1_{\text{cand}}} + 2 \cdot \overline{b2_{\text{cand}}} + 3 \cdot \overline{b3_{\text{cand}}} + 4 \cdot b4_{\text{cand}} \geq F_r$

i.e., $8 \cdot active_{r,3,\text{cand}} + 1 \cdot \overline{b1_{\text{cand}}} + 2 \cdot \overline{b2_{\text{cand}}} + 3 \cdot \overline{b3_{\text{cand}}} + 4 \cdot b4_{\text{cand}} \geq 8$

Constraint 2 (for each level $s \in \{3, \ldots, 10\}$):
$s \cdot \overline{active_{r,s,\text{cand}}} + 1 \cdot b1_{\text{cand}} + 2 \cdot b2_{\text{cand}} + 3 \cdot b3_{\text{cand}} + 4 \cdot \overline{b4_{\text{cand}}} \geq s$

Constraint 3 (head requirement, $n=1$, base level):
$h_{\text{cand}} + \overline{active_{r,3,\text{cand}}} \geq 1$

Constraint 4 (used implies active, for each level $s$):
$\overline{used_{r,s,\text{cand}}} + active_{r,s,\text{cand}} \geq 1$

Constraint 5 (head selection, $n=1$, for each level $s$):
$\overline{active_{r,h,s,\text{cand}}} + used_{r,s,\text{cand}} \geq 1$

Constraint 6 (exclusive head, $n=1$, base level):
$\overline{h_{\text{cand}}} \geq 0$ (trivially satisfied when $n=1$)

Constraint 7 (head propagation, base level):
$h_{\text{cand}} + \overline{active_{r,h,3,\text{cand}}} \geq 1$

**Check Solver Constraints:**

Constraint 10 (uses base level):
$F_r \cdot \overline{active_{r,3,\text{cand}}} + F_r \cdot active_{r,\text{check}} + 1 \cdot \overline{b1_{\text{check}}} + 2 \cdot \overline{b2_{\text{check}}} + 3 \cdot \overline{b3_{\text{check}}} + 4 \cdot b4_{\text{check}} \geq F_r$

i.e., $8 \cdot \overline{active_{r,3,\text{cand}}} + 8 \cdot active_{r,\text{check}} + 1 \cdot \overline{b1_{\text{check}}} + 2 \cdot \overline{b2_{\text{check}}} + 3 \cdot \overline{b3_{\text{check}}} + 4 \cdot b4_{\text{check}} \geq 8$

Constraint 11 (head implication, $n=1$):
$h_{\text{check}} + \overline{active_{r,\text{check}}} \geq 1$

**Loop Constraint Example with Weight Bodies:**

Consider rule `aux :- {c, b} >= 1` with atoms $c$ (weight 1) and $b$ (weight 1), threshold $t_r = 1$, $W_r = 2$, levels $s \in \{1, 2\}$.

If $U = \{aux, b\}$ (unfounded set) but $c \notin U$:
- $overlap = w_b = 1$ (weight of $b$ which is in $U$)
- $s = t_r + overlap = 1 + 1 = 2$
- Since $s = 2 \leq W_r = 2$, the rule *can* provide external support

We add $active_{r,aux,2,\text{cand}}$ to the loop constraint. This variable is true only when the body's non-$U$ weight (from $c$ alone) satisfies threshold 1, which it does when $c$ is true.

Compare to the old (buggy) behavior: since $b \in body^+(r) \cap U \neq \emptyset$, the rule was incorrectly marked as INTERNAL and excluded from external support.
