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

**Integrity constraints** (`:- body.`) are non-choice rules with $|heads(r)| = 0$. Constraint 3 reduces to $\overline{active_r} \geq 1$, forcing $active_r = 0$. If the body is satisfied, Constraint 2 forces $active_r = 1$, causing UNSAT—correctly rejecting models that violate the constraint.

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
- $active_r$ for each rule $r \in R$

### Constraint 1: Body Satisfaction (when active)
For a rule $r$ with positive body atoms $B^+ = \{b_1, \ldots, b_n\}$ with weights $\{w_1, \ldots, w_n\}$, and negative body atoms $B^- = \{c_1, \ldots, c_k\}$ with weights $\{u_1, \ldots, u_k\}$:

$$F_r \cdot active_r + \sum_{b_i \in B^+} w_i \cdot \overline{b_{i,\text{cand}}} + \sum_{c_j \in B^-} u_j \cdot c_{j,\text{cand}} \geq F_r$$

### Constraint 2: Body Falsification (when inactive)
$$t_r \cdot \overline{active_r} + \sum_{b_i \in B^+} w_i \cdot b_{i,\text{cand}} + \sum_{c_j \in B^-} u_j \cdot \overline{c_{j,\text{cand}}} \geq t_r$$

This enforces: $active_r \iff \text{body is satisfied}$

### Constraint 3: Head Requirement — Non-Choice Rules Only
For a non-choice rule $r$:
$$\sum_{h \in heads(r)} h_{\text{cand}} + \overline{active_r} \geq 1$$

If the rule is active, at least one head must be true.

*Note: Constraints 1–3 are the only candidate solver constraints. Choice rules omit Constraint 3.*

---

## Check Solver

### Variables
- $x_{\text{check}}$ for each atom $x \in A$
- $x_{\text{dim}}$ (diminished) for each atom $x \in A$
- $active_{r,\text{check}}$ for each rule $r \in R$

### Constraint 4: Subset Relationship
For each atom $x$:
$$\overline{x_{\text{check}}} + x_{\text{cand}} + \overline{x_{\text{dim}}} \geq 2$$

This enforces:
- $x_{\text{check}} \implies x_{\text{cand}}$ (check atoms must be candidate atoms)
- $x_{\text{dim}} \implies x_{\text{cand}}$ (diminished atoms must be candidate atoms)
- $x_{\text{check}} \land x_{\text{dim}}$ is false (an atom cannot be both checked and diminished)

### Constraint 5: Strict Subset
$$\sum_{x \in A} x_{\text{dim}} \geq 1$$

At least one atom must be diminished.

### Constraint 6: Reduct Body Satisfaction
If a rule is active in the candidate solver, its body must be satisfiable in the check (reduct) interpretation:

$$F_r \cdot \overline{active_r} + F_r \cdot active_{r,\text{check}} + \sum_{b_i \in B^+} w_i \cdot \overline{b_{i,\text{check}}} + \sum_{c_j \in B^-} u_j \cdot c_{j,\text{check}} \geq F_r$$

### Constraint 7: Reduct Head Implication — Non-Choice Rules Only
For a non-choice rule $r$:
$$\sum_{h \in heads(r)} h_{\text{check}} + \overline{active_{r,\text{check}}} \geq 1$$

If the rule is active in check, at least one head must be true in check.

### Constraint 8: Reduct Head Propagation — Choice Rules Only
For each head $h$ of a choice rule $r$:
$$\overline{h_{\text{cand}}} + h_{\text{check}} + \overline{active_{r,\text{check}}} \geq 1$$

If $h$ was in the candidate and the rule is active in check, then $h$ must be in check.

---

## Loop Constraints

### Constraint 9: Loop Constraints

When the candidate solver finds a solution $S_{\text{cand}}$ and the check solver finds a strict subset $S_{\text{check}} \subset S_{\text{cand}}$:

1. Compute the **unfounded set**: $U = S_{\text{cand}} \setminus S_{\text{check}}$
2. Add a loop constraint to the candidate solver (see below)
3. Repeat until the check solver returns UNSAT

**Loop constraint structure:**

Let $k = |U|$ be the size of the unfounded set. The loop constraint is a PB constraint:

$$\sum_{r} k \cdot reason_r + \sum_{x \in U} 1 \cdot \overline{x_{\text{cand}}} \geq k$$

This means:
- If ANY reason literal is true → constraint satisfied (contributes $k$)
- If NO reason is true → ALL $k$ UFS atoms must be false

**Selecting reason literals for a rule $r$ with $x \in heads(r)$:**

Let $W_{\notin U}$ be the sum of weights of all positive body literals whose atoms are NOT in $U$.

1. If $W_{\notin U} < t_r$: skip this rule (external support is impossible)

2. Otherwise, if some head $z \in heads(r)$ with $z \notin U$: add $\overline{z}$ to the clause

3. Otherwise: collect all positive body literals $b$ where $b \notin U$ and $b$ is currently FALSE. Shuffle them (seeded RNG for determinism). Add them one-by-one to the clause until their weights sum to at least $W_{\notin U} - t_r + 1$.

4. If step 3 exhausts all FALSE non-UFS body literals without reaching the threshold: panic (bug)

---

## Example

Consider the rule:
```
h :- #sum{1:b1; 2:b2; 3:b3; 4:not b4} >= 3.
```

Here: $B^+ = \{b1, b2, b3\}$ with weights $\{1, 2, 3\}$, $B^- = \{b4\}$ with weight $4$, threshold $t_r = 3$, sum of weights $W_r = 10$.

Falsification weight: $F_r = W_r - t_r + 1 = 10 - 3 + 1 = 8$

**Candidate Solver Constraints:**

Constraint 1:
$8 \cdot active_r + 1 \cdot \overline{b1_{\text{cand}}} + 2 \cdot \overline{b2_{\text{cand}}} + 3 \cdot \overline{b3_{\text{cand}}} + 4 \cdot b4_{\text{cand}} \geq 8$

Constraint 2:
$3 \cdot \overline{active_r} + 1 \cdot b1_{\text{cand}} + 2 \cdot b2_{\text{cand}} + 3 \cdot b3_{\text{cand}} + 4 \cdot \overline{b4_{\text{cand}}} \geq 3$

Constraint 3 (head requirement, single head):
$h_{\text{cand}} + \overline{active_r} \geq 1$

**Check Solver Constraints:**

Constraint 6:
$8 \cdot \overline{active_r} + 8 \cdot active_{r,\text{check}} + 1 \cdot \overline{b1_{\text{check}}} + 2 \cdot \overline{b2_{\text{check}}} + 3 \cdot \overline{b3_{\text{check}}} + 4 \cdot b4_{\text{check}} \geq 8$

Constraint 7 (head implication, single head):
$h_{\text{check}} + \overline{active_{r,\text{check}}} \geq 1$

---

## Disjunctive Rule Example

Consider: `a | b :- c.`

**Candidate constraints:**
- Constraint 1: $active_r + \overline{c} \geq 1$ (if active, c must be true)
- Constraint 2: $\overline{active_r} + c \geq 1$ (if c true, rule is active)
- Constraint 3: $a + b + \overline{active_r} \geq 1$ (if active, a or b must be true)

**Runtime UFS example:**

Suppose candidate model is $\{a, b, c\}$ and UFS = $\{a\}$ (so $b$ is true but not in UFS):

For the rule `a | b :- c`:
- Body is satisfied (c is true)
- But $b \in heads(r)$, $b \notin U$, and $b$ is true
- So this rule supports $b$, not $a$
- Reason literal: $\overline{b}$

Loop constraint added:
$$\overline{a} \lor \overline{b}$$

This says: "either $a$ is false, or $b$ is false." Since we want $a$ and $b$ is stealing support, we need $b$ to be false for $a$ to be supported by this rule.

**Another scenario:**

Suppose candidate model is $\{a, c\}$ and UFS = $\{a\}$ (so $b$ is false):

For the rule `a | b :- c`:
- Body is satisfied (c is true)
- $b$ is false, so it's not stealing support
- This rule CAN provide external support for $a$
- No reason literal needed from this rule

If there's no other support for $a$, the body literal would be added:
$$\overline{a} \lor \overline{c}$$

This says: "either $a$ is false, or $c$ is false (so the rule doesn't fire)."
