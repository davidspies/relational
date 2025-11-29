//! Aggregation operators: aggregate, count, sum, min, max.

use std::collections::HashMap;

use crate::change::Diff;
use crate::collection::Multiset;
use crate::Tuple;

/// Group and aggregate operator.
/// Groups tuples by key and applies an aggregation function.
pub fn aggregate<
    T: Tuple,
    K: Tuple,
    V: Tuple,
    A: Tuple,
    FK: Fn(&T) -> K,
    FV: Fn(&T) -> V,
    FA: Fn(K, &[(V, Diff)]) -> A,
>(
    input: &Multiset<T>,
    key_fn: FK,
    value_fn: FV,
    agg_fn: FA,
) -> Multiset<A> {
    // Group by key
    let mut groups: HashMap<K, Vec<(V, Diff)>> = HashMap::new();
    for (tuple, diff) in input.iter_with_multiplicity() {
        let key = key_fn(tuple);
        let value = value_fn(tuple);
        groups.entry(key).or_default().push((value, diff));
    }

    // Apply aggregation
    let mut output = Multiset::new();
    for (key, values) in groups {
        let result = agg_fn(key, &values);
        output.insert(result);
    }
    output
}

/// Count aggregation helper.
pub fn count<K: Tuple>(key: K, values: &[((), Diff)]) -> (K, i64) {
    let total: i64 = values.iter().map(|(_, d)| d.0).sum();
    (key, total)
}

/// Sum aggregation helper.
pub fn sum<K: Tuple>(key: K, values: &[(i64, Diff)]) -> (K, i64) {
    let total: i64 = values.iter().map(|(v, d)| v * d.0).sum();
    (key, total)
}

/// Min aggregation helper (returns None if empty).
pub fn min<K: Tuple, V: Tuple + Ord>(key: K, values: &[(V, Diff)]) -> Option<(K, V)> {
    values
        .iter()
        .filter(|(_, d)| d.is_positive())
        .map(|(v, _)| v)
        .min()
        .cloned()
        .map(|v| (key, v))
}

/// Max aggregation helper (returns None if empty).
pub fn max<K: Tuple, V: Tuple + Ord>(key: K, values: &[(V, Diff)]) -> Option<(K, V)> {
    values
        .iter()
        .filter(|(_, d)| d.is_positive())
        .map(|(v, _)| v)
        .max()
        .cloned()
        .map(|v| (key, v))
}
