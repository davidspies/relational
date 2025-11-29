//! Relational operators for differential dataflow.
//!
//! These operators define how to incrementally maintain derived relations
//! when the input relations change.

mod basic;
mod join;
mod aggregate;
mod group_state;

#[cfg(test)]
mod tests;

pub use basic::{
    distinct, distinct_changes, filter, filter_changes, flat_map, flat_map_changes, map,
    map_changes, negate, negate_changes, union,
};
pub use join::{antijoin, join, join_changes_left, join_changes_right, semijoin};
pub use aggregate::{aggregate, count, max, min, sum};
pub use group_state::{
    group_max_changes, group_max_init, group_min_changes, group_min_init, GroupMaxState,
    GroupMinState,
};
