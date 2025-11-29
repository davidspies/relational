//! Composable relation handles for building dataflow graphs.

use std::marker::PhantomData;

use crate::dataflow::NodeId;

/// A handle to a relation in the dataflow graph.
///
/// Relations are immutable references to nodes - operations on them
/// create new derived relations rather than mutating in place.
#[derive(Clone, Copy)]
pub struct Relation<T> {
    pub(crate) id: NodeId,
    pub(crate) _phantom: PhantomData<T>,
}

impl<T> Relation<T> {
    pub(crate) fn new(id: NodeId) -> Self {
        Relation {
            id,
            _phantom: PhantomData,
        }
    }

    pub fn id(&self) -> NodeId {
        self.id
    }
}

/// A variable that can receive feedback in a fixed-point computation.
///
/// This is the "input side" of a feedback loop - you feed data into it,
/// and it merges with the recursive output.
#[derive(Clone, Copy)]
pub struct Variable<T> {
    pub(crate) id: NodeId,
    pub(crate) _phantom: PhantomData<T>,
}

impl<T> Variable<T> {
    pub(crate) fn new(id: NodeId) -> Self {
        Variable {
            id,
            _phantom: PhantomData,
        }
    }

    /// Get a relation handle for reading from this variable.
    pub fn as_relation(&self) -> Relation<T> {
        Relation::new(self.id)
    }

    pub fn id(&self) -> NodeId {
        self.id
    }
}
