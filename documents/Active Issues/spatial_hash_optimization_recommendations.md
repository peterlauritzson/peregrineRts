# Spatial Hash Optimization Recommendations

**Date**: 2026-01-19  
**Priority**: Low to Medium (implement when scaling beyond 1M entities)

## Overview

The spatial hash implementation is excellent and production-ready. These optimizations are **optional enhancements** for when you scale to 5M+ entities or want to achieve perfect zero-allocation compliance.

## Issue #1: `cells_in_radius()` Allocation

### Current Code
[grid.rs#L483](../src/game/spatial_hash/grid.rs#L483):
```rust
pub fn cells_in_radius(&self, pos: FixedVec2, radius: FixedNum) -> Vec<(usize, usize)> {
    let mut result = Vec::new();  // ❌ Allocates on every call
    
    // Calculate bounds...
    for row in min_row..=max_row {
        for col in min_col..=max_col {
            result.push((col, row));
        }
    }
    result
}
```

### Impact
- Allocates 64-144 bytes per query (typically 4-9 cells)
- Called from `get_potential_collisions()` (hot path)
- At 10k queries/frame: ~1MB allocations/frame
- Heap fragmentation over time

### Solution A: Scratch Buffer (Recommended)

**Step 1**: Add to `SpatialHashScratch`:
```rust
// In src/game/spatial_hash/mod.rs
#[derive(Resource)]
pub struct SpatialHashScratch {
    pub query_results: Vec<Entity>,
    pub query_results_secondary: Vec<Entity>,
    pub cell_coords: Vec<(usize, usize)>,  // NEW
}

impl SpatialHashScratch {
    pub fn new(query_capacity: usize) -> Self {
        Self {
            query_results: Vec::with_capacity(query_capacity),
            query_results_secondary: Vec::with_capacity(query_capacity),
            cell_coords: Vec::with_capacity(16),  // Max 4x4 grid typically
        }
    }
}
```

**Step 2**: Update `cells_in_radius()`:
```rust
// In src/game/spatial_hash/grid.rs
pub fn cells_in_radius(
    &self, 
    pos: FixedVec2, 
    radius: FixedNum, 
    out_cells: &mut Vec<(usize, usize)>
) {
    out_cells.clear();  // O(1), keeps capacity
    
    // Calculate bounds (same as before)...
    
    // CRITICAL: Check capacity before pushing
    for row in min_row..=max_row {
        for col in min_col..=max_col {
            if out_cells.len() < out_cells.capacity() {
                out_cells.push((col, row));
            } else {
                #[cfg(debug_assertions)]
                panic!("Cell coords buffer overflow!");
                #[cfg(not(debug_assertions))]
                warn_once!("Cell coords overflow - query truncated");
                return;
            }
        }
    }
}
```

**Step 3**: Update callers in `query.rs`:
```rust
// Add scratch parameter to get_potential_collisions
pub fn get_potential_collisions(
    &self, 
    pos: FixedVec2, 
    query_radius: FixedNum, 
    exclude_entity: Option<Entity>, 
    out_entities: &mut Vec<Entity>,
    scratch: &mut SpatialHashScratch,  // NEW
) {
    // ...
    
    // Use scratch buffer for cell coords
    size_class.grid_a.cells_in_radius(pos, query_radius, &mut scratch.cell_coords);
    for &(col, row) in &scratch.cell_coords {
        // ... process cells ...
    }
    
    size_class.grid_b.cells_in_radius(pos, query_radius, &mut scratch.cell_coords);
    for &(col, row) in &scratch.cell_coords {
        // ... process cells ...
    }
}
```

### Solution B: Iterator (Zero-Cost Abstraction)

More elegant but requires more refactoring:

```rust
pub fn cells_in_radius_iter(
    &self, 
    pos: FixedVec2, 
    radius: FixedNum
) -> impl Iterator<Item = (usize, usize)> + '_ {
    // Calculate bounds
    let min_col = /* ... */;
    let max_col = /* ... */;
    let min_row = /* ... */;
    let max_row = /* ... */;
    
    // Return lazy iterator (no allocation!)
    (min_row..=max_row)
        .flat_map(move |row| (min_col..=max_col).map(move |col| (col, row)))
}
```

**Pros**: Zero allocation, idiomatic Rust  
**Cons**: Caller must consume immediately (can't store iterator)

### Recommendation

- **For now**: Use Solution A (scratch buffer) - simple and effective
- **Future**: Consider Solution B when refactoring query system

---

## Issue #2: HashSet Allocation in Queries

### Current Code
[query.rs#L16](../src/game/spatial_hash/query.rs#L16):
```rust
pub fn get_potential_collisions(...) {
    let mut seen = HashSet::new();  // ❌ Allocates on every call
    // ... use seen to deduplicate across Grid A and Grid B ...
}
```

### Impact
- Allocates ~24 bytes + bucket array (size depends on query result count)
- Called on every proximity query
- Lower impact than `cells_in_radius` but still allocates

### Solution: Preallocated HashSet in Scratch

**Step 1**: Add to `SpatialHashScratch`:
```rust
use bevy::utils::HashSet;  // Bevy's faster hash set

#[derive(Resource)]
pub struct SpatialHashScratch {
    pub query_results: Vec<Entity>,
    pub query_results_secondary: Vec<Entity>,
    pub cell_coords: Vec<(usize, usize)>,
    pub seen_entities: HashSet<Entity>,  // NEW
}

impl SpatialHashScratch {
    pub fn new(query_capacity: usize) -> Self {
        Self {
            query_results: Vec::with_capacity(query_capacity),
            query_results_secondary: Vec::with_capacity(query_capacity),
            cell_coords: Vec::with_capacity(16),
            seen_entities: HashSet::with_capacity(query_capacity),  // NEW
        }
    }
}
```

**Step 2**: Update `get_potential_collisions()`:
```rust
pub fn get_potential_collisions(
    &self, 
    pos: FixedVec2, 
    query_radius: FixedNum, 
    exclude_entity: Option<Entity>, 
    out_entities: &mut Vec<Entity>,
    scratch: &mut SpatialHashScratch,
) {
    scratch.seen_entities.clear();  // O(1), keeps capacity
    out_entities.clear();
    
    let capacity = out_entities.capacity();
    
    for size_class in &self.size_classes {
        // ... query Grid A ...
        for &entity in entities {
            if entity != Entity::PLACEHOLDER 
                && Some(entity) != exclude_entity 
                && scratch.seen_entities.insert(entity)  // ✅ Use preallocated set
            {
                if out_entities.len() < capacity {
                    out_entities.push(entity);
                } else {
                    // ... overflow handling ...
                }
            }
        }
        
        // ... query Grid B ...
    }
}
```

### Recommendation

Implement this when optimizing for >5M entities or >10k queries/frame.

---

## Issue #3: System Integration Updates

### Update collision system to pass scratch buffer

**Current** [collision.rs#L75](../src/game/simulation/collision.rs#L75):
```rust
spatial_hash.get_potential_collisions(
    pos.0,
    collider.radius * FixedNum::from_num(2.0),
    Some(entity),
    &mut scratch.query_results,
);
```

**After implementing Issue #1 and #2**:
```rust
spatial_hash.get_potential_collisions(
    pos.0,
    collider.radius * FixedNum::from_num(2.0),
    Some(entity),
    &mut scratch.query_results,
    &mut scratch,  // Pass full scratch for cell_coords and seen_entities
);
```

Do the same for all other query call sites.

---

## Testing Plan

### Unit Tests

```rust
#[test]
fn test_cells_in_radius_no_allocation() {
    let grid = StaggeredGrid::new(/* ... */);
    let mut scratch = vec![(0, 0); 16];  // Preallocate
    
    // Call multiple times - should reuse buffer
    for _ in 0..1000 {
        grid.cells_in_radius(pos, radius, &mut scratch);
        assert!(scratch.len() <= 16);  // No reallocation
    }
}

#[test]
fn test_query_no_allocation() {
    let spatial_hash = SpatialHash::new(/* ... */);
    let mut scratch = SpatialHashScratch::new(1000);
    
    // Multiple queries should not allocate
    for _ in 0..1000 {
        spatial_hash.get_potential_collisions(
            pos, radius, None, 
            &mut scratch.query_results, 
            &mut scratch
        );
        
        // Verify capacities unchanged (no reallocation)
        assert_eq!(scratch.query_results.capacity(), 1000);
        assert_eq!(scratch.cell_coords.capacity(), 16);
    }
}
```

### Allocation Profiling

Use a memory profiler to verify zero allocation:

```rust
// In a benchmark or profiling build
#[cfg(feature = "profiling")]
fn profile_spatial_hash_queries() {
    use std::alloc::{System, GlobalAlloc, Layout};
    
    // Track allocations
    static mut ALLOC_COUNT: usize = 0;
    
    // Run 10,000 queries
    for i in 0..10_000 {
        spatial_hash.query_radius(/* ... */);
    }
    
    // Should be ZERO allocations after first query
    assert_eq!(ALLOC_COUNT, 0, "Found {} allocations in hot path!", ALLOC_COUNT);
}
```

---

## Implementation Priority

### Priority 1 (When scaling to >1M entities)
- [ ] Issue #1: Fix `cells_in_radius()` allocation (Solution A)
- [ ] Update collision system to pass scratch buffer

### Priority 2 (When scaling to >5M entities)
- [ ] Issue #2: Fix HashSet allocation in queries
- [ ] Add allocation profiling tests
- [ ] Profile memory usage at scale

### Priority 3 (Future optimizations)
- [ ] Consider iterator-based `cells_in_radius()` (Solution B)
- [ ] Parallel query processing
- [ ] SIMD optimizations for distance checks

---

## Estimated Impact

### Before Optimizations
- Allocations per query: 2 (Vec + HashSet)
- Bytes per query: ~200-400 bytes
- At 10k queries/frame: ~2-4 MB allocations/frame
- At 60 FPS: ~120-240 MB/sec allocation rate

### After Optimizations
- Allocations per query: **0** ✅
- Bytes per query: **0** ✅
- At 10k queries/frame: **0** ✅
- At 60 FPS: **0 MB/sec** ✅

**Reduction**: 100% allocation elimination in query hot path

---

## Conclusion

These optimizations are **non-critical** for current scale but will become important when:
- Entity count exceeds 1M
- Query count exceeds 10k/frame
- Targeting 10M+ entity scale
- Experiencing heap fragmentation issues

The current implementation is already excellent. Implement these optimizations incrementally as you scale up.

**For full verification details**, see [spatial_hash_verification.md](./spatial_hash_verification.md).
