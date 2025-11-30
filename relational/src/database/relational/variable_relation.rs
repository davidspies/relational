//! A Relation wrapper around a Variable for reading in feedback loops.

use std::cell::RefCell;
use std::rc::Rc;

use crate::Tuple;
use crate::change::Diff;
use crate::database::feedback::Variable as InternalVariable;

use super::relation::Relation;

/// A handle for a feedback variable.
///
/// Created by `Database::create_variable()` and passed to `Database::feedback()`
/// to wire up the input relation.
pub struct Variable<T: Tuple> {
    pub(crate) inner: Rc<RefCell<InternalVariable<T>>>,
}

impl<T: Tuple> Clone for Variable<T> {
    fn clone(&self) -> Self {
        Variable {
            inner: self.inner.clone(),
        }
    }
}

/// A relation that reads changes from a feedback variable.
///
/// Created by `Database::create_variable()`. Use this in your dataflow graph
/// to read the variable's output.
pub struct VariableRelation<T: Tuple> {
    pub(crate) inner: Rc<RefCell<InternalVariable<T>>>,
}

impl<T: Tuple + 'static> Relation<T> for VariableRelation<T> {
    fn foreach(&mut self, consumer: &mut dyn FnMut(T, Diff)) {
        let mut var = self.inner.borrow_mut();
        let changes = var.take_changes();
        for (tuple, diff) in changes {
            consumer(tuple, diff);
        }
    }
}
