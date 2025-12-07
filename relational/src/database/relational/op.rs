use std::{cell::RefCell, hash::Hash, rc::Rc};

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

    fn dump_to_consumers(&mut self, consumers: &[Rc<RefCell<Multiset<T>>>])
    where
        T: Clone + Eq + Hash,
    {
        self.foreach(|t, diff| {
            for consumer in consumers {
                consumer.borrow_mut().update(t.clone(), diff);
            }
        });
    }

    fn dump_split(&mut self, left: &mut Multiset<T::A>, right: &mut Multiset<T::B>)
    where
        T: IsPair,
    {
        self.foreach(|t, diff| {
            let (a, b) = t.unpack();
            left.update(a, diff);
            right.update(b, diff);
        });
    }

    /// Box this operator to allow type erasure.
    /// Use this when the compiler struggles with deeply nested types.
    fn boxed<'a>(self) -> Box<dyn DynOp<T> + 'a>
    where
        Self: 'a,
    {
        Box::new(self)
    }

    fn passthrough_op_type(&self, _name: &'static str) -> bool {
        false
    }
}

pub trait DynOp<T> {
    fn foreach_dyn(&mut self, f: &mut dyn FnMut(T, Diff));

    fn dump_to_multiset(&mut self, multiset: &mut Multiset<T>) -> usize
    where
        T: Eq + Hash;

    fn dump_to_consumers(&mut self, consumers: &[Rc<RefCell<Multiset<T>>>])
    where
        T: Clone + Eq + Hash;

    fn dump_split(&mut self, left: &mut Multiset<T::A>, right: &mut Multiset<T::B>)
    where
        T: IsPair;
}

impl<T, R: Op<T>> DynOp<T> for R {
    fn foreach_dyn(&mut self, f: &mut dyn FnMut(T, Diff)) {
        self.foreach(f);
    }

    fn dump_to_multiset(&mut self, multiset: &mut Multiset<T>) -> usize
    where
        T: Eq + Hash,
    {
        Op::dump_to_multiset(self, multiset)
    }

    fn dump_to_consumers(&mut self, consumers: &[Rc<RefCell<Multiset<T>>>])
    where
        T: Clone + Eq + Hash,
    {
        Op::dump_to_consumers(self, consumers);
    }

    fn dump_split(&mut self, left: &mut Multiset<T::A>, right: &mut Multiset<T::B>)
    where
        T: IsPair,
    {
        Op::dump_split(self, left, right);
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
        (**self).dump_to_multiset(multiset)
    }

    fn dump_to_consumers(&mut self, consumers: &[Rc<RefCell<Multiset<T>>>])
    where
        T: Clone + Eq + Hash,
    {
        (**self).dump_to_consumers(consumers);
    }

    fn dump_split(&mut self, left: &mut Multiset<T::A>, right: &mut Multiset<T::B>)
    where
        T: IsPair,
    {
        (**self).dump_split(left, right);
    }

    fn boxed<'a>(self) -> Box<dyn DynOp<T> + 'a> {
        self
    }
}

pub trait IsPair {
    type A: Eq + Hash;
    type B: Eq + Hash;

    fn unpack(self) -> (Self::A, Self::B);
}

impl<A: Eq + Hash, B: Eq + Hash> IsPair for (A, B) {
    type A = A;
    type B = B;

    fn unpack(self) -> (Self::A, Self::B) {
        self
    }
}
