# Consolidate Placement Rules

## Overview

When deciding where to place `consolidate` operations, analyze the dataflow graph and apply these rules.

## Rule 1: Skip Same-Count Consolidates

If a consolidate has the **same count as its parent**, don't include it.

Example from graph:
```
n8: [global_max] count=28713 <- n5(14357)
n9: current_level [consolidate] count=28713 <- n8(28713)  # SKIP - same count
```

## Rule 2: Different Count - Check Downstream

If a consolidate has a **different count from its parent**, check what happens downstream.

### 2a: SKIP if it feeds into "terminal" operations

Don't include the consolidate if, after any chain (including length 0) of **map or concat operations** (`concat`, `flat_map`, `filter_map`, `filter`, `map`, `fst`, `snd`, `swap`), it feeds into:
- `output`
- `feedback`
- `interrupt`
- `partition` / `partition_left` / `partition_right`
- `split` / `split_left` / `split_right`
- another `consolidate` (one that you've decided to include per these rules, regardless of what's currently in the code)
- `.saved()`
- final non-map operation in an `assign_saved!`

Example - SKIP (feeds to partition after filter_map):
```
n74: [consolidate] count=205658 <- n73(254628)  # different count
n75: [filter_map] count=205658 <- n74
n76: [consolidate] count=125302 <- n75          # another consolidate follows
```

### 2b: INCLUDE if it feeds into "join-y" or "aggregate-y" operations

Include the consolidate if it feeds into operations like:
- `join`, `join_values`
- `semijoin`, `antijoin`
- `intersection`, `set_minus`
- `cartesian_product`
- `group_min`, `group_max`, `group_sum`
- `global_min`, `global_max`
- `distinct`

Example - INCLUDE (feeds to group_min):
```
n18: [map] count=859528 <- n17
n19: [*consolidate] count=566026 <- n18(859528)  # INCLUDE - different count, feeds to group_min
n20: [group_min] count=566026 <- n19
```

## Decision Flowchart

```
Is consolidate count == parent count?
├─ YES → SKIP
└─ NO → Follow downstream through map/concat ops (concat, map, flat_map, filter_map, filter, fst, snd, swap)
         What's the first non-map operation?
         ├─ output, feedback, interrupt, partition, split, included consolidate, .saved(), or assign_saved! → SKIP
         └─ join, semijoin, antijoin, intersection, cartesian_product,
            group_*, global_*, distinct, etc. → INCLUDE
```
