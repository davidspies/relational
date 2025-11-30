//! Relational operators for differential dataflow.
//!
//! These operators define how to incrementally maintain derived relations
//! when the input relations change.

mod aggregate;
mod basic;
mod group_max_state;
mod group_sum_state;
mod join;

#[cfg(test)]
mod tests;

pub use aggregate::{aggregate, count, max, min, sum};
pub use basic::{
    distinct, distinct_changes, filter, filter_changes, flat_map, flat_map_changes, map,
    map_changes, negate, negate_changes, union,
};
pub use group_max_state::{GroupMaxState, group_max_changes, group_max_init};
pub use group_sum_state::{GroupSumState, group_sum_changes, group_sum_init};
pub use join::{antijoin, join, join_changes_left, join_changes_right, semijoin};
