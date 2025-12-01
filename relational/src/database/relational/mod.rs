//! Pull-based relational operators.
//!
//! This module implements a dataflow system where:
//! - Relations are streams of changes (tuple, diff) pairs
//! - `foreach` iterates over pending changes, pulling from upstream
//! - Operators are generic over their input relation types
//! - Use `.boxed()` to break type chains when needed

pub(crate) mod graph;
pub(crate) mod input;
mod ops_antijoin;
mod ops_consolidate;
mod ops_convenience;
mod ops_count;
mod ops_distinct;
mod ops_filter;
mod ops_flat_map;
mod ops_join;
mod ops_map;
mod ops_max;
mod ops_min;
mod ops_negate;
mod ops_sum;
mod ops_union;
mod output;
mod relation;
pub(crate) mod saved;
mod sink;
mod variable_relation;

// Core types - InputHandle and InputRelation are created via Database2::create_input()
pub use input::{InputHandle, PersistentInputHandle};
pub use relation::{Op, Relation};
pub use saved::SavedRelation;
pub use variable_relation::{Variable, VariableRelation};

// Output
pub use output::{Output, SavedOutput, output, output_with_sink};
pub use sink::Sink;

// Graph exports (only public types)
pub use graph::{Graph, GraphHandle, NodeId};
