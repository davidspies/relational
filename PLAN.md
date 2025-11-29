# Refactor Plan: Pull-Based Differential Dataflow

## Goals

1. **Pull semantics** - `foreach()` iterates over pending changes, calling a callback for each
2. **Purely differential** - No `state()` method, everything is changes flowing through
3. **Move-only Relations** - `Relation<T>` is not Clone, pass by value to build graph
4. **SavedRelation for reuse** - To use a relation multiple times, save it first
5. **Operators hold needed state** - Stateful operators (distinct, join) track input internally

## Core Design

### The Relation Trait

```rust
/// A relation is a stream of changes.
/// Call foreach to iterate over pending changes.
trait Relation<T: Tuple> {
    fn foreach(&mut self, f: &mut dyn FnMut(&T, Count));
}
```

That's it. One method. For each pending change, call the callback with the tuple and its count delta.

### Stateless Operators

Stateless operators just transform changes as they flow through:

```rust
struct MapRelation<T, U, F, R> {
    inner: R,  // R: Relation<T>
    f: F,      // F: Fn(&T) -> U
}

impl<T, U, F, R> Relation<U> for MapRelation<T, U, F, R>
where
    R: Relation<T>,
    F: Fn(&T) -> U,
{
    fn foreach(&mut self, consumer: &mut dyn FnMut(&U, Count)) {
        let f = &self.f;
        self.inner.foreach(&mut |t, count| {
            consumer(&f(t), count);
        });
    }
}

struct FilterRelation<T, F, R> {
    inner: R,  // R: Relation<T>
    pred: F,   // F: Fn(&T) -> bool
}

impl<T, F, R> Relation<T> for FilterRelation<T, F, R>
where
    R: Relation<T>,
    F: Fn(&T) -> bool,
{
    fn foreach(&mut self, consumer: &mut dyn FnMut(&T, Count)) {
        let pred = &self.pred;
        self.inner.foreach(&mut |t, count| {
            if pred(t) {
                consumer(t, count);
            }
        });
    }
}

struct UnionRelation<T, L, R> {
    left: L,   // L: Relation<T>
    right: R,  // R: Relation<T>
}

impl<T, L, R> Relation<T> for UnionRelation<T, L, R>
where
    L: Relation<T>,
    R: Relation<T>,
{
    fn foreach(&mut self, consumer: &mut dyn FnMut(&T, Count)) {
        self.left.foreach(consumer);
        self.right.foreach(consumer);
    }
}
```

### Stateful Operators

Stateful operators need to track input state to compute correct output deltas:

```rust
struct DistinctRelation<T, R> {
    inner: R,
    /// Track input multiplicities to know when count crosses 0/1 boundary
    input_counts: HashMap<T, Count>,
}

impl<T: Tuple, R: Relation<T>> Relation<T> for DistinctRelation<T, R> {
    fn foreach(&mut self, consumer: &mut dyn FnMut(&T, Count)) {
        self.inner.foreach(&mut |t, delta| {
            let old_count = self.input_counts.get(t).copied().unwrap_or(Count(0));
            let new_count = Count(old_count.0 + delta.0);
            self.input_counts.insert(t.clone(), new_count);

            let was_present = old_count.0 > 0;
            let is_present = new_count.0 > 0;

            if !was_present && is_present {
                consumer(t, Count(1));  // appeared
            } else if was_present && !is_present {
                consumer(t, Count(-1)); // disappeared
            }
            // else: no change to output
        });
    }
}

struct JoinRelation<L, R, K, FL, FR, RL, RR> {
    left: RL,
    right: RR,
    key_left: FL,
    key_right: FR,
    /// Track left tuples by key
    left_index: HashMap<K, Vec<(L, Count)>>,
    /// Track right tuples by key
    right_index: HashMap<K, Vec<(R, Count)>>,
}

impl<...> Relation<(L, R)> for JoinRelation<...> {
    fn foreach(&mut self, consumer: &mut dyn FnMut(&(L, R), Count)) {
        // Process left changes - join with existing right
        self.left.foreach(&mut |l, l_delta| {
            let k = (self.key_left)(l);
            // Add to left index
            self.left_index.entry(k.clone()).or_default().push((l.clone(), l_delta));
            // Join with right
            if let Some(rights) = self.right_index.get(&k) {
                for (r, r_count) in rights {
                    consumer(&(l.clone(), r.clone()), Count(l_delta.0 * r_count.0));
                }
            }
        });

        // Process right changes - join with existing left (but not the new ones we just added)
        // ... careful bookkeeping needed here
    }
}
```

### Input Relations

```rust
struct InputRelation<T> {
    pending: Vec<(T, Count)>,
}

impl<T: Tuple> Relation<T> for InputRelation<T> {
    fn foreach(&mut self, consumer: &mut dyn FnMut(&T, Count)) {
        for (t, count) in self.pending.drain(..) {
            consumer(&t, count);
        }
    }
}

struct InputHandle<T> {
    inner: Rc<RefCell<InputRelation<T>>>,
}

impl<T: Tuple> InputHandle<T> {
    fn insert(&mut self, t: T) {
        self.inner.borrow_mut().pending.push((t, Count(1)));
    }

    fn delete(&mut self, t: T) {
        self.inner.borrow_mut().pending.push((t, Count(-1)));
    }
}
```

### SavedRelation

For using a relation in multiple places:

```rust
struct SavedRelation<T> {
    /// Pending changes to deliver to consumers
    pending: Vec<(T, Count)>,
    /// How many consumers exist
    num_consumers: usize,
    /// Which consumers have read the current batch
    consumed: HashSet<usize>,
}

impl<T: Tuple> SavedRelation<T> {
    fn get(&mut self) -> SavedGetter<T> {
        let id = self.num_consumers;
        self.num_consumers += 1;
        SavedGetter { saved: Rc::new(RefCell::new(self)), id }
    }
}

struct SavedGetter<T> {
    saved: Rc<RefCell<SavedRelation<T>>>,
    id: usize,
}

impl<T: Tuple> Relation<T> for SavedGetter<T> {
    fn foreach(&mut self, consumer: &mut dyn FnMut(&T, Count)) {
        let mut saved = self.saved.borrow_mut();
        if !saved.consumed.contains(&self.id) {
            for (t, count) in &saved.pending {
                consumer(t, *count);
            }
            saved.consumed.insert(self.id);
            // Clear when all consumers have read
            if saved.consumed.len() == saved.num_consumers {
                saved.pending.clear();
                saved.consumed.clear();
            }
        }
    }
}
```

### Type Erasure for Relation

To store relations of different types, use a wrapper:

```rust
struct Relation<T: Tuple> {
    inner: Box<dyn RelationImpl<T>>,
}

trait RelationImpl<T: Tuple> {
    fn foreach(&mut self, f: &mut dyn FnMut(&T, Count));
}

impl<T: Tuple> Relation<T> {
    fn foreach(&mut self, f: &mut dyn FnMut(&T, Count)) {
        self.inner.foreach(f);
    }

    fn map<U: Tuple, F: Fn(&T) -> U + 'static>(self, f: F) -> Relation<U> {
        Relation { inner: Box::new(MapRelation { inner: self, f }) }
    }

    fn filter<F: Fn(&T) -> bool + 'static>(self, pred: F) -> Relation<T> {
        Relation { inner: Box::new(FilterRelation { inner: self, pred }) }
    }
}
```

## Migration Steps

### Phase 1: Core Types
1. Define `Count` type (or reuse existing `Diff`)
2. Define `RelationImpl<T>` trait with `foreach`
3. Define `Relation<T>` wrapper struct
4. Define `InputRelation<T>` and `InputHandle<T>`

### Phase 2: Stateless Operators
5. `MapRelation`
6. `FilterRelation`
7. `FlatMapRelation`
8. `UnionRelation`

### Phase 3: Stateful Operators
9. `DistinctRelation`
10. `JoinRelation`
11. `DifferenceRelation` (distinct of left + negated right)

### Phase 4: SavedRelation
12. `SavedRelation` and `SavedGetter`

### Phase 5: Aggregation
13. `GroupMaxRelation`
14. `GroupSumRelation`

### Phase 6: Feedback & Fixpoint
15. `FeedbackRelation` for cycles
16. `Variable` for recursive definitions
17. Fixpoint computation

## Key Insights

1. **foreach is the only interface** - Everything flows through `foreach`
2. **No state exposure** - Operators may hold state internally, but don't expose it
3. **Pull semantics** - Calling foreach on output pulls from inputs recursively
4. **Stateless is simple** - Map/filter/union just transform and pass through
5. **Stateful tracks input** - Distinct/join need input history to compute correct deltas
