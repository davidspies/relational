//! A Relation wrapper around a Variable for reading in feedback loops.

use std::cell::RefCell;
use std::rc::Rc;

use crate::Tuple;
use crate::change::Diff;
use crate::database2::feedback::Variable;

use super::relation::Relation;

/// A relation that reads changes from a Variable.
///
/// Used in feedback loops to connect a Variable's output to downstream operators.
pub struct VariableRelation<T: Tuple> {
    variable: Rc<RefCell<Variable<T>>>,
}

impl<T: Tuple> VariableRelation<T> {
    /// Create a new VariableRelation wrapping the given Variable.
    pub fn new(variable: Rc<RefCell<Variable<T>>>) -> Self {
        VariableRelation { variable }
    }
}

impl<T: Tuple + 'static> Relation<T> for VariableRelation<T> {
    fn foreach(&mut self, consumer: &mut dyn FnMut(T, Diff)) {
        let mut var = self.variable.borrow_mut();
        let changes = var.take_changes();
        for (tuple, diff) in changes {
            consumer(tuple, diff);
        }
    }
}
