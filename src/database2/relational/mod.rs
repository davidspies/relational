//! Pull-based relational operators.
//!
//! This module implements a dataflow system where:
//! - Relations are streams of changes (tuple, diff) pairs
//! - `foreach` iterates over pending changes, pulling from upstream
//! - Operators are generic over their input relation types
//! - Use `.boxed()` to break type chains when needed

pub(crate) mod input;
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
mod relation;
mod saved;
mod variable_relation;

// Core types - InputHandle and InputRelation are created via Database2::create_input()
pub use input::{InputHandle, InputRelation};
pub use relation::Relation;
pub use saved::{save, SavedGetter, SavedRelation};
pub use variable_relation::VariableRelation;

// Operators
pub use ops_difference::{difference, negate, DifferenceRelation, NegateRelation};
pub use ops_distinct::{distinct, DistinctRelation};
pub use ops_filter::filter;
pub use ops_flat_map::{flat_map, FlatMapRelation};
pub use ops_join::{join, JoinRelation};
pub use ops_map::map;
pub use ops_max::{max, MaxRelation};
pub use ops_min::min;
pub use ops_sum::{sum, SumRelation};
pub use ops_union::{union, UnionRelation};
