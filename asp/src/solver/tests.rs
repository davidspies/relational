use super::*;
use crate::parse_smodels;

/// Helper to solve an ASP program and return sorted answer set names.
fn solve_asp(input: &str) -> Vec<Vec<String>> {
    let program = parse_smodels(input).unwrap();
    let mut solver = AspSolver::new(program);
    let mut results: Vec<Vec<String>> = solver
        .solve()
        .into_iter()
        .map(|answer_set| {
            let mut names: Vec<String> = answer_set
                .iter()
                .filter_map(|atom| solver.atom_name(*atom).map(String::from))
                .collect();
            names.sort();
            names
        })
        .collect();
    results.sort();
    results
}

#[test]
fn test_simple_fact() {
    // p.
    let input = "1 2 0 0\n0\n2 p\n0\n";
    let results = solve_asp(input);
    assert_eq!(results, vec![vec!["p"]]);
}

#[test]
fn test_two_facts() {
    // p. q.
    let input = "1 2 0 0\n1 3 0 0\n0\n2 p\n3 q\n0\n";
    let results = solve_asp(input);
    assert_eq!(results, vec![vec!["p", "q"]]);
}

#[test]
fn test_default_negation_two_models() {
    // a :- not b. b :- not a.
    // Two answer sets: {a} and {b}
    let input = "1 2 1 1 3\n1 3 1 1 2\n0\n2 a\n3 b\n0\n";
    let results = solve_asp(input);
    assert_eq!(results, vec![vec!["a"], vec!["b"]]);
}

#[test]
fn test_self_referential_negation_unsat() {
    // a :- not a.
    // No stable models
    let input = "1 2 1 1 2\n0\n2 a\n0\n";
    let results = solve_asp(input);
    assert!(results.is_empty());
}

#[test]
fn test_constraint_filters_model() {
    // a :- not b. b :- not a. :- a, b.
    // Still two answer sets since a and b can't both be true anyway
    let input = "1 2 1 1 3\n1 3 1 1 2\n1 1 2 0 2 3\n0\n2 a\n3 b\n0\n";
    let results = solve_asp(input);
    assert_eq!(results, vec![vec!["a"], vec!["b"]]);
}

#[test]
fn test_constraint_makes_unsat() {
    // a :- not b. b :- not a. :- a. :- b.
    // No stable models - both a and b are forbidden
    let input = "1 2 1 1 3\n1 3 1 1 2\n1 1 1 0 2\n1 1 1 0 3\n0\n2 a\n3 b\n0\n";
    let results = solve_asp(input);
    assert!(results.is_empty());
}

#[test]
fn test_chain_derivation() {
    // a. b :- a. c :- b.
    // One answer set: {a, b, c}
    let input = "1 2 0 0\n1 3 1 0 2\n1 4 1 0 3\n0\n2 a\n3 b\n4 c\n0\n";
    let results = solve_asp(input);
    assert_eq!(results, vec![vec!["a", "b", "c"]]);
}

#[test]
fn test_unfounded_loop() {
    // a :- b. b :- a.
    // No facts, so no stable models with a or b (only empty model)
    let input = "1 2 1 0 3\n1 3 1 0 2\n0\n2 a\n3 b\n0\n";
    let results = solve_asp(input);
    assert_eq!(results, vec![Vec::<String>::new()]);
}

#[test]
fn test_empty_program() {
    // Empty program has one answer set: {}
    let input = "0\n0\n";
    let results = solve_asp(input);
    assert_eq!(results, vec![Vec::<String>::new()]);
}

#[test]
fn test_supported_loop() {
    // a :- b. b :- a. a :- not c.
    // c is false by default, so a is supported, then b is supported
    let input = "1 2 1 0 3\n1 3 1 0 2\n1 2 1 1 4\n0\n2 a\n3 b\n4 c\n0\n";
    let results = solve_asp(input);
    assert_eq!(results, vec![vec!["a", "b"]]);
}

#[test]
fn test_constraint_requires_derivation() {
    // a. :- not a. (a is a fact, constraint requires a to be true - satisfied)
    let input = "1 2 0 0\n1 1 1 1 2\n0\n2 a\n0\n";
    let results = solve_asp(input);
    assert_eq!(results, vec![vec!["a"]]);
}

#[test]
fn test_multiple_rules_same_head() {
    // a :- b. a :- c. b.
    // a is derived from b
    let input = "1 2 1 0 3\n1 2 1 0 4\n1 3 0 0\n0\n2 a\n3 b\n4 c\n0\n";
    let results = solve_asp(input);
    assert_eq!(results, vec![vec!["a", "b"]]);
}

#[test]
fn test_choice_rule_forbidden() {
    // {a}. :- a.
    // Only {} is valid since a is forbidden
    let input = "3 1 2 0 0\n1 1 1 0 2\n0\n2 a\n0\n";
    let results = solve_asp(input);
    assert_eq!(results, vec![Vec::<String>::new()]);
}

#[test]
fn test_choice_rule_required() {
    // {a}. :- not a.
    // Only {a} is valid since a is required
    let input = "3 1 2 0 0\n1 1 1 1 2\n0\n2 a\n0\n";
    let results = solve_asp(input);
    assert_eq!(results, vec![vec!["a"]]);
}

#[test]
fn test_disjunctive_simple() {
    // a | b.
    // Two answer sets: {a} and {b}
    // Format: 8 num_heads h1 h2 num_pos num_neg body_lits...
    let input = "8 2 2 3 0 0\n0\n2 a\n3 b\n0\n";
    let results = solve_asp(input);
    assert_eq!(results, vec![vec!["a"], vec!["b"]]);
}

#[test]
fn test_disjunctive_with_body() {
    // a | b :- c. c.
    // Two answer sets: {a, c} and {b, c}
    // Rule 1: 8 2 2 3 1 0 4 (a | b :- c)
    // Rule 2: 1 4 0 0 (c.)
    let input = "8 2 2 3 1 0 4\n1 4 0 0\n0\n2 a\n3 b\n4 c\n0\n";
    let results = solve_asp(input);
    assert_eq!(results, vec![vec!["a", "c"], vec!["b", "c"]]);
}

#[test]
fn test_disjunctive_with_constraint() {
    // a | b. :- a.
    // Only {b} is valid since a is forbidden
    let input = "8 2 2 3 0 0\n1 1 1 0 2\n0\n2 a\n3 b\n0\n";
    let results = solve_asp(input);
    assert_eq!(results, vec![vec!["b"]]);
}

#[test]
fn test_disjunctive_three_heads() {
    // a | b | c.
    // Three answer sets: {a}, {b}, {c}
    let input = "8 3 2 3 4 0 0\n0\n2 a\n3 b\n4 c\n0\n";
    let results = solve_asp(input);
    assert_eq!(results, vec![vec!["a"], vec!["b"], vec!["c"]]);
}

#[test]
fn test_disjunctive_with_default_negation() {
    // a | b. c :- not a.
    // Two answer sets: {a} and {b, c}
    let input = "8 2 2 3 0 0\n1 4 1 1 2\n0\n2 a\n3 b\n4 c\n0\n";
    let results = solve_asp(input);
    assert_eq!(results, vec![vec!["a"], vec!["b", "c"]]);
}

#[test]
fn test_weight_body_loop() {
    // a :- #count{b:b; c:c} >= 1.
    // b :- a.
    // {c}.
    // :- not c.
    // Expected answer set: {a, b, c}
    //
    // From gringo --output=smodels:
    // 3 1 2 0 0       -> {c}.  (choice rule, head=2)
    // 1 1 1 1 2       -> :- not c.  (constraint: false :- not c)
    // 1 3 1 0 4       -> a :- aux4
    // 1 5 1 0 3       -> b :- a
    // 2 6 2 0 1 2 5   -> aux6 :- {c, b} >= 1  (cardinality rule)
    // 1 4 1 0 6       -> aux4 :- aux6
    // Symbols: 2=c, 3=a, 5=b
    let input = "3 1 2 0 0\n1 1 1 1 2\n1 3 1 0 4\n1 5 1 0 3\n2 6 2 0 1 2 5\n1 4 1 0 6\n0\n2 c\n3 a\n5 b\n0\n";
    let results = solve_asp(input);
    assert_eq!(results, vec![vec!["a", "b", "c"]]);
}

#[test]
fn test_weight_body_loop_empty() {
    // a :- #count{b:b; c:c} >= 1.
    // b :- a.
    // {c}.
    // :- c.       (c must be false)
    //
    // The only stable model is {} because:
    // - c=false (from :- c)
    // - a requires {b, c} >= 1, but c=false so need b=true
    // - b requires a, which requires b (circular with no external support)
    // - So {a, b} is unfounded and rejected
    //
    // From gringo --output=smodels:
    // 3 1 2 0 0       -> {c}.
    // 1 1 1 0 2       -> :- c.
    // 1 3 1 0 4       -> a :- aux4
    // 1 5 1 0 3       -> b :- a
    // 2 6 2 0 1 2 5   -> aux6 :- {c, b} >= 1
    // 1 4 1 0 6       -> aux4 :- aux6
    // Symbols: 2=c, 3=a, 5=b
    let input = "3 1 2 0 0\n1 1 1 0 2\n1 3 1 0 4\n1 5 1 0 3\n2 6 2 0 1 2 5\n1 4 1 0 6\n0\n2 c\n3 a\n5 b\n0\n";
    let results = solve_asp(input);
    // Only the empty set should be a stable model
    assert_eq!(
        results,
        vec![Vec::<&str>::new()],
        "Expected only empty set but got: {:?}",
        results
    );
}

#[test]
fn test_weight_body_two_models() {
    // a :- #count{b:b; c:c} >= 1.
    // b :- a.
    // {c}.
    //
    // Two stable models:
    // - {} (empty: c not chosen, so a not derived, so b not derived)
    // - {a, b, c} (c chosen, {c} >= 1 satisfied, a derived, b derived)
    //
    // From gringo --output=smodels:
    // 3 1 2 0 0       -> {c}.
    // 1 3 1 0 4       -> a :- aux4
    // 1 5 1 0 3       -> b :- a
    // 2 6 2 0 1 2 5   -> aux6 :- {c, b} >= 1
    // 1 4 1 0 6       -> aux4 :- aux6
    // Symbols: 2=c, 3=a, 5=b
    let input = "3 1 2 0 0\n1 3 1 0 4\n1 5 1 0 3\n2 6 2 0 1 2 5\n1 4 1 0 6\n0\n2 c\n3 a\n5 b\n0\n";
    let results = solve_asp(input);
    assert_eq!(results.len(), 2, "Expected 2 models but got: {:?}", results);
    assert!(
        results.iter().any(|m| m.is_empty()),
        "Expected empty model but got: {:?}",
        results
    );
    assert!(
        results.iter().any(|m| m == &vec!["a", "b", "c"]),
        "Expected {{a,b,c}} but got: {:?}",
        results
    );
}

#[test]
fn test_self_loop_rule() {
    // Program:
    //   {a}.
    //   b :- a.
    //   c :- a.
    //   d :- b, c.
    //   d :- d, d.  % Self-loop
    //   :- not d, a.
    //
    // Two stable models: {} and {a, b, c, d}
    //
    // From gringo --output=smodels:
    // 3 1 2 0 0      -> {a}.
    // 1 3 1 0 2      -> b :- a
    // 1 4 1 0 2      -> c :- a
    // 1 5 2 0 3 4    -> d :- b, c
    // 1 5 2 0 5 5    -> d :- d, d
    // 1 1 2 1 5 2    -> :- not d, a
    // Symbols: 2=a, 5=d
    let input =
        "3 1 2 0 0\n1 3 1 0 2\n1 4 1 0 2\n1 5 2 0 3 4\n1 5 2 0 5 5\n1 1 2 1 5 2\n0\n2 a\n5 d\n0\n";
    let results = solve_asp(input);
    assert_eq!(results.len(), 2, "Expected 2 models but got: {:?}", results);
    assert!(
        results.iter().any(|m| m.is_empty()),
        "Expected empty model but got: {:?}",
        results
    );
    assert!(
        results.iter().any(|m| m == &vec!["a", "d"]),
        "Expected {{a, d}} but got: {:?}",
        results
    );
}

#[test]
fn test_loop_constraint_should_be_added_not_blocked() {
    // Program:
    //   a :- {b, c} >= 1.
    //   b :- a.
    //   {c}.
    //   :- c.       (c must be false)
    //
    // Only valid stable model is {} (empty set).

    let input = "3 1 2 0 0\n1 1 1 0 2\n1 3 1 0 4\n1 5 1 0 3\n2 6 2 0 1 2 5\n1 4 1 0 6\n0\n2 c\n3 a\n5 b\n0\n";
    let program = crate::parse_smodels(input).unwrap();
    let mut solver = super::AspSolver::new(program);

    let results = solver.solve();

    // The result is correct (empty set only)
    assert_eq!(results.len(), 1);
    assert!(results[0].is_empty());
}
