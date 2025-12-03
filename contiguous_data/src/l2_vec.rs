//! L2Vec - a Vec<Vec<T>> with efficient storage where only the last Vec is mutable.

/// A `Vec<Vec<T>>` equivalent with contiguous storage.
///
/// Only the last inner vec is mutable. Implemented as a flat `Vec<T>`
/// plus `Vec<usize>` for start indices.
#[derive(Clone, Debug)]
pub struct L2Vec<T> {
    data: Vec<T>,
    /// Start index of each inner vec. Length is number of inner vecs.
    /// The i-th inner vec spans data[starts[i]..starts[i+1]] (or data[starts[i]..] for last).
    starts: Vec<usize>,
}

impl<T> L2Vec<T> {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            starts: Vec::new(),
        }
    }

    /// Number of inner vecs.
    pub fn len(&self) -> usize {
        self.starts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.starts.is_empty()
    }

    /// Push a new empty inner vec.
    pub fn push_empty(&mut self) {
        self.starts.push(self.data.len());
    }

    /// Push a value to the last inner vec. Panics if empty.
    #[track_caller]
    pub fn push(&mut self, value: T) {
        assert!(!self.starts.is_empty(), "No inner vec to push to");
        self.data.push(value);
    }

    /// Pop the last inner vec. Returns an iterator over its elements.
    pub fn pop(&mut self) -> Option<impl Iterator<Item = T> + '_> {
        let start = self.starts.pop()?;
        Some(self.data.drain(start..))
    }

    /// Get the i-th inner vec as a slice.
    pub fn get(&self, index: usize) -> Option<&[T]> {
        if index >= self.starts.len() {
            return None;
        }
        let start = self.starts[index];
        let end = self.starts.get(index + 1).copied().unwrap_or(self.data.len());
        Some(&self.data[start..end])
    }

    /// Get the last inner vec as a slice.
    pub fn last(&self) -> Option<&[T]> {
        let start = *self.starts.last()?;
        Some(&self.data[start..])
    }

    /// Get the last inner vec as a mutable slice.
    pub fn last_mut(&mut self) -> Option<&mut [T]> {
        let start = *self.starts.last()?;
        Some(&mut self.data[start..])
    }

    /// Iterate over all inner vecs as slices.
    pub fn iter(&self) -> impl Iterator<Item = &[T]> {
        (0..self.starts.len()).map(|i| self.get(i).unwrap())
    }
}

impl<T> Default for L2Vec<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic() {
        let mut v: L2Vec<i32> = L2Vec::new();
        assert!(v.is_empty());

        v.push_empty();
        v.push(1);
        v.push(2);
        assert_eq!(v.get(0), Some(&[1, 2][..]));

        v.push_empty();
        v.push(3);
        assert_eq!(v.get(0), Some(&[1, 2][..]));
        assert_eq!(v.get(1), Some(&[3][..]));
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn test_pop() {
        let mut v: L2Vec<i32> = L2Vec::new();
        v.push_empty();
        v.push(1);
        v.push(2);
        v.push_empty();
        v.push(3);

        let popped: Vec<_> = v.pop().unwrap().collect();
        assert_eq!(popped, vec![3]);
        assert_eq!(v.len(), 1);
        assert_eq!(v.get(0), Some(&[1, 2][..]));

        let popped: Vec<_> = v.pop().unwrap().collect();
        assert_eq!(popped, vec![1, 2]);
        assert!(v.is_empty());
        assert!(v.pop().is_none());
    }

    #[test]
    fn test_iter() {
        let mut v: L2Vec<i32> = L2Vec::new();
        v.push_empty();
        v.push(1);
        v.push_empty();
        v.push(2);
        v.push(3);
        v.push_empty();

        let vecs: Vec<_> = v.iter().collect();
        assert_eq!(vecs, vec![&[1][..], &[2, 3][..], &[][..]]);
    }

    #[test]
    fn test_last_mut() {
        let mut v: L2Vec<i32> = L2Vec::new();
        v.push_empty();
        v.push(1);
        v.push(2);

        if let Some(last) = v.last_mut() {
            last[0] = 10;
        }
        assert_eq!(v.get(0), Some(&[10, 2][..]));
    }
}
