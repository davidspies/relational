mod l2_heaps;
mod l2_multiset;
mod multiset;

pub use l2_heaps::L2Heaps;
pub use l2_multiset::L2Multiset;
pub use multiset::Multiset;

/// A difference/multiplicity value. Positive means insertions, negative means deletions.
pub type Diff = i64;
