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

Define the **falsification weight** for rule $r$:
$$W_r = \left(\sum_{i=1}^{m} w_i\right) - t + 1$$

This is the minimum total weight of falsified literals needed to guarantee the body is not satisfied.

---

## Candidate Solver

### Variables
- $x_{\text{cand}}$ for each atom $x \in A$
- $active_{r,\text{cand}}$ for each rule $r \in R$
- $used_{r,\text{cand}}$ for each non-choice rule $r \in R$
- $active_{r,h,\text{cand}}$ for each non-choice rule $r \in R$ and head $h \in heads(r)$

### Constraint 1: Body Satisfaction (when active)
For a rule $r$ with positive body atoms $B^+ = \{b_1, \ldots, b_n\}$ with weights $\{w_1, \ldots, w_n\}$, and negative body atoms $B^- = \{c_1, \ldots, c_k\}$ with weights $\{u_1, \ldots, u_k\}$:

$$W_r \cdot active_{r,\text{cand}} + \sum_{b_i \in B^+} w_i \cdot \overline{b_{i,\text{cand}}} + \sum_{c_j \in B^-} u_j \cdot c_{j,\text{cand}} \geq W_r$$

### Constraint 2: Body Falsification (when inactive)
$$t \cdot \overline{active_{r,\text{cand}}} + \sum_{b_i \in B^+} w_i \cdot b_{i,\text{cand}} + \sum_{c_j \in B^-} u_j \cdot \overline{c_{j,\text{cand}}} \geq t$$

### Constraint 3: Head Requirement — Non-Choice Rules Only
For a non-choice rule $r$:
$$\sum_{h \in heads(r)} h_{\text{cand}} + \overline{active_{r,\text{cand}}} \geq 1$$

If the rule is active, at least one head must be true.

### Constraint 4: Used Implies Active — Non-Choice Rules Only
For a non-choice rule $r$:
$$\overline{used_{r,\text{cand}}} + active_{r,\text{cand}} \geq 1$$

This ensures that if a rule is "used" (i.e., it supports one of its heads), then it must be active.

### Constraint 5: Head Selection — Non-Choice Rules Only
For a non-choice rule $r$ with $n = |heads(r)|$ heads:
$$\sum_{h \in heads(r)} \overline{active_{r,h,\text{cand}}} + used_{r,\text{cand}} \geq n$$

This ensures:
- If the rule is not used, all head-activations must be false
- If the rule is used, at most one head-activation can be true

(We don't require at least one head-activation — these variables track which head a rule *supports*, not which heads are true.)

### Constraint 6: Exclusive Head — Non-Choice Rules Only
For a non-choice rule $r$ with $n = |heads(r)|$ heads:
$$\sum_{h \in heads(r)} \overline{h_{\text{cand}}} + (n-1) \cdot \overline{used_{r,\text{cand}}} \geq n - 1$$

This ensures that if a rule is used, at most one of its heads can be true. (Combined with Constraint 5, if the rule supports head $h$, then $h$ is true and all other heads are false.)

### Constraint 7: Head Propagation — Non-Choice Rules Only
For each head $h$ of a non-choice rule $r$:
$$h_{\text{cand}} + \overline{active_{r,h,\text{cand}}} \geq 1$$

*Note: Constraints 3–7 are omitted for choice rules.*

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
If a rule is active in the candidate solver, its body must be satisfiable in the check (reduct) interpretation:

$$W_r \cdot \overline{active_{r,\text{cand}}} + W_r \cdot active_{r,\text{check}} + \sum_{b_i \in B^+} w_i \cdot \overline{b_{i,\text{check}}} + \sum_{c_j \in B^-} u_j \cdot c_{j,\text{check}} \geq W_r$$

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
2. Find **external support**: for each atom $x \in U$, find all rules $r$ where $x \in heads(r)$ but $body^+(r) \cap U = \emptyset$
3. Add to the candidate solver (where $n = |U|$):
$$\sum_{x \in U} \overline{x_{\text{cand}}} + \sum_{(r,x) \in \text{external}} n \cdot active_{r,x,\text{cand}} \geq n$$

where "external" is the set of $(r, x)$ pairs such that $x \in U$, $x \in heads(r)$, and $body^+(r) \cap U = \emptyset$.

This ensures that if ANY atom in U is true, there must be external support. Without external support, ALL atoms in U must be false.

*Note: For choice rules, use $active_{r,\text{cand}}$ instead of $active_{r,x,\text{cand}}$.*

Repeat until the check solver returns UNSAT, indicating no unfounded set exists.

### Constraint 13 Initialization: Single-Atom Loop Constraints
As a special case, for each atom $x$ we add single-atom loop constraints upfront:
$$\overline{x_{\text{cand}}} + \sum_{r : x \in heads(r) \land x \notin body^+(r)} active_{r,x,\text{cand}} \geq 1$$

These are loop constraints where $U = \{x\}$, added to bootstrap supportedness without needing check solver iterations.

*Note: For choice rules, use $active_{r,\text{cand}}$ instead of $active_{r,x,\text{cand}}$.*

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

Here: $B^+ = \{b1, b2, b3\}$ with weights $\{1, 2, 3\}$, $B^- = \{b4\}$ with weight $4$, threshold $t = 3$.

Falsification weight: $W_r = (1 + 2 + 3 + 4) - 3 + 1 = 8$

**Candidate Solver Constraints:**

Constraint 1:
$8 \cdot active_{r,\text{cand}} + 1 \cdot \overline{b1_{\text{cand}}} + 2 \cdot \overline{b2_{\text{cand}}} + 3 \cdot \overline{b3_{\text{cand}}} + 4 \cdot b4_{\text{cand}} \geq 8$

Constraint 2:
$3 \cdot \overline{active_{r,\text{cand}}} + 1 \cdot b1_{\text{cand}} + 2 \cdot b2_{\text{cand}} + 3 \cdot b3_{\text{cand}} + 4 \cdot \overline{b4_{\text{cand}}} \geq 3$

Constraint 3 (head requirement, $n=1$):
$h_{\text{cand}} + \overline{active_{r,\text{cand}}} \geq 1$

Constraint 4 (used implies active):
$\overline{used_{r,\text{cand}}} + active_{r,\text{cand}} \geq 1$

Constraint 5 (head selection, $n=1$):
$\overline{active_{r,h,\text{cand}}} + used_{r,\text{cand}} \geq 1$

Constraint 6 (exclusive head, $n=1$):
$\overline{h_{\text{cand}}} \geq 0$ (trivially satisfied when $n=1$)

Constraint 7 (head propagation):
$h_{\text{cand}} + \overline{active_{r,h,\text{cand}}} \geq 1$

**Check Solver Constraints:**

Constraint 10:
$8 \cdot \overline{active_{r,\text{cand}}} + 8 \cdot active_{r,\text{check}} + 1 \cdot \overline{b1_{\text{check}}} + 2 \cdot \overline{b2_{\text{check}}} + 3 \cdot \overline{b3_{\text{check}}} + 4 \cdot b4_{\text{check}} \geq 8$

Constraint 11 (head implication, $n=1$):
$h_{\text{check}} + \overline{active_{r,\text{check}}} \geq 1$

**Loop Constraint Example:**

If candidate solution is $\{w, x, y, z\}$ and check solution is $\{w, x\}$, the unfounded set is $\{y, z\}$.

Suppose $r_1$ has $y$ in its head, $r_2$ has $z$ in its head, and $r_3$ has both $y$ and $z$ in its heads, and none have $y$ or $z$ in their positive bodies. The external support pairs are $(r_1, y)$, $(r_2, z)$, $(r_3, y)$, $(r_3, z)$, so we add (with $n = |U| = 2$):
$$\overline{y_{\text{cand}}} + \overline{z_{\text{cand}}} + 2 \cdot active_{r_1,y,\text{cand}} + 2 \cdot active_{r_2,z,\text{cand}} + 2 \cdot active_{r_3,y,\text{cand}} + 2 \cdot active_{r_3,z,\text{cand}} \geq 2$$
