use arrayvec::ArrayVec;
use index_list::Index;

pub(super) enum Root<V> {
    Small(ArrayVec<V, 2>),
    Large { head: Index, len: usize },
}

impl<V> Root<V> {
    pub(super) fn as_small(&self) -> &ArrayVec<V, 2> {
        match self {
            Root::Small(arr) => arr,
            Root::Large { .. } => panic!("expected Small"),
        }
    }

    pub(super) fn as_small_mut(&mut self) -> &mut ArrayVec<V, 2> {
        match self {
            Root::Small(arr) => arr,
            Root::Large { .. } => panic!("expected Small"),
        }
    }

    pub(super) fn inc_len(&mut self) {
        match self {
            Root::Large { len, .. } => *len += 1,
            Root::Small(_) => panic!("expected Large"),
        }
    }

    pub(super) fn dec_len(&mut self) {
        match self {
            Root::Large { len, .. } => *len -= 1,
            Root::Small(_) => panic!("expected Large"),
        }
    }
}
