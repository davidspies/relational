//! Interrupt wrapper for type-erased interrupt operations.

use crate::database::relational::Op;

/// Type-erased interrupt operations.
pub(crate) trait AnyInterrupt {
    /// Check if the interrupt condition is met (relation has positive entries).
    fn check(&mut self) -> bool;
    /// Reset the interrupt state (clear has_positive flag).
    fn reset(&mut self);
}

/// Wrapper for interrupt - checks if a relation has any positive entries.
pub(crate) struct InterruptWrapper<T, R: Op<T>> {
    input: R,
    has_positive: bool,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, R: Op<T>> InterruptWrapper<T, R> {
    pub(crate) fn new(input: R) -> Self {
        InterruptWrapper {
            input,
            has_positive: false,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T, R: Op<T>> AnyInterrupt for InterruptWrapper<T, R> {
    fn check(&mut self) -> bool {
        let has_positive = &mut self.has_positive;
        self.input.foreach(|_, diff| {
            if diff.0 > 0 {
                *has_positive = true;
            }
        });
        self.has_positive
    }

    fn reset(&mut self) {
        self.has_positive = false;
    }
}
