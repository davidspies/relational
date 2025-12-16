# ASP to PB Encoding Formalization

## Notation

- **Atoms**: Let $A = \{a_1, a_2, \ldots\}$ be the set of atoms in the ASP program
- **Rules**: Let $R = \{r_1, r_2, \ldots\}$ be the set of rules in the ASP program
- For a rule $r$: $heads(r)$ is the set of head atoms, $body^+(r)$ are positive body literals, $body^-(r)$ are negative body literals

**PB Constraint Notation**: Constraints are written as $\sum_i w_i \cdot l_i \geq k$ where $w_i$ are weights, $l_i$ are literals, and $k$ is the threshold. We use $\overline{x}$ to denote the negation of variable $x$.

## Rule Classification

ASP rules vary along two independent axes:

**Head type:**
- **Basic rule**: Head is a single atom. If the rule fires, the head *must* be true.
- **Choice rule**: Head is `{h1; h2; ...}`. If the rule fires, each head *may* be true (independently).

**Body type:**
- **Basic body**: Conjunction of literals (e.g., `a, b, not c`)
- **Cardinality body**: Count constraint (e.g., `#count{a; b; c} >= 2`)
- **Weight body**: Weighted sum constraint (e.g., `#sum{1:a; 2:b; 3:c} >= 4`)

Since basic and cardinality bodies are special cases of weight bodies (with all weights = 1, and threshold = number of literals or cardinality bound respectively), this formalization treats all bodies uniformly as weight bodies.

## Weight Body Encoding

A rule with a weight body has the form:
$$h \leftarrow \#sum\{w_1 : b_1; \ldots; w_n : b_n; w_{n+1} : \text{not } c_1; \ldots; w_m : \text{not } c_k\} \geq t$$

where $h$ is the head, $b_i$ are positive body atoms, $c_j$ are negated body atoms, $w_i$ are weights, and $t$ is the threshold.

Define the **falsification weight** for rule $r$:
$$W_r = \left(\sum_{i=1}^{m} w_i\right) - t + 1$$

This is the minimum total weight of falsified literals needed to guarantee the body is not satisfied.

---

## Candidate Solver

### Variables
- $x_{\text{cand}}$ for each atom $x \in A$
- $active_{r,\text{cand}}$ for each rule $r \in R$

### Constraint 1: Body Satisfaction (when active)
For a weight rule $r$ with head $h$, positive body atoms $B^+ = \{b_1, \ldots, b_n\}$ with weights $\{w_1, \ldots, w_n\}$, and negative body atoms $B^- = \{c_1, \ldots, c_k\}$ with weights $\{u_1, \ldots, u_k\}$:

$$W_r \cdot active_{r,\text{cand}} + \sum_{b_i \in B^+} w_i \cdot \overline{b_{i,\text{cand}}} + \sum_{c_j \in B^-} u_j \cdot c_{j,\text{cand}} \geq W_r$$

### Constraint 2: Body Falsification (when inactive)
$$t \cdot \overline{active_{r,\text{cand}}} + \sum_{b_i \in B^+} w_i \cdot b_{i,\text{cand}} + \sum_{c_j \in B^-} u_j \cdot \overline{c_{j,\text{cand}}} \geq t$$

### Constraint 3: Head Propagation — Basic Rules Only
$$h_{\text{cand}} + \overline{active_{r,\text{cand}}} \geq 1$$

*Note: This constraint is omitted for choice rules.*

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

$$W_r \cdot \overline{active_{r,\text{cand}}} + W_r \cdot active_{r,\text{check}} + \sum_{b_i \in B^+} w_i \cdot \overline{b_{i,\text{check}}} + \sum_{c_j \in B^-} u_j \cdot c_{j,\text{check}} \geq W_r$$

### Constraint 7: Reduct Head Propagation
For each head $h$ of rule $r$:
$$\overline{h_{\text{cand}}} + h_{\text{check}} + \overline{active_{r,\text{check}}} \geq 1$$

*Note: Basic rules have exactly one head. Choice rules may have multiple heads, generating one instance of this constraint per head.*

---

## Loop Constraints

### Constraint 8: Loop Constraints

When the candidate solver finds a solution $S_{\text{cand}}$ and the check solver finds a strict subset $S_{\text{check}} \subset S_{\text{cand}}$:

1. Compute the **unfounded set**: $U = S_{\text{cand}} \setminus S_{\text{check}}$
2. Find **external support rules**: rules $r$ where $heads(r) \cap U \neq \emptyset$ but $body^+(r) \cap U = \emptyset$
3. Add to the candidate solver:
$$\sum_{x \in U} \overline{x_{\text{cand}}} + \sum_{r \in \text{external}} active_{r,\text{cand}} \geq 1$$

Repeat until the check solver returns UNSAT, indicating no unfounded set exists.

### Constraint 8 Initialization: Single-Atom Loop Constraints
As a special case, for each atom $x$ we add single-atom loop constraints upfront:
$$\overline{x_{\text{cand}}} + \sum_{r : x \in heads(r) \land x \notin body^+(r)} active_{r,\text{cand}} \geq 1$$

These are loop constraints where $U = \{x\}$, added to bootstrap supportedness without needing check solver iterations.

---

## Choice Rules

Choice rules (e.g., `{h1; h2} :- body.`) differ from basic rules:
- **Omit Constraint 3** (head propagation in candidate solver)
- May have multiple heads, so **Constraint 7** generates one instance per head

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

Constraint 3:
$h_{\text{cand}} + \overline{active_{r,\text{cand}}} \geq 1$

**Check Solver Constraints:**

Constraint 6:
$8 \cdot \overline{active_{r,\text{cand}}} + 8 \cdot active_{r,\text{check}} + 1 \cdot \overline{b1_{\text{check}}} + 2 \cdot \overline{b2_{\text{check}}} + 3 \cdot \overline{b3_{\text{check}}} + 4 \cdot b4_{\text{check}} \geq 8$

Constraint 7:
$\overline{h_{\text{cand}}} + h_{\text{check}} + \overline{active_{r,\text{check}}} \geq 1$

**Loop Constraint Example:**

If candidate solution is $\{w, x, y, z\}$ and check solution is $\{w, x\}$, the unfounded set is $\{y, z\}$.

If rules $r_1, r_2, r_3$ have $y$ or $z$ in their heads but neither $y$ nor $z$ in their positive bodies, add:
$$\overline{y_{\text{cand}}} + \overline{z_{\text{cand}}} + active_{r_1,\text{cand}} + active_{r_2,\text{cand}} + active_{r_3,\text{cand}} \geq 1$$
