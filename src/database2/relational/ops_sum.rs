//! Sum operator - sum values by key.

use std::collections::HashMap;
use std::hash::Hash;
use std::ops::{Add, Mul, Sub};

use crate::change::Diff;
use crate::Tuple;

use super::relation::Relation;

/// A sum relation - sums values by key.
/// Output is (key, sum) pairs.
pub struct SumRelation<T, K, V, FK, FV, R>
where
    T: Tuple,
    K: Tuple + Eq + Hash,
    V: Tuple + Add<Output = V> + Sub<Output = V> + Mul<i64, Output = V> + Default + PartialEq,
    FK: Fn(&T) -> K,
    FV: Fn(&T) -> V,
    R: Relation<T>,
{
    inner: R,
    key_fn: FK,
    val_fn: FV,
    /// Track sum per key
    sums: HashMap<K, V>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, K, V, FK, FV, R> Relation<(K, V)> for SumRelation<T, K, V, FK, FV, R>
where
    T: Tuple + 'static,
    K: Tuple + Eq + Hash + 'static,
    V: Tuple + Add<Output = V> + Sub<Output = V> + Mul<i64, Output = V> + Default + PartialEq + 'static,
    FK: Fn(&T) -> K + 'static,
    FV: Fn(&T) -> V + 'static,
    R: Relation<T>,
{
    fn foreach(&mut self, consumer: &mut dyn FnMut((K, V), Diff)) {
        let key_fn = &self.key_fn;
        let val_fn = &self.val_fn;
        let sums = &mut self.sums;

        self.inner.foreach(&mut |t, diff| {
            let k = key_fn(&t);
            let v = val_fn(&t);
            let delta = v * diff.0;

            let old_sum = sums.get(&k).cloned().unwrap_or_default();
            let new_sum = old_sum.clone() + delta;

            let zero = V::default();
            if new_sum == zero {
                sums.remove(&k);
            } else {
                sums.insert(k.clone(), new_sum.clone());
            }

            // Output: delete old (key, sum), insert new (key, sum)
            if old_sum != zero {
                consumer((k.clone(), old_sum), Diff(-1));
            }
            if new_sum != zero {
                consumer((k, new_sum), Diff(1));
            }
        });
    }
}

/// Create a sum relation - sums values by key.
pub fn sum<T, K, V, FK, FV, R>(input: R, key_fn: FK, val_fn: FV) -> SumRelation<T, K, V, FK, FV, R>
where
    T: Tuple + 'static,
    K: Tuple + Eq + Hash + 'static,
    V: Tuple + Add<Output = V> + Sub<Output = V> + Mul<i64, Output = V> + Default + PartialEq + 'static,
    FK: Fn(&T) -> K + 'static,
    FV: Fn(&T) -> V + 'static,
    R: Relation<T>,
{
    SumRelation {
        inner: input,
        key_fn,
        val_fn,
        sums: HashMap::new(),
        _phantom: std::marker::PhantomData,
    }
}
