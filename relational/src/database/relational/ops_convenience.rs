//! Convenience methods for common relation patterns.

use std::{cmp::Reverse, hash::Hash};

use arrayvec::ArrayVec;
use either::Either;

use super::op::Op;
use super::relation::Relation;

impl<R> Relation<R> {
    /// Extract the first element of a tuple relation.
    /// Equivalent to `.map(|(a, _)| a)`.
    pub fn fst<A, B>(self) -> Relation<impl Op<A>>
    where
        R: Op<(A, B)>,
        A: Eq + Hash,
    {
        self.map(|(a, _)| a).with_op_type("fst")
    }

    /// Extract the second element of a tuple relation.
    /// Equivalent to `.map(|(_, b)| b)`.
    pub fn snd<A, B>(self) -> Relation<impl Op<B>>
    where
        R: Op<(A, B)>,
        B: Eq + Hash,
    {
        self.map(|(_, b)| b).with_op_type("snd")
    }

    /// Swap the elements of a tuple relation.
    /// Equivalent to `.map(|(a, b)| (b, a))`.
    pub fn swap<A, B>(self) -> Relation<impl Op<(B, A)>>
    where
        R: Op<(A, B)>,
        A: Eq + Hash,
        B: Eq + Hash,
    {
        self.map_(|(a, b)| (b, a)).with_op_type("swap")
    }

    /// Global maximum - finds the max value across all tuples.
    /// Returns a relation with a single max value.
    pub fn global_max<V>(self) -> Relation<impl Op<V>>
    where
        R: Op<V>,
        V: Clone + Eq + Hash + Ord,
    {
        self.map_h(|v| ((), v))
            .group_max()
            .with_op_type("global_max")
            .map_h(|((), v)| v)
    }

    /// Global minimum - finds the min value across all tuples.
    /// Returns a relation with a single min value.
    pub fn global_min<V>(self) -> Relation<impl Op<V>>
    where
        R: Op<V>,
        V: Clone + Eq + Hash + Ord,
    {
        self.map_h(|v| ((), v))
            .group_min()
            .with_op_type("global_min")
            .map_h(|((), v)| v)
    }

    /// Inner join two relations on matching keys.
    /// Both inputs must be (K, V) tuples. Output is (K, (V1, V2)) tuples.
    /// Implemented as left_join followed by filter to remove None values.
    pub fn join<K, V1, V2, RR>(self, right: Relation<RR>) -> Relation<impl Op<(K, (V1, V2))>>
    where
        R: Op<(K, V1)>,
        K: Clone + Eq + Hash,
        V1: Clone + Eq + Hash,
        V2: Clone + Eq + Hash,
        RR: Op<(K, V2)>,
    {
        self.left_join(right)
            .with_op_type("join")
            .filter_map_h(|(k, (v1, opt_v2))| opt_v2.map(|v2| (k, (v1, v2))))
    }

    /// Join two relations and discard the key.
    /// Both inputs must be `(K, V)` tuples. Output is `(V1, V2)` pairs.
    pub fn join_values<K, V1, V2, RR>(self, right: Relation<RR>) -> Relation<impl Op<(V1, V2)>>
    where
        R: Op<(K, V1)>,
        K: Clone + Eq + Hash,
        V1: Clone + Eq + Hash,
        V2: Clone + Eq + Hash,
        RR: Op<(K, V2)>,
    {
        self.join(right)
            .with_op_type("join_values")
            .map_h(|(_k, (v1, v2))| (v1, v2))
    }

    /// Cartesian product - pairs every tuple from left with every tuple from right.
    /// Output is `(T1, T2)` pairs.
    pub fn cartesian_product<T1, T2, RR>(self, right: Relation<RR>) -> Relation<impl Op<(T1, T2)>>
    where
        R: Op<T1>,
        T1: Clone + Eq + Hash,
        T2: Clone + Eq + Hash,
        RR: Op<T2>,
    {
        self.map_h(|t| ((), t))
            .join(right.map_h(|t| ((), t)))
            .with_op_type("cartesian_product")
            .map_h(|((), (t1, t2))| (t1, t2))
    }

    /// Semijoin - filter left relation to only tuples that have a matching key in right.
    /// Input: left is `(K, V)`, right is any relation of keys `K`.
    /// Output: `(K, V)` tuples from left where key exists in right.
    pub fn semijoin<K, V, RR>(self, right: Relation<RR>) -> Relation<impl Op<(K, V)>>
    where
        R: Op<(K, V)>,
        K: Clone + Eq + Hash,
        V: Clone + Eq + Hash,
        RR: Op<K>,
    {
        self.join(right.map_h(|k| (k, ())))
            .with_op_type("semijoin")
            .map_h(|(k, (v, ()))| (k, v))
    }

    /// Left lookup - for each key in self, look up a value in right.
    /// Input: self is `K`, right is `(K, V)`.
    /// Output: `(K, Option<V>)` - Some(V) if key exists in right, None otherwise.
    /// A key appears in output iff it appears in self (the left/key relation).
    pub fn left_lookup<K, V, RR>(self, right: Relation<RR>) -> Relation<impl Op<(K, Option<V>)>>
    where
        R: Op<K>,
        K: Clone + Eq + Hash,
        V: Clone + Eq + Hash,
        RR: Op<(K, V)>,
    {
        self.map_h(|k| (k, ()))
            .left_join(right)
            .with_op_type("left_lookup")
            .map_h(|(k, ((), opt_v))| (k, opt_v))
    }

    /// Set difference (self - right).
    /// Implemented via antijoin: treat tuples as (T, ()) pairs.
    pub fn set_minus<T, RR>(self, right: Relation<RR>) -> Relation<impl Op<T>>
    where
        T: Clone + Eq + Hash,
        R: Op<T>,
        RR: Op<T>,
    {
        self.map_h(|t: T| (t, ()))
            .antijoin(right)
            .with_op_type("set_minus")
            .map_h(|(t, ())| t)
    }

    /// Set intersection (self ∩ right).
    /// Keeps only tuples that appear in both relations.
    pub fn intersection<T, RR>(self, right: Relation<RR>) -> Relation<impl Op<T>>
    where
        T: Clone + Eq + Hash,
        R: Op<T>,
        RR: Op<T>,
    {
        self.map_h(|t: T| (t, ()))
            .semijoin(right)
            .with_op_type("intersection")
            .map_h(|(t, ())| t)
    }

    /// Count tuples by key.
    /// Input must be (K, V) tuples where K is the key.
    /// Output is (K, count) pairs.
    pub fn counts<T>(self) -> Relation<impl Op<(T, i64)>>
    where
        R: Op<T>,
        T: Clone + Eq + Hash,
    {
        self.map_h(|t| (t, 1i64)).group_sum().with_op_type("counts")
    }

    /// Minimum value by key (without consolidation).
    /// Input must be (K, V) tuples where K is the key and V is the value.
    pub fn group_min<K, V>(self) -> Relation<impl Op<(K, V)>>
    where
        R: Op<(K, V)>,
        K: Clone + Eq + Hash,
        V: Clone + Eq + Hash + Ord,
    {
        self.group_min_n::<K, V, 1>()
            .with_op_type("group_min")
            .map_h(|(k, arr)| {
                let mut iter = arr.into_iter();
                let v = iter.next().unwrap();
                assert!(iter.next().is_none());
                (k, v)
            })
    }

    /// Maximum value by key.
    /// Input must be (K, V) tuples where K is the key and V is the value.
    pub fn group_max<K, V>(self) -> Relation<impl Op<(K, V)>>
    where
        R: Op<(K, V)>,
        K: Clone + Eq + Hash,
        V: Clone + Eq + Hash + Ord,
    {
        self.map_h(|(k, v)| (k, Reverse(v)))
            .group_min()
            .with_op_type("group_max")
            .map_h(|(k, Reverse(v))| (k, v))
    }

    pub fn group_max_n<K, V, const N: usize>(self) -> Relation<impl Op<(K, ArrayVec<V, N>)>>
    where
        R: Op<(K, V)>,
        K: Clone + Eq + Hash,
        V: Clone + Eq + Hash + Ord,
    {
        self.map_h(|(k, v)| (k, Reverse(v)))
            .group_min_n::<K, Reverse<V>, N>()
            .with_op_type("group_max_n")
            .map_h(|(k, arr)| (k, arr.into_iter().map(|Reverse(v)| v).collect()))
    }

    /// Filter tuples by predicate.
    pub fn filter<T, F>(self, pred: F) -> Relation<impl Op<T>>
    where
        R: Op<T>,
        F: Fn(&T) -> bool,
        T: Eq + Hash,
    {
        self.flat_map(move |t| if pred(&t) { Some(t) } else { None })
            .with_op_type("filter")
    }

    pub fn filter_map<T, U, F>(self, f: F) -> Relation<impl Op<U>>
    where
        R: Op<T>,
        F: Fn(T) -> Option<U>,
        U: Eq + Hash,
    {
        self.flat_map(f).with_op_type("filter_map")
    }

    pub fn filter_map_h<T, U, F>(self, f: F) -> Relation<impl Op<U>>
    where
        R: Op<T>,
        F: Fn(T) -> Option<U>,
    {
        self.flat_map_h(f)
    }

    pub fn partition<A, B>(self) -> (Relation<impl Op<A>>, Relation<impl Op<B>>)
    where
        R: Op<Either<A, B>>,
        A: Eq + Hash,
        B: Eq + Hash,
    {
        let (l, r) = self
            .map_h(|e| match e {
                Either::Left(a) => (Some(a), None),
                Either::Right(b) => (None, Some(b)),
            })
            .split();
        (
            l.with_op_type("partition_left").filter_map_h(|opt_a| opt_a),
            r.with_op_type("partition_right")
                .filter_map_h(|opt_b| opt_b),
        )
    }
}
