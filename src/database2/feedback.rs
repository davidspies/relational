//! Push-based feedback and fixpoint computation.
//!
//! This layer sits on top of the pull-based relational operators to enable
//! recursive computations like transitive closure.
//!
//! The key abstraction is the `Iteration` which:
//! 1. Maintains variables that can be read and written
//! 2. Runs a computation repeatedly until fixpoint
//! 3. Tracks what's been seen to avoid infinite loops

use std::collections::HashMap;

use crate::change::Diff;
use crate::Tuple;


/// A variable in an iterative computation.
///
/// Variables track a "seen set" - tuples are only emitted once when they
/// first become positive. This ensures monotonic growth toward fixpoint.
pub struct Variable<T: Tuple> {
    /// All tuples with their total multiplicities.
    totals: HashMap<T, i64>,
    /// Changes from the current iteration (to be delivered to readers).
    current_changes: Vec<(T, Diff)>,
}

impl<T: Tuple> Variable<T> {
    /// Create a new empty variable.
    pub fn new() -> Self {
        Variable {
            totals: HashMap::new(),
            current_changes: Vec::new(),
        }
    }

    /// Insert an initial value.
    pub fn insert(&mut self, tuple: T) {
        self.add_change(tuple, Diff(1));
    }

    /// Add a change to this variable.
    /// Only emits if the tuple transitions to/from positive.
    pub fn add_change(&mut self, tuple: T, diff: Diff) {
        let total = self.totals.entry(tuple.clone()).or_insert(0);
        let was_positive = *total > 0;
        *total += diff.0;
        let is_positive = *total > 0;

        if !was_positive && is_positive {
            self.current_changes.push((tuple, Diff(1)));
        } else if was_positive && !is_positive {
            self.current_changes.push((tuple, Diff(-1)));
        }
    }

    /// Take the current changes (empties the buffer).
    pub fn take_changes(&mut self) -> Vec<(T, Diff)> {
        std::mem::take(&mut self.current_changes)
    }

    /// Check if there are pending changes.
    pub fn has_changes(&self) -> bool {
        !self.current_changes.is_empty()
    }

    /// Get all positive tuples.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.totals
            .iter()
            .filter(|&(_, &count)| count > 0)
            .map(|(t, _)| t)
    }

    /// Collect all positive tuples into a Vec.
    pub fn collect(&self) -> Vec<T> {
        self.iter().cloned().collect()
    }
}

impl<T: Tuple> Default for Variable<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Run a computation to fixpoint.
///
/// The `step` function takes the current changes and returns new changes
/// to feed back. This continues until `step` returns no new changes.
///
/// Returns (final_variable, iterations).
pub fn fixpoint<T, F>(mut var: Variable<T>, mut step: F, max_iterations: usize) -> (Variable<T>, usize)
where
    T: Tuple,
    F: FnMut(&[(T, Diff)]) -> Vec<(T, Diff)>,
{
    let mut iterations = 0;

    loop {
        let changes = var.take_changes();

        if changes.is_empty() {
            break;
        }

        iterations += 1;

        // Run the step function with current changes
        let new_changes = step(&changes);

        // Feed results back into the variable
        for (t, diff) in new_changes {
            var.add_change(t, diff);
        }

        if iterations >= max_iterations {
            break;
        }
    }

    (var, iterations)
}

/// A helper for running iterative computations with relational operators.
///
/// Usage:
/// ```ignore
/// let mut iter = Iteration::new();
/// let var = iter.variable::<(i32, i32)>();
///
/// // Add initial data
/// for edge in edges {
///     iter.insert(&var, edge);
/// }
///
/// // Define the step: extend paths by joining with edges
/// iter.fixpoint(|changes| {
///     // Use relational operators on changes...
/// });
/// ```
pub struct Iteration<T: Tuple> {
    variable: Variable<T>,
}

impl<T: Tuple> Iteration<T> {
    /// Create a new iteration.
    pub fn new() -> Self {
        Iteration {
            variable: Variable::new(),
        }
    }

    /// Insert initial data.
    pub fn insert(&mut self, tuple: T) {
        self.variable.insert(tuple);
    }

    /// Run to fixpoint.
    ///
    /// The `step` function receives the current changes and a reference
    /// to all accumulated data, and returns new changes to add.
    pub fn run<F>(&mut self, mut step: F, max_iterations: usize) -> usize
    where
        F: FnMut(&[(T, Diff)], &HashMap<T, i64>) -> Vec<(T, Diff)>,
    {
        let mut iterations = 0;

        loop {
            let changes = self.variable.take_changes();

            if changes.is_empty() {
                break;
            }

            iterations += 1;

            // Run step with changes and accumulated state
            let new_changes = step(&changes, &self.variable.totals);

            // Feed back
            for (t, diff) in new_changes {
                self.variable.add_change(t, diff);
            }

            if iterations >= max_iterations {
                break;
            }
        }

        iterations
    }

    /// Get the final results.
    pub fn result(&self) -> &Variable<T> {
        &self.variable
    }

    /// Consume and return the variable.
    pub fn into_variable(self) -> Variable<T> {
        self.variable
    }
}

impl<T: Tuple> Default for Iteration<T> {
    fn default() -> Self {
        Self::new()
    }
}
