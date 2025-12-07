pub mod hash;
mod l2_heaps;
mod l2_multiset;
mod l2_vec;
mod multiset;

pub use hash::{HashMap, HashSet};
pub use l2_heaps::L2Heaps;
pub use l2_multiset::L2Multiset;
pub use l2_vec::L2Vec;
pub use multiset::Multiset;

/// A difference/multiplicity value. Positive means insertions, negative means deletions.
pub type Diff = i64;
