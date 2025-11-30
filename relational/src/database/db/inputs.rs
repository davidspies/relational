//! Input and variable creation for Database.

use std::cell::RefCell;
use std::hash::Hash;
use std::rc::Rc;

use crate::database::feedback::Variable as InternalVariable;
use crate::database::relational::input::{InputRelation, InputState};
use crate::database::relational::{
    InputHandle, PersistentInputHandle, Relation, Variable, VariableRelation,
};

use super::Database;
use super::wrappers::InputWrapper;

impl Database {
    /// Create an input and register it with the database.
    ///
    /// Returns a handle for inserting/deleting tuples and a relation for reading.
    /// Changes are staged until `db.commit()` is called.
    pub fn create_input<T: Clone + Eq + Hash + 'static>(
        &mut self,
    ) -> (InputHandle<T>, Relation<InputRelation<T>>) {
        let state = Rc::new(RefCell::new(InputState::new()));

        let handle = InputHandle {
            state: state.clone(),
        };
        let relation = Relation::new(
            InputRelation {
                state: state.clone(),
            },
            self.commit_id.clone(),
            self.graph.clone(),
            "input",
            vec![],
        );

        let mut wrapper = InputWrapper::new(state);
        wrapper.push_initial_checkpoints(self.checkpoint_depth);

        self.inputs.push(Box::new(wrapper));
        (handle, relation)
    }

    /// Create a persistent input that survives pop().
    ///
    /// Like `create_input`, but changes are not undone when `pop()` is called.
    /// Use this for learned clauses, facts that should persist through backtracking, etc.
    pub fn create_persistent_input<T: Clone + Eq + Hash>(
        &mut self,
    ) -> (PersistentInputHandle<T>, Relation<InputRelation<T>>) {
        let state = Rc::new(RefCell::new(InputState::new()));

        let handle = PersistentInputHandle {
            state: state.clone(),
        };
        let relation = Relation::new(
            InputRelation {
                state: state.clone(),
            },
            self.commit_id.clone(),
            self.graph.clone(),
            "persistent_input",
            vec![],
        );

        (handle, relation)
    }

    /// Create a feedback variable.
    ///
    /// Returns a `Variable` handle (to pass to `feedback()`) and a `Relation<VariableRelation>`
    /// (to use in your dataflow graph).
    pub fn create_variable<T: Clone + Eq + Hash>(
        &self,
    ) -> (Variable<T>, Relation<VariableRelation<T>>) {
        let inner = Rc::new(RefCell::new(InternalVariable::new()));
        let var = Variable {
            inner: inner.clone(),
        };
        let rel = Relation::new(
            VariableRelation { inner },
            self.commit_id.clone(),
            self.graph.clone(),
            "variable",
            vec![],
        );
        (var, rel)
    }
}
