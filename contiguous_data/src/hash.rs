//! Hash map and set type aliases that switch based on feature flags.
//!
//! Options (mutually exclusive, checked at compile time):
//! - Default: `ahash` - fast, DoS-resistant
//! - `fx-hash`: `rustc-hash` - faster for small integer keys, not DoS-resistant
//! - `std-hash`: standard library - for debugging or deterministic ordering

#[cfg(feature = "std-hash")]
pub use std::collections::{HashMap, HashSet};

#[cfg(not(feature = "std-hash"))]
pub use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
