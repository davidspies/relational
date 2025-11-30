//! Pull-based relational operators.
//!
//! This module implements a dataflow system where:
//! - Relations are streams of changes (tuple, diff) pairs
//! - `foreach` iterates over pending changes, pulling from upstream
//! - Operators are generic over their input relation types
//! - Use `.boxed()` to break type chains when needed

pub(crate) mod input;
mod ops_count;
mod ops_difference;
mod ops_distinct;
mod ops_filter;
mod ops_flat_map;
mod ops_join;
mod ops_map;
mod ops_max;
mod ops_min;
mod ops_sum;
mod ops_union;
mod output;
mod relation;
pub(crate) mod saved;
mod sink;
mod variable_relation;

// Core types - InputHandle and InputRelation are created via Database2::create_input()
pub use input::{InputHandle, PersistentInputHandle};
pub use relation::Relation;
pub use saved::{SavedRelation, save};
pub use variable_relation::{Variable, VariableRelation};

// Operators
pub use ops_count::count;
pub use ops_difference::{difference, negate};
pub use ops_distinct::distinct;
pub use ops_filter::filter;
pub use ops_flat_map::flat_map;
pub use ops_join::join;
pub use ops_map::map;
pub use ops_max::max;
pub use ops_min::min;
pub use ops_sum::sum;
pub use ops_union::union;
pub use output::{Output, output, output_with_sink};
pub use sink::Sink;
