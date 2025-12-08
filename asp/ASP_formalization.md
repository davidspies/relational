To do this incrementally, let's start with just basic rules. No disjunction, no choice rules, no weight bodies, nothing else.

Initially, the bottom solver is solving the ASP program _nearly_ naively encoded as SAT with no supportedness constraints. It contains the variables x_bottom for each atom x in the ASP program, and active_r_bottom for each rule r in the ASP program. So if you have a rule r which says:

h :- b1, b2, b3, not b4.

We'll encode that as:

active_r_bottom v not b1_bottom v not b2_bottom v not b3_bottom v b4_bottom

not active_r_bottom v b1_bottom
not active_r_bottom v b2_bottom
not active_r_bottom v b3_bottom
not active_r_bottom v not b4_bottom

h_bottom v not active_r_bottom

We export that as the external relation (both the bottom variables and the rule activeness variables).

The _top_ solver has two more variables for each ASP atom, x_top and x_diminished, and one for each rule, active_r_top. These aren't external, they're free in the top solver, but come with the following constraints:

not a_top v a_bottom
not a_diminished v not a_top
not a_diminished v a_bottom

We also have a single clause enforcing that our subset is strict:

a_diminished v b_diminished v c_diminished...

If a rule is active, then its positive part must be satisfied in the reduct

not active_r_bottom v active_r_top v not b1_top v not b2_top v not b3_top (we can omit b4, because it's not positive)
not h_bottom v h_top v not active_r_top

Now suppose that we find a solution, let's say {w_top, x_top} are the true variables, and that the bottom solution was {w_bottom, x_bottom, y_bottom, z_bottom}. In that case, we want to take the difference and add in the loop constraint to the bottom solver and re-solve. In this case that's {y, z}.

To find the loop constraint, find all rules which have y or z in their head but have _neither_ in their body (so we would exclude a rule of the form y :- z). Then we assert that in order for all of those to be true, at least one of these rules needs to be active. So if we found that the rules which have y or z in their heads but not in their bodies are r1, r2, r3, we would add the new base clause (to the _bottom_ solver only):

not y_bottom v not z_bottom v r1_active v r2_active v r3_active

and re-run it until we get to where the bottom solver returns SAT and the top solver returns UNSAT. Then we return that solution.

Choice rules are almost the same as basic rules, with just one minor change.

We don't include line 16

Also we have a line 33 rule _for each head_ (since choice rules are allowed multiple heads)
