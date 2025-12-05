use std::hash::Hash;

use contiguous_data::{Diff, Multiset};

/// The core trait for relational operators.
/// An operator is a stream of changes - call foreach to iterate over pending changes.
pub trait Op<T>: Sized {
    /// Iterate over pending changes, calling f for each (tuple, count) pair.
    fn foreach(&mut self, f: impl FnMut(T, Diff));

    fn dump_to_multiset(&mut self, multiset: &mut Multiset<T>) -> usize
    where
        T: Eq + Hash,
    {
        let mut counter = 0;
        self.foreach(|t, diff| {
            multiset.update(t, diff);
            counter += 1;
        });
        counter
    }

    /// Box this operator to allow type erasure.
    /// Use this when the compiler struggles with deeply nested types.
    fn boxed<'a>(self) -> Box<dyn DynOp<T> + 'a>
    where
        Self: 'a,
    {
        Box::new(self)
    }

    fn passthrough_op_name(&self, _name: &'static str) -> bool {
        false
    }
}

pub trait DynOp<T> {
    fn foreach_dyn(&mut self, f: &mut dyn FnMut(T, Diff));

    fn dump_to_multiset_dyn(&mut self, multiset: &mut Multiset<T>) -> usize
    where
        T: Eq + Hash;
}

impl<T, R: Op<T>> DynOp<T> for R {
    fn foreach_dyn(&mut self, f: &mut dyn FnMut(T, Diff)) {
        self.foreach(f);
    }

    fn dump_to_multiset_dyn(&mut self, multiset: &mut Multiset<T>) -> usize
    where
        T: Eq + Hash,
    {
        self.dump_to_multiset(multiset)
    }
}

/// Implement Op for Box<dyn DynOp<T>> to allow type erasure.
impl<T> Op<T> for Box<dyn DynOp<T>> {
    fn foreach(&mut self, mut f: impl FnMut(T, Diff)) {
        (**self).foreach_dyn(&mut f);
    }

    fn dump_to_multiset(&mut self, multiset: &mut Multiset<T>) -> usize
    where
        T: Eq + Hash,
    {
        (**self).dump_to_multiset_dyn(multiset)
    }

    fn boxed<'a>(self) -> Box<dyn DynOp<T> + 'a> {
        self
    }
}
