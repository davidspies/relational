//! Max operator - maximum value by key.

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;

use crate::Tuple;
use crate::change::Diff;

use super::relation::Relation;

/// A max relation - tracks maximum value by key.
/// Output is (key, max_value) pairs.
pub struct MaxRelation<T, K, V, FK, FV, R>
where
    T: Tuple,
    K: Tuple + Eq + Hash,
    V: Tuple + Ord,
    FK: Fn(&T) -> K,
    FV: Fn(&T) -> V,
    R: Relation<T>,
{
    inner: R,
    key_fn: FK,
    val_fn: FV,
    /// Track all values per key with their counts: key -> (value -> count)
    /// Using BTreeMap so we can efficiently find max
    values: HashMap<K, BTreeMap<V, i64>>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, K, V, FK, FV, R> Relation<(K, V)> for MaxRelation<T, K, V, FK, FV, R>
where
    T: Tuple + 'static,
    K: Tuple + Eq + Hash + 'static,
    V: Tuple + Ord + 'static,
    FK: Fn(&T) -> K + 'static,
    FV: Fn(&T) -> V + 'static,
    R: Relation<T>,
{
    fn foreach(&mut self, consumer: &mut dyn FnMut((K, V), Diff)) {
        let key_fn = &self.key_fn;
        let val_fn = &self.val_fn;
        let values = &mut self.values;

        self.inner.foreach(&mut |t, diff| {
            let k = key_fn(&t);
            let v = val_fn(&t);

            let key_values = values.entry(k.clone()).or_default();

            // Get old max before update
            let old_max = key_values.keys().next_back().cloned();

            // Update the value count
            let count = key_values.entry(v.clone()).or_insert(0);
            *count += diff.0;
            if *count == 0 {
                key_values.remove(&v);
            }

            // Clean up empty key entries
            if key_values.is_empty() {
                values.remove(&k);
            }

            // Get new max after update
            let new_max = values.get(&k).and_then(|kv| kv.keys().next_back().cloned());

            // Output changes if max changed
            if old_max != new_max {
                if let Some(old) = old_max {
                    consumer((k.clone(), old), Diff(-1));
                }
                if let Some(new) = new_max {
                    consumer((k, new), Diff(1));
                }
            }
        });
    }
}

/// Create a max relation - maximum value by key.
pub fn max<T, K, V, FK, FV, R>(input: R, key_fn: FK, val_fn: FV) -> MaxRelation<T, K, V, FK, FV, R>
where
    T: Tuple + 'static,
    K: Tuple + Eq + Hash + 'static,
    V: Tuple + Ord + 'static,
    FK: Fn(&T) -> K + 'static,
    FV: Fn(&T) -> V + 'static,
    R: Relation<T>,
{
    MaxRelation {
        inner: input,
        key_fn,
        val_fn,
        values: HashMap::new(),
        _phantom: std::marker::PhantomData,
    }
}
