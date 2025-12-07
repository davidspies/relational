//! Hash map and set type aliases that switch between ahash and std based on feature flags.
//!
//! By default, uses `ahash` for better performance. Enable the `std-hash` feature
//! to use the standard library's hash collections instead (useful for debugging
//! or when deterministic ordering is needed).

#[cfg(not(feature = "std-hash"))]
pub use ahash::{AHashMap as HashMap, AHashSet as HashSet};

#[cfg(feature = "std-hash")]
pub use std::collections::{HashMap, HashSet};
