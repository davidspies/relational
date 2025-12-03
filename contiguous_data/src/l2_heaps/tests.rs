use super::*;
use proptest::prelude::*;
use std::collections::{BTreeSet, HashMap};

#[test]
fn test_default() {
    let heaps: L2Heaps<&str, i32> = L2Heaps::default();
    assert!(heaps.is_empty(&"a"));
}

#[test]
fn test_push_pop_single_heap() {
    let mut heaps: L2Heaps<&str, i32> = L2Heaps::new();
    heaps.push("a", 3);
    heaps.push("a", 1);
    heaps.push("a", 4);
    heaps.push("a", 2);
    heaps.push("a", 5);

    assert_eq!(heaps.pop(&"a"), Some(1));
    assert_eq!(heaps.pop(&"a"), Some(2));
    assert_eq!(heaps.pop(&"a"), Some(3));
    assert_eq!(heaps.pop(&"a"), Some(4));
    assert_eq!(heaps.pop(&"a"), Some(5));
    assert_eq!(heaps.pop(&"a"), None);
}

#[test]
fn test_multiple_heaps() {
    let mut heaps: L2Heaps<&str, i32> = L2Heaps::new();
    heaps.push("a", 5);
    heaps.push("a", 3);
    heaps.push("b", 10);
    heaps.push("b", 2);
    heaps.push("a", 7);

    assert_eq!(heaps.peek(&"a"), Some(&3));
    assert_eq!(heaps.peek(&"b"), Some(&2));

    assert_eq!(heaps.pop(&"b"), Some(2));
    assert_eq!(heaps.pop(&"a"), Some(3));
    assert_eq!(heaps.pop(&"b"), Some(10));
    assert_eq!(heaps.pop(&"a"), Some(5));
    assert_eq!(heaps.pop(&"a"), Some(7));
}

#[test]
fn test_peek() {
    let mut heaps: L2Heaps<&str, i32> = L2Heaps::new();
    assert_eq!(heaps.peek(&"a"), None);

    heaps.push("a", 5);
    assert_eq!(heaps.peek(&"a"), Some(&5));

    heaps.push("a", 2);
    assert_eq!(heaps.peek(&"a"), Some(&2));

    heaps.push("a", 8);
    assert_eq!(heaps.peek(&"a"), Some(&2));
}

#[test]
fn test_remove_from_middle() {
    let mut heaps: L2Heaps<&str, i32> = L2Heaps::new();
    heaps.push("a", 1);
    heaps.push("a", 5);
    heaps.push("a", 3);
    heaps.push("a", 7);
    heaps.push("a", 2);

    assert!(heaps.remove(&"a", &3));
    assert!(!heaps.contains(&"a", &3));

    assert_eq!(heaps.pop(&"a"), Some(1));
    assert_eq!(heaps.pop(&"a"), Some(2));
    assert_eq!(heaps.pop(&"a"), Some(5));
    assert_eq!(heaps.pop(&"a"), Some(7));
}

#[test]
fn test_remove_min() {
    let mut heaps: L2Heaps<&str, i32> = L2Heaps::new();
    heaps.push("a", 1);
    heaps.push("a", 5);
    heaps.push("a", 3);

    assert!(heaps.remove(&"a", &1));
    assert_eq!(heaps.peek(&"a"), Some(&3));
}

#[test]
fn test_remove_nonexistent() {
    let mut heaps: L2Heaps<&str, i32> = L2Heaps::new();
    heaps.push("a", 1);
    heaps.push("a", 2);

    assert!(!heaps.remove(&"a", &99));
    assert!(!heaps.remove(&"b", &1));
}

#[test]
fn test_contains() {
    let mut heaps: L2Heaps<&str, i32> = L2Heaps::new();
    heaps.push("a", 42);

    assert!(heaps.contains(&"a", &42));
    assert!(!heaps.contains(&"a", &99));
    assert!(!heaps.contains(&"b", &42));
}

#[test]
fn test_is_empty() {
    let mut heaps: L2Heaps<&str, i32> = L2Heaps::new();
    assert!(heaps.is_empty(&"a"));

    heaps.push("a", 1);
    assert!(!heaps.is_empty(&"a"));
    assert!(heaps.is_empty(&"b"));

    heaps.pop(&"a");
    assert!(heaps.is_empty(&"a"));
}

#[test]
fn test_remove_triggers_bubble_up() {
    // When removing a node, if the replacement value is smaller than parent,
    // bubble_up should be called in reheapify.
    //
    // Build this heap (no bubble-ups during construction):
    //            1
    //          /   \
    //         2     1000
    //        / \    /  \
    //       3   4  1001 1002
    //      /
    //     5
    //
    // When we remove 1001, the last leaf (5) replaces it.
    // Parent of 1001's position is 1000. Since 5 < 1000, bubble_up triggers.
    let mut heaps: L2Heaps<&str, i32> = L2Heaps::new();
    heaps.push("a", 1);
    heaps.push("a", 1000);
    heaps.push("a", 2);
    heaps.push("a", 3);
    heaps.push("a", 4);
    heaps.push("a", 1001);
    heaps.push("a", 1002);
    heaps.push("a", 5);

    heaps.remove(&"a", &1001);

    // Verify heap property still holds
    assert_eq!(heaps.pop(&"a"), Some(1));
    assert_eq!(heaps.pop(&"a"), Some(2));
    assert_eq!(heaps.pop(&"a"), Some(3));
    assert_eq!(heaps.pop(&"a"), Some(4));
    assert_eq!(heaps.pop(&"a"), Some(5));
    assert_eq!(heaps.pop(&"a"), Some(1000));
    assert_eq!(heaps.pop(&"a"), Some(1002));
}

#[test]
fn test_heap_property_after_removal() {
    let mut heaps: L2Heaps<&str, i32> = L2Heaps::new();
    for i in (0..20).rev() {
        heaps.push("a", i);
    }

    heaps.remove(&"a", &10);
    heaps.remove(&"a", &5);
    heaps.remove(&"a", &15);

    let mut prev = None;
    while let Some(val) = heaps.pop(&"a") {
        if let Some(p) = prev {
            assert!(
                val >= p,
                "heap property violated: {} should be >= {}",
                val,
                p
            );
        }
        prev = Some(val);
    }
}

#[test]
fn test_same_value_different_heaps() {
    let mut heaps: L2Heaps<&str, i32> = L2Heaps::new();
    heaps.push("a", 5);
    heaps.push("b", 5);

    assert!(heaps.contains(&"a", &5));
    assert!(heaps.contains(&"b", &5));

    heaps.remove(&"a", &5);
    assert!(!heaps.contains(&"a", &5));
    assert!(heaps.contains(&"b", &5));
}

#[derive(Debug, Clone)]
enum Op {
    Push(u8, u8),
    Pop(u8),
    Remove(u8, u8),
    Contains(u8, u8),
    Peek(u8),
    IsEmpty(u8),
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0..4u8, 0..20u8).prop_map(|(k, v)| Op::Push(k, v)),
        (0..4u8).prop_map(Op::Pop),
        (0..4u8, 0..20u8).prop_map(|(k, v)| Op::Remove(k, v)),
        (0..4u8, 0..20u8).prop_map(|(k, v)| Op::Contains(k, v)),
        (0..4u8).prop_map(Op::Peek),
        (0..4u8).prop_map(Op::IsEmpty),
    ]
}

proptest! {
    #[test]
    fn behaves_like_hashmap_btreeset(ops in proptest::collection::vec(op_strategy(), 0..200)) {
        let mut l2: L2Heaps<u8, u8> = L2Heaps::new();
        let mut reference: HashMap<u8, BTreeSet<u8>> = HashMap::new();

        for op in ops {
            match op {
                Op::Push(k, v) => {
                    let set = reference.entry(k).or_default();
                    if !set.contains(&v) {
                        set.insert(v);
                        l2.push(k, v);
                    }
                }
                Op::Pop(k) => {
                    let expected = reference.get_mut(&k).and_then(|set| set.pop_first());
                    let actual = l2.pop(&k);
                    prop_assert_eq!(expected, actual);
                }
                Op::Remove(k, v) => {
                    let expected = reference.get_mut(&k).map(|s| s.remove(&v)).unwrap_or(false);
                    let actual = l2.remove(&k, &v);
                    prop_assert_eq!(expected, actual);
                }
                Op::Contains(k, v) => {
                    let expected = reference.get(&k).map(|s| s.contains(&v)).unwrap_or(false);
                    let actual = l2.contains(&k, &v);
                    prop_assert_eq!(expected, actual);
                }
                Op::Peek(k) => {
                    let expected = reference.get(&k).and_then(|s| s.first());
                    let actual = l2.peek(&k);
                    prop_assert_eq!(expected, actual);
                }
                Op::IsEmpty(k) => {
                    let expected = reference.get(&k).map(|s| s.is_empty()).unwrap_or(true);
                    let actual = l2.is_empty(&k);
                    prop_assert_eq!(expected, actual);
                }
            }
        }
    }
}
