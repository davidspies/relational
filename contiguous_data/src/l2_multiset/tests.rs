use super::*;
use proptest::prelude::*;
use std::collections::HashMap;

#[test]
fn test_basic_insert_delete() {
    let mut l2: L2Multiset<&str, i32> = L2Multiset::new();

    l2.update("a", 1, 1);
    assert!(l2.contains(&"a", &1));
    assert_eq!(l2.get(&"a", &1), 1);
    assert_eq!(l2.len(&"a"), 1);

    l2.update("a", 1, 1);
    assert_eq!(l2.get(&"a", &1), 2);
    assert_eq!(l2.len(&"a"), 1); // Still 1 distinct value

    l2.update("a", 2, 1);
    assert_eq!(l2.len(&"a"), 2);

    l2.update("a", 1, -1);
    assert_eq!(l2.get(&"a", &1), 1);
    assert!(l2.contains(&"a", &1));

    l2.update("a", 1, -1);
    assert_eq!(l2.get(&"a", &1), 0);
    assert!(!l2.contains(&"a", &1));
    assert_eq!(l2.len(&"a"), 1); // Only value 2 left
}

#[test]
fn test_iter_values() {
    let mut l2: L2Multiset<&str, i32> = L2Multiset::new();

    l2.update("a", 1, 1);
    l2.update("a", 2, 1);
    l2.update("a", 3, 1);

    let values: Vec<_> = l2.iter_values(&"a").cloned().collect();
    assert_eq!(values.len(), 3);
    assert!(values.contains(&1));
    assert!(values.contains(&2));
    assert!(values.contains(&3));
}

#[test]
fn test_promotion_and_demotion() {
    let mut l2: L2Multiset<&str, i32> = L2Multiset::new();

    // Small: 1 element
    l2.update("a", 1, 1);
    assert_eq!(l2.len(&"a"), 1);

    // Small: 2 elements
    l2.update("a", 2, 1);
    assert_eq!(l2.len(&"a"), 2);

    // Promote to Large: 3 elements
    l2.update("a", 3, 1);
    assert_eq!(l2.len(&"a"), 3);

    // Still Large: 4 elements
    l2.update("a", 4, 1);
    assert_eq!(l2.len(&"a"), 4);

    // Remove to 3
    l2.update("a", 4, -1);
    assert_eq!(l2.len(&"a"), 3);

    // Demote to Small: 2 elements
    l2.update("a", 3, -1);
    assert_eq!(l2.len(&"a"), 2);

    // Verify values still present
    assert!(l2.contains(&"a", &1));
    assert!(l2.contains(&"a", &2));
}

#[test]
fn test_empty_key() {
    let mut l2: L2Multiset<&str, i32> = L2Multiset::new();

    assert!(l2.is_empty(&"a"));
    assert_eq!(l2.len(&"a"), 0);
    assert!(l2.iter_values(&"a").next().is_none());

    l2.update("a", 1, 1);
    assert!(!l2.is_empty(&"a"));

    l2.update("a", 1, -1);
    assert!(l2.is_empty(&"a"));
}

#[derive(Debug, Clone)]
enum Op {
    Insert(u8, u8), // key, value
    Delete(u8, u8),
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0..4u8, 0..8u8).prop_map(|(k, v)| Op::Insert(k, v)),
        (0..4u8, 0..8u8).prop_map(|(k, v)| Op::Delete(k, v)),
    ]
}

proptest! {
    #[test]
    fn prop_matches_hashmap_multiset(ops in proptest::collection::vec(op_strategy(), 0..100)) {
        let mut l2: L2Multiset<u8, u8> = L2Multiset::new();
        let mut reference: HashMap<u8, Multiset<u8>> = HashMap::default();

        for op in ops {
            match op {
                Op::Insert(k, v) => {
                    l2.update(k, v, 1);
                    let ms = reference.entry(k).or_default();
                    ms.update(v, 1);
                    if ms.is_empty() {
                        reference.remove(&k);
                    }
                }
                Op::Delete(k, v) => {
                    l2.update(k, v, -1);
                    let ms = reference.entry(k).or_default();
                    ms.update(v, -1);
                    if ms.is_empty() {
                        reference.remove(&k);
                    }
                }
            }

            // Verify consistency
            for key in 0..4u8 {
                let ref_ms = reference.get(&key);
                // is_empty means no values with non-zero count
                let ref_has_nonzero = ref_ms.is_some_and(|ms| ms.iter().next().is_some());

                prop_assert_eq!(l2.is_empty(&key), !ref_has_nonzero);

                for value in 0..8u8 {
                    let ref_count = ref_ms.map_or(0, |ms| ms.get(&value));
                    prop_assert_eq!(
                        l2.get(&key, &value),
                        ref_count,
                        "Mismatch for ({}, {})",
                        key,
                        value
                    );
                    prop_assert_eq!(
                        l2.contains(&key, &value),
                        ref_count != 0,
                        "Contains mismatch for ({}, {})",
                        key,
                        value
                    );
                }

                // Verify iter_values matches
                let l2_values: std::collections::HashSet<_> = l2.iter_values(&key).cloned().collect();
                let ref_values: std::collections::HashSet<_> = ref_ms
                    .map(|ms| ms.iter().cloned().collect())
                    .unwrap_or_default();
                prop_assert_eq!(
                    l2_values,
                    ref_values,
                    "Iter values mismatch for key {}",
                    key
                );
            }
        }
    }
}
