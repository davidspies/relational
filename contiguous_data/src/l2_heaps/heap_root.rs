//! HeapRoot enum and helper methods.

use arrayvec::ArrayVec;
use index_list::Index;

pub(crate) enum HeapRoot<V, const N: usize> {
    /// 0 to N elements, kept sorted.
    Small(ArrayVec<V, N>),
    /// N elements in top (sorted), plus overflow in heap.
    Large {
        top: ArrayVec<V, N>,
        root: Index,
        heap_size: usize,
    },
}

impl<V, const N: usize> HeapRoot<V, N> {
    pub(super) fn as_small(&self) -> &ArrayVec<V, N> {
        match self {
            HeapRoot::Small(arr) => arr,
            HeapRoot::Large { .. } => panic!("expected Small"),
        }
    }

    pub(super) fn as_small_mut(&mut self) -> &mut ArrayVec<V, N> {
        match self {
            HeapRoot::Small(arr) => arr,
            HeapRoot::Large { .. } => panic!("expected Small"),
        }
    }

    pub(super) fn as_top(&self) -> &ArrayVec<V, N> {
        match self {
            HeapRoot::Large { top, .. } => top,
            HeapRoot::Small(_) => panic!("expected Large"),
        }
    }

    pub(super) fn as_top_mut(&mut self) -> &mut ArrayVec<V, N> {
        match self {
            HeapRoot::Large { top, .. } => top,
            HeapRoot::Small(_) => panic!("expected Large"),
        }
    }

    pub(super) fn inc_heap_size(&mut self) {
        match self {
            HeapRoot::Large { heap_size, .. } => *heap_size += 1,
            HeapRoot::Small(_) => panic!("expected Large"),
        }
    }
}
