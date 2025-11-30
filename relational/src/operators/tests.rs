//! Tests for relational operators.

use super::*;
use crate::change::Diff;
use crate::collection::Multiset;

#[test]
fn test_map() {
    let input: Multiset<i32> = [1, 2, 3].into_iter().collect();
    let output = map(&input, |x| x * 2);

    assert!(output.contains(&2));
    assert!(output.contains(&4));
    assert!(output.contains(&6));
}

#[test]
fn test_filter() {
    let input: Multiset<i32> = [1, 2, 3, 4, 5].into_iter().collect();
    let output = filter(&input, |x| x % 2 == 0);

    assert!(!output.contains(&1));
    assert!(output.contains(&2));
    assert!(!output.contains(&3));
    assert!(output.contains(&4));
}

#[test]
fn test_join() {
    let left: Multiset<(i32, &str)> = [(1, "a"), (2, "b"), (3, "c")].into_iter().collect();
    let right: Multiset<(i32, i32)> = [(1, 10), (2, 20), (4, 40)].into_iter().collect();

    let joined = join(&left, &right, |(k, _)| *k, |(k, _)| *k);

    assert!(joined.contains(&((1, "a"), (1, 10))));
    assert!(joined.contains(&((2, "b"), (2, 20))));
    assert!(!joined.contains(&((3, "c"), (3, 30))));
}

#[test]
fn test_distinct() {
    let mut input = Multiset::new();
    input.insert(1);
    input.insert(1);
    input.insert(2);

    let output = distinct(&input);

    assert_eq!(output.get(&1), Diff::ONE);
    assert_eq!(output.get(&2), Diff::ONE);
}

#[test]
fn test_union() {
    let a: Multiset<i32> = [1, 2].into_iter().collect();
    let b: Multiset<i32> = [2, 3].into_iter().collect();

    let result = union(&a, &b);

    assert_eq!(result.get(&1), Diff::ONE);
    assert_eq!(result.get(&2), Diff(2)); // 2 appears in both
    assert_eq!(result.get(&3), Diff::ONE);
}
