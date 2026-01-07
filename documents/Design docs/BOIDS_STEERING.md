# Boids Steering System - Design & Scalability

## Overview

Boids steering provides emergent flocking behavior using three classic behaviors:
- **Separation**: Avoid crowding neighbors (personal space)
- **Alignment**: Match velocity with nearby units (move together)
- **Cohesion**: Steer toward average position of neighbors (stay in group)

This creates natural-looking swarm movement without explicit formation logic.

## Performance Requirements

| Unit Count | Target Frame Time | Boids Budget | Status |
|------------|------------------|--------------|--------|
| 1,000 | 16ms (60 FPS) | <1ms | ✅ Achievable |
| 3,500 | 16ms (60 FPS) | <2ms | ⚠️ Current: 5ms |
| 10,000 | 33ms (30 FPS) | <3ms | ❌ Not tested |
| 100,000 | 33ms (30 FPS) | <5ms | ❌ Requires LOD |
| 1,000,000+ | 33ms (30 FPS) | <8ms | ❌ Requires major changes |

## Current Implementation (v1.0)

### Algorithm
```rust
for each unit:
    neighbors = spatial_hash.query_radius(pos, radius)  // O(1) via spatial partitioning
    limit neighbors to MAX (default: 8)
    
    for each neighbor (up to 8):
        separation += inverse_square_force(distance)
        alignment += neighbor.velocity
        cohesion += neighbor.position
    
    normalize and apply weighted forces
```

### Performance Analysis (3500 units)
- **Total time**: 5ms per frame
- **Inner loop iterations**: 3500 units × 8 neighbors = 28,000
- **Per-iteration cost**: ~178ns

#### Bottlenecks Identified:
1. **HashMap Construction**: Building `HashMap<Entity, (pos, vel)>` every frame
   - Cost: 3500 allocations + hashing + insertions = ~0.5-1ms
   - **Anti-pattern**: Allocating collections in hot loops
   
2. **FixedNum Math**: Fixed-point arithmetic is 2-5× slower than f32
   - normalize(): ~20-50ns each (3-4 per unit)
   - sqrt(): ~15ns (1-2 per neighbor for separation)
   
3. **Query Lookups**: 28,000 `query.get()` or HashMap lookups per frame
   - HashMap: ~10ns per lookup = 0.28ms total
   - Direct query.get(): ~20-30ns per lookup = 0.56-0.84ms total

## Optimization History

### ❌ What DIDN'T Work (Lessons Learned)

#### 1. O(N²) Neighbor Search (Initial Implementation)
```rust
// BROKEN: Catastrophic for >1000 units
for unit in units {
    for other in units {  // O(N²) = 3500² = 12.25 million checks!
        if distance(unit, other) < radius {
            // process neighbor
        }
    }
}
```
**Result**: 128ms for 2500 units (unplayable)  
**Fix**: Spatial hash for O(N×M) where M = avg neighbors

#### 2. Linear Neighbor Velocity Lookup
```rust
// BROKEN: O(N) search for each neighbor
for neighbor in nearby {
    let vel = units.iter().find(|u| u.entity == neighbor).unwrap().vel;
    // 2500 units × 23 neighbors × 2500 search = 144M operations!
}
```
**Result**: 128ms for 2500 units  
**Fix**: HashMap for O(1) lookups → 8ms (16× improvement)

#### 3. Redundant Distance Checks
```rust
// BROKEN: Spatial hash already filters by radius!
let nearby = spatial_hash.query_radius(pos, neighbor_radius);
for neighbor in nearby {
    if distance(pos, neighbor) < neighbor_radius {  // ← Redundant!
        // ...
    }
}
```
**Result**: Unnecessary sqrt() calls  
**Fix**: Trust spatial hash, work with dist² for separation

#### 4. Excessive Normalization
```rust
// BROKEN: 4 normalizations per unit
alignment.normalize();
cohesion.normalize();
separation.normalize();
velocity.normalize();  // if clamping speed
```
**Result**: 3500 × 4 = 14,000 normalizes @ 40ns each = 0.56ms wasted  
**Fix**: Accumulate unnormalized, normalize final force only

#### 5. Processing ALL Neighbors
```rust
// BROKEN: Processing 94 neighbors in dense areas
for neighbor in spatial_hash.query_radius(pos, radius) {
    // With 94 neighbors: 3500 × 94 = 329,000 iterations!
}
```
**Result**: 15-20ms for 3500 units in dense areas  
**Fix**: Limit to 8 closest neighbors → 5ms (3-4× improvement)

### ✅ What DID Work (Applied Optimizations)

1. **Spatial Hash**: O(N²) → O(N×M) where M ≈ 8-20
2. **Neighbor Limit**: Cap at 8 neighbors per unit (StarCraft II uses 10-15)
3. **Squared Distance Math**: Avoid sqrt in separation force calculation
4. **Batch Normalization**: 1 normalize per unit instead of 4
5. **Early Exits**: Skip units with zero weights or no neighbors
6. **Single HashMap**: Eliminate redundant Vec, use one HashMap

## Scalability Roadmap

### Level 1: 10K Units (<3ms target)

**Strategy: Temporal LOD**
```rust
// Update boids every N frames based on distance to camera
let update_interval = if distance_to_camera < 100.0 { 1 } else { 3 };
if tick % update_interval != 0 { return; }
```
**Impact**: 5ms → 1.67ms average (3× reduction)  
**Visual Quality**: Indistinguishable (human eye can't see 33ms → 100ms update difference in flocking)

### Level 2: 100K Units (<5ms target)

**Strategy: Spatial LOD + Staggered Updates**
```rust
// Only process units in visible area + buffer
if !camera_frustum.contains(unit.pos, buffer=50.0) { continue; }

// Distribute work across frames (process 1/3 each frame)
let batch = tick % 3;
for unit in units.iter().skip(batch).step_by(3) { ... }
```
**Impact**: Only process ~5-10K units per frame instead of 100K  
**Visual Quality**: Off-screen units don't need perfect flocking

### Level 3: 1M Units (<8ms target)

**Strategy: Hierarchical Boids + GPU Compute**
```rust
// Group units into flocks of 100, run boids on flock leaders only
struct Flock {
    leader_entity: Entity,
    members: Vec<Entity>,  // max 100
}

// Update 10K flock leaders, members inherit behavior
for flock in flocks {
    update_boids_for_leader(flock.leader);
    for member in flock.members {
        member.velocity = lerp(member.velocity, flock.leader.velocity, 0.1);
    }
}
```
**Impact**: 1M units → 10K boids calculations  
**Alternative**: Move to GPU compute shader (process 1M units in parallel)

### Level 4: 10M Units (Future)

**Requirements**:
- GPU compute shaders mandatory
- Chunked processing (stream units in/out of GPU memory)
- Aggressive LOD (only process visible + nearby chunks)
- Consider dropping boids for distant units entirely

## Neighbor Caching Strategy

### Existing System: Collision Neighbor Cache

The game already has a neighbor caching system for collision detection. Understanding it helps design the boids cache.

#### How Cache Hit/Miss Works

**IMPORTANT**: Cache hit/miss is **NOT** about reading collision results. It's about whether we need to **rebuild the neighbor list** from the spatial hash.

**Cache HIT** (72.5% of units):
```rust
// Unit hasn't moved much since last spatial query
if moved_distance < 0.5 && frames_since_update < 10 {
    // HIT: Reuse old neighbor list (free!)
    // cache.neighbors still has entities from 3-5 frames ago
    // No spatial hash query happens!
}
```

**Cache MISS** (27.5% of units):
```rust
// Unit has moved significantly or cache is stale
if moved_distance > 0.5 || frames_since_update >= 10 {
    // MISS: Query spatial hash and rebuild list (expensive!)
    cache.neighbors = spatial_hash.query(pos, radius);
    cache.last_query_pos = current_pos;
    cache.frames_since_update = 0;
}
```

**Why this matters**: Without caching, 3500 spatial queries × 50µs = **175ms per frame**. With caching, only 962 queries × 50µs = **48ms** (3.6× faster!)

#### Collision Cache Component Specs

```rust
struct CachedNeighbors {
    neighbors: Vec<(Entity, FixedVec2)>,  // Nearby entities + positions
    last_query_pos: FixedVec2,            // Position when cache was built
    frames_since_update: u32,             // Age of cache
    is_fast_mover: bool,                  // Speed classification
}
```

**Search Radius**: `unit.radius × collision_search_radius_multiplier = 0.5 × 4.0 = 2.0 units`

**Invalidation Rules**:
- **Slow movers** (speed < 8.0): Invalidate if moved >0.5 units OR every 10 frames
- **Fast movers** (speed ≥ 8.0): Invalidate if moved >0.2 units OR every 2 frames

**Current Performance** (3500 units):
- Avg neighbors: 47.7 per unit (max: 107!)
- Cache hit rate: 72.5%
- Memory: ~2.6 MB (762 bytes per unit)

### Why NOT Reuse Collision Cache for Boids

| Feature | Collision Cache | Boids Needs | Problem |
|---------|----------------|-------------|---------|
| **Search Radius** | 2.0 units | 5.0 units | ❌ 60% of boids neighbors missed! |
| **Avg Neighbors** | 47.7 | 8 (capped) | ❌ 40 wasted neighbors per unit = 140K wasted iterations |
| **Max Neighbors** | 107 | 8 (capped) | ❌ 99 wasted checks in dense areas |
| **Stores Velocity** | ❌ No | ✅ Required | ❌ Need 28K extra query.get() calls |
| **Update Frequency** | 2-10 frames | 3-5 frames OK | ✅ Compatible |
| **Determinism** | Critical | Less critical | ✅ Boids is visual-only |

**Verdict**: Incompatible radius and missing velocity data make reuse impractical.

### Proposed: BoidsNeighborCache Component

```rust
#[derive(Component)]
struct BoidsNeighborCache {
    /// Closest 8 neighbors with position and velocity
    neighbors: SmallVec<[(Entity, FixedVec2, FixedVec2); 8]>,
    
    /// Position when cache was last updated
    last_query_pos: FixedVec2,
    
    /// Frames elapsed since last update
    frames_since_update: u32,
}
```

**Key Design Decisions**:

1. **SmallVec with 8-element inline storage**
   - No heap allocation for typical case (8 neighbors)
   - Falls back to heap only if >8 neighbors (rare)
   - Stack allocation = faster, no fragmentation

2. **Stores velocity in cache**
   - Eliminates 28,000 query.get() lookups per frame
   - Alignment behavior reads directly from cache
   - Tradeoff: Uses 8 bytes more per neighbor

3. **Search radius: 5.0 units**
   - Matches `neighbor_radius` config
   - Finds ~8-15 neighbors in typical density
   - Limit to closest 8 when building cache

4. **Update frequency: Every 3-5 frames**
   - Boids is visual-only, tolerates stale data
   - Slower than collision (2-10 frames) = less overhead
   - Invalidate if moved >1.0 units (more lenient than collision's 0.5)

**Expected Performance**:
- **Memory**: 8 neighbors × 24 bytes = 192 bytes per unit × 3500 = **672 KB** (4× smaller than collision cache)
- **Cache hit rate**: ~80-85% (slower invalidation = more hits)
- **Spatial queries saved**: 2800-3000 per frame (only 500-700 misses)
- **Boids time reduction**: 5ms → **1-2ms** (2.5-5× faster)

**Implementation Phases**:

**Phase 1**: Add component + update system
```rust
fn update_boids_neighbor_cache(
    query: Query<(Entity, &SimPosition, &SimVelocity, &mut BoidsNeighborCache)>,
    spatial_hash: Res<SpatialHash>,
    all_units: Query<(Entity, &SimPosition, &SimVelocity)>,
) {
    for (entity, pos, vel, mut cache) in query.iter_mut() {
        cache.frames_since_update += 1;
        let moved = (pos.0 - cache.last_query_pos).length();
        
        if moved > 1.0 || cache.frames_since_update >= 5 {
            // Rebuild cache from spatial hash
            let nearby = spatial_hash.query_radius(entity, pos.0, 5.0);
            
            // Take closest 8, fetch their velocities
            cache.neighbors.clear();
            for (neighbor_entity, neighbor_pos) in nearby.iter().take(8) {
                if let Ok((_, _, neighbor_vel)) = all_units.get(*neighbor_entity) {
                    cache.neighbors.push((*neighbor_entity, *neighbor_pos, neighbor_vel.0));
                }
            }
            
            cache.last_query_pos = pos.0;
            cache.frames_since_update = 0;
        }
    }
}
```

**Phase 2**: Update boids system to use cache
```rust
fn apply_boids_steering(
    query: Query<(Entity, &SimPosition, &SimVelocity, &BoidsNeighborCache)>,
) {
    for (entity, pos, vel, cache) in query.iter() {
        // No spatial hash query! Just read cache
        for &(neighbor_entity, neighbor_pos, neighbor_vel) in &cache.neighbors {
            // All data already available - no lookups needed!
            separation += calculate_separation(pos.0, neighbor_pos);
            alignment += neighbor_vel;
            cohesion += neighbor_pos;
        }
    }
}
```

## Memory Allocation Strategy

### ❌ ANTI-PATTERN: Per-Frame Allocations
```rust
// DO NOT DO THIS - Allocates 3500 times per frame!
fn boids_system(query: Query<...>) {
    let units: Vec<_> = query.iter().collect();  // ❌ Allocation!
    let map: HashMap<_, _> = units.iter().collect();  // ❌ Another allocation!
    // ...
}
```
**Why this breaks at scale**:
- 3500 units = 3500 allocations + HashMap allocation every 33ms
- Heap fragmentation over time → slower allocations
- Non-deterministic timing (allocator state varies)
- At 10K units: 10,000 allocations/frame = likely GC/allocator stalls

### ✅ SOLUTION: Cached Allocations
```rust
fn boids_system(
    query: Query<...>,
    mut cache: Local<HashMap<Entity, (FixedVec2, FixedVec2)>>,
) {
    cache.clear();  // Reuse allocation, just clear entries
    for (e, p, v) in query.iter() {
        cache.insert(e, (p.0, v.0));  // Reuse HashMap's capacity
    }
}
```
**Benefits**:
- One allocation at startup, reused forever
- HashMap capacity grows to fit max unit count, then stabilizes
- Deterministic performance (no allocator variance)

### ✅ BETTER SOLUTION: No HashMap At All
```rust
fn boids_system(query: Query<...>, spatial_hash: Res<SpatialHash>) {
    for (entity, pos, vel) in query.iter() {
        let neighbors = spatial_hash.query_radius(entity, pos, radius);
        for (neighbor_entity, neighbor_pos) in neighbors {
            // Spatial hash provides position for FREE
            // Only need to lookup velocity via query.get()
            let Ok((_, _, neighbor_vel)) = query.get(neighbor_entity) else { continue; };
            // Use neighbor_pos from spatial hash, neighbor_vel from query
        }
    }
}
```
**Benefits**:
- Zero allocations per frame
- Positions come from spatial hash (already computed)
- Only 28,000 `query.get()` calls instead of building entire HashMap

## Determinism Considerations

### Safe:
- Neighbor limits (always process first 8)
- Spatial hash queries (deterministic order via sorted cells)
- Fixed-point math (cross-platform identical)

### DANGEROUS:
- Processing ALL neighbors from spatial hash without sorting
  - Cell iteration order may vary by implementation
  - **Solution**: Always `.take(N)` or sort by distance first
  
- Using f32 for intermediate calculations
  - Different FPU rounding on ARM vs x86
  - **Solution**: Convert to f32 only for non-critical visuals, use FixedNum for forces

## Configuration Parameters

| Parameter | Default | Range | Impact |
|-----------|---------|-------|--------|
| `boids_max_neighbors` | 8 | 5-15 | Linear scaling with inner loop cost |
| `neighbor_radius` | 5.0 | 3-10 | Quadratic scaling (more neighbors found) |
| `separation_weight` | 1.5 | 0-3 | Visual only (spacing between units) |
| `alignment_weight` | 1.0 | 0-3 | Visual only (heading coherence) |
| `cohesion_weight` | 1.0 | 0-3 | Visual only (group tightness) |

**Tuning for Performance**:
- Reduce `boids_max_neighbors` to 5-6 for distant units
- Reduce `neighbor_radius` to 3.0 (fewer neighbors found)
- Set weights to 0.0 to skip behaviors (early exit)

## Future Optimizations (Not Implemented)

### 1. SIMD Vectorization
Process 4-8 units in parallel using SIMD instructions:
```rust
// Process 4 units at once with AVX
let pos_x = [unit0.x, unit1.x, unit2.x, unit3.x];
let pos_y = [unit0.y, unit1.y, unit2.y, unit3.y];
// SIMD distance calculations...
```
**Impact**: 2-4× speedup on compatible CPUs

### 2. Multi-threading
Split units into chunks, process on separate threads:
```rust
units.par_chunks(1000).for_each(|chunk| {
    for unit in chunk { update_boids(unit); }
});
```
**Impact**: Near-linear scaling with CPU cores (8 cores = 8× faster)

### 3. Adaptive Update Rates
```rust
// Units moving fast update more often
let interval = if velocity.length() > 5.0 { 1 } else { 5 };
```
**Impact**: Idle units consume nearly zero CPU

### 4. GPU Compute Shaders (Required for 1M+)
Move entire boids calculation to GPU:
- Upload unit positions/velocities to GPU buffer
- Compute shader runs boids in parallel (1M threads)
- Download results back to CPU
**Impact**: Can handle millions of units

## Testing Strategy

### Performance Benchmarks
```rust
#[test]
fn bench_boids_1k_units() {
    // Ensure <0.5ms for 1000 units
}

#[test]
fn bench_boids_10k_units() {
    // Ensure <3ms for 10K units
}
```

### Determinism Tests
```rust
#[test]
fn boids_deterministic() {
    // Run simulation twice with same seed
    // Assert final velocities are bit-identical
}
```

### Visual Quality Tests
- Test with update_interval=3: Should still look like coherent flocking
- Test with max_neighbors=5 vs 15: Minimal visual difference

## References

- Craig Reynolds (1987): "Flocks, Herds, and Schools: A Distributed Behavioral Model"
- StarCraft II GDC Talk (2011): Uses 10-15 neighbors max, LOD for distant units
- Boids GPU Implementation (2023): Unity DOTS example with 100K units

## Current Status (2026-01-07)

- **Implementation**: v1.0 with HashMap optimization
- **Performance**: 5ms @ 3500 units (needs improvement)
- **Next Steps**: 
  1. Eliminate HashMap allocation (use query.get() + spatial hash positions)
  2. Add temporal LOD (update every N frames)
  3. Test with 10K units
  4. Implement spatial LOD for camera frustum culling
