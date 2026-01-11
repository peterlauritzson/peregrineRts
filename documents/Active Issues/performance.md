# Active Performance Issues
**Last Updated:** January 11, 2026  
**Project:** Peregrine RTS - Performance optimization for 10M+ unit goal

---

## 🎯 Latest Performance Test Results (Jan 11, 2026)

### Automated Scaling Test Suite Results

**Test Configuration:** Release build, REAL per-system timing (fixed profiling bug)

#### ✅ 100 Units Baseline
- **Actual TPS:** 24,523 (massively exceeds target of 10 TPS)
- **Tick Time:** ~0.04ms per tick
- **Status:** ✅ Excellent performance
- **System Breakdown (per tick avg):**
  - Spatial Hash: 0.02ms (58%)
  - Collision Detect: 0.01ms (30%)
  - Collision Resolve: 0.00ms (6%)
  - Physics: 0.00ms (7%)
- **Primary Bottleneck:** Spatial Hash (58%)

#### ✅ 10K Units Moderate Scale
- **Actual TPS:** 936.7 (exceeds target of 10 TPS by 93.7x)
- **Tick Time:** ~1.35ms per tick
- **Status:** ✅ Strong performance, ready for next scale
- **System Breakdown (per tick avg):**
  - Spatial Hash: 0.95ms (71%)
  - Collision Detect: 0.34ms (25%)
  - Collision Resolve: 0.00ms (0.2%)
  - Physics: 0.06ms (4%)
- **Primary Bottleneck:** Spatial Hash (71%) - **This is the real bottleneck!**

#### 🔜 Next Test Targets (Currently Skipped)
- 100K units stress test
- 1M units extreme test
- 10M units ultimate goal

### 📊 Key Performance Insight: Spatial Hash is the Real Bottleneck!

**CORRECTION (Jan 11, 2026):** Previous analysis used ESTIMATED timing ratios that were completely wrong!

**Real Measurements with Fixed Profiling:**
- Spatial Hash: **50-71% of tick time** (was incorrectly estimated at 39%)
- Collision Detection: **25-42% of tick time** (was incorrectly estimated at 40%)
- Collision Resolution: **0.1-6% of tick time** (was incorrectly estimated at 8%)
- Physics: **4-7% of tick time** (was incorrectly estimated at 13%)

#### The Three-Stage Pipeline:

1. **Spatial Hash Update** (50-71% / DOMINANT)
   - Inserts/updates entity positions in grid cells
   - O(n) complexity - one update per entity
   - **PRIMARY BOTTLENECK** - especially with small cell sizes
   - Multi-cell storage overhead kills performance (see cell size analysis below)

2. **Neighbor Cache** (integrated with collision detection)
   - Queries spatial hash for nearby entities
   - **90-100% cache hit rate** with velocity-aware caching
   - Only queries when entities move significantly or timeout occurs

3. **Collision Detection** (25-42%)
   - Reads cached neighbor lists (NOT querying spatial hash!)
   - Performs actual distance calculations on cached pairs
   - Applies collision masks and filters
   - Generates collision events
   - O(n × avg_neighbors) complexity

#### Why Spatial Hash Dominates:

The spatial hash overhead comes from:
1. **Multi-cell storage** - entities stored in 4-9 cells with small cell size
2. **Grid clearing** - thousands of cell.clear() operations per tick
3. **Insert/remove overhead** - vector operations for each entity
4. **Cache misses** - large grid doesn't fit in L1 cache

**The neighbor cache prevents queries from dominating, but spatial hash UPDATES are the real cost!**

### 🎯 Next Optimization Target

**PRIMARY:** Spatial Hash (50-71% of tick time) ⚠️ CRITICAL
- Fix cell size (currently 5.0, should be 20-50) for **24-32% speedup**
- Reduce multi-cell storage overhead
- Optimize grid clearing operations
- Consider lockless data structures
- **This is the single biggest bottleneck!**

**SECONDARY:** Collision Detection (25-42% of tick time)
- Current complexity: O(n × neighbors) is acceptable
- Already using cached neighbors efficiently
- Further optimization less critical than spatial hash fix

### 📈 Performance Projection

Based on current scaling WITH REAL MEASUREMENTS:
- **10K units:** 936 TPS ✅ (Current - better than previous estimates!)
- **100K units:** ~140 TPS (estimated - should pass with cell size fix)
- **500K units:** ~30 TPS (current bottleneck - FAILS at 100 TPS target)
- **1M units:** ~15 TPS (estimated - will need major optimization)

**Immediate Action:** Fix spatial hash cell size from 5.0 to 20-50 for **immediate 24-32% speedup**. This alone could get us to 500K units @ 100 TPS!

---

## 🔥 Pathfinding Bottleneck Analysis (Jan 11, 2026)

### Performance Crisis at 100K Units

**Test Configuration:** 100k units @ 50 TPS with chunky pathfinding (10-20% of units to same point every 10 ticks)

#### Test Results

| Scale | Target TPS | Actual TPS | Status | Pathfinding Time | Total Requests |
|-------|------------|------------|--------|------------------|----------------|
| 10k units | 50 | 163.6 | ✅ PASS | 14.4ms avg | 199,384 |
| 100k units | 50 | 3.1 | ❌ FAIL | **1003ms avg** (4235ms spike!) | 2,089,498 |

**Scaling Problem:** 71x increase in pathfinding time for only 10x increase in units = **super-linear scaling**

#### Pathfinding System Breakdown

Detailed profiling of individual path requests revealed:

**Time Distribution (per path request @ 10k scale):**
- **Portal Graph A*: ~9.4ms (100%)** ← PRIMARY BOTTLENECK
- Goal Validation: <0.01ms (negligible)
- Line of Sight: <0.01ms (negligible)
- Connectivity Check: <0.01ms (negligible)
- Local A* (intra-cluster): <0.01ms (negligible)
- Flow Field Lookup: <0.01ms (negligible)

**Root Cause: Portal Graph A* Dominates**

The hierarchical pathfinding spends virtually ALL its time in the portal graph search phase - the high-level pathfinding between clusters.

#### Why Portal Graph A* is Slow

1. **Graph Size Scales with Map Size**
   - Larger maps = more clusters = more portals
   - 100k units requires ~632×632 map = massive portal graph
   - Each path search explores many portals

2. **No Path Caching**
   - 10-20% of 100k units = 10k-20k requests to SAME GOAL every 10 ticks
   - Every unit recalculates the same high-level path independently
   - Massive redundant work in chunky scenarios

3. **Potential Graph Structure Issues**
   - May be exploring too many portals per search
   - Heuristic might not be tight enough
   - Closed set/visited checking overhead

#### Request Volume Analysis

**100k units test generated 2,089,498 total path requests:**
- Test ran 100 ticks
- Chunky requests every 10 ticks = 10 request waves
- ~10-20k requests per wave (10-20% of 100k)
- Expected: ~100k-200k total requests
- **Actual: 2M+ requests** (10-20x higher than expected!)

This suggests units are re-requesting paths frequently, possibly due to:
- Path invalidation when goals change
- Units reaching waypoints and requesting next segment
- Path timeout/expiration causing re-requests

### Optimization Strategies

#### HIGH PRIORITY - Path Caching for Same Goals

When many units request paths to the same destination:

1. **Cache Portal Routes**
   - Hash (start_cluster, goal_cluster) → portal_path
   - Reuse high-level portal graph results
   - Only compute local A* per unit (cheap)

2. **Batch Path Requests**
   - Group requests by goal cluster
   - Compute portal path once per goal cluster
   - Apply to all units in batch

**Expected Impact:** Could reduce 10k-20k redundant searches to ~100 unique cluster-pair searches

#### MEDIUM PRIORITY - Reduce Request Volume

1. **Path Following Improvements**
   - Increase path waypoint lookahead
   - Reduce path invalidation triggers
   - Add path replanning cooldown

2. **Goal Stability**
   - Deduplicate requests for same entity
   - Filter out redundant re-requests

#### LOW PRIORITY - Portal Graph Optimizations

1. **Better Heuristic**
   - Tighter underestimate for A* guidance
   - Reduce portal exploration

2. **Graph Pruning**
   - Remove redundant portals
   - Simplify portal connections

3. **Parallel Pathfinding**
   - Process multiple requests concurrently
   - Use thread pool for pathfinding

### Performance Projections

**Current State (100k @ 50 TPS):**
- 1003ms pathfinding per tick = 99.1% of total time
- Actual: 3.1 TPS (failed - needs 50 TPS)

**With Path Caching (estimated):**
- Reduce 20k redundant searches to ~100 unique → 200x reduction
- 1003ms / 200 = ~5ms per tick
- New total tick time: ~9ms
- Projected: **111 TPS** ✅ (passes 50 TPS target!)

**Required for 1M units @ 100 TPS:**
- Even with caching, need additional optimizations:
  - Async pathfinding (separate thread pool)
  - Portal graph simplification
  - Request throttling/prioritization

### Action Items

- [ ] **CRITICAL:** Implement portal route caching for same-goal batches
- [ ] Add path request deduplication
- [ ] Investigate why 2M+ requests for 100k units (10-20x higher than expected)
- [ ] Add pathfinding performance metrics to game logs
- [ ] Profile portal graph search internals (iteration count, closed set size)
- [ ] Consider async pathfinding thread pool for large batches

**Related Code:**
- [src/game/pathfinding/astar.rs](../../src/game/pathfinding/astar.rs) - `find_path_hierarchical` portal graph search
- [src/game/pathfinding/systems.rs](../../src/game/pathfinding/systems.rs) - `process_path_requests` batch processing

---

## 🔬 Spatial Hash Cell Size Impact Analysis (Jan 11, 2026)

### Test Configuration
Ran performance_scaling test suite with three different cell sizes to analyze impact on both spatial hash and collision detection performance.

**Test Parameters:**
- Benchmark: 100k units @ 100 TPS
- Map size: ~632 units (sqrt(100,000 × 4))
- Collision radius: 0.5 units (default)
- Search radius: 2.0 units (0.5 × 4.0 multiplier)

### Results Summary

| Cell Size | Spatial Hash | Collision Detect | Total Time | Performance | Grid Size |
|-----------|-------------|------------------|------------|-------------|-----------|
| **5.0** (default) | 3.37ms | 3.45ms | 8.63ms | 153.6 TPS | 126×126 = 15,876 cells |
| **20.0** (normal) | 2.74ms | 2.81ms | 7.02ms | 190.9 TPS ⚡ **+24%** | 31×31 = 961 cells |
| **200.0** (huge) | 2.49ms | 2.56ms | 6.39ms | 203.2 TPS ⚡ **+32%** | 3×3 = 9 cells |

### Key Finding: Multi-Cell Storage Overhead

**The Surprise:** Both spatial hash AND collision detection improved with larger cells, when we expected a tradeoff.

**Root Cause:** Entities use **multi-cell storage** - each entity is inserted into EVERY cell its radius overlaps:

```rust
pub fn calculate_occupied_cells(&self, pos: FixedVec2, radius: FixedNum) -> Vec<(usize, usize)> {
    // Entity is inserted into ALL cells from min_col..=max_col, min_row..=max_row
    let min_col = ((pos.x - radius) / cell_size).floor()...;
    let max_col = ((pos.x + radius) / cell_size).floor()...;
    
    for row in min_row..=max_row {
        for col in min_col..=max_col {
            cells.push((col, row));  // EVERY cell in the bounding box!
        }
    }
}
```

### Why Both Systems Get Faster:

#### 1. Spatial Hash Performance
**Cell size 5.0:**
- Each unit (radius ~2) spans **4-9 cells** → must insert/remove from 4-9 vectors
- 15,876 total cells in grid → `clear()` called 15,876 times per tick

**Cell size 200.0:**
- Each unit spans **1 cell** → insert/remove from 1 vector only
- 9 total cells in grid → `clear()` called 9 times per tick

**Result:** **9× less work** inserting/removing entities, **1,764× fewer** clear operations!

#### 2. Collision Detection Performance
**Cell size 5.0:**
- Entities are **duplicated across multiple cells**
- When querying for neighbors, spatial hash returns **the same entity multiple times**
- Collision detection wastes cycles **filtering duplicates**:
  ```rust
  for &(other_entity, _) in &cache.neighbors {
      if entity > other_entity { 
          total_duplicate_skips += 1;  // ← Heavy overhead with small cells!
          continue; 
      }
  ```

**Cell size 200.0:**
- Each entity appears in **1 cell only**
- Neighbor queries return **each entity once**
- Minimal duplicate filtering needed

#### 3. Why Cell Size 200 Beats 20 (Additional 10% gain)

Beyond eliminating multi-cell occupancy, larger cells reduce **grid management overhead**:

**Cache Effects:**
- 961 cells (size 20): Grid metadata doesn't fit in L1 cache
- 9 cells (size 200): **Entire grid fits in L1 cache** = faster access

**Memory Operations:**
- Fewer cells = fewer Vec allocations
- Less memory fragmentation  
- Better memory locality when iterating

**Loop Overhead:**
- Fewer bounds calculations in nested loops
- Simpler index arithmetic

### Optimal Cell Size Recommendation

**Rule of Thumb:** Cell size should be **≥ 2× your typical entity radius** to avoid multi-cell storage.

For this codebase:
- Default collision radius: 0.5 units
- Search radius: 2.0 units (0.5 × 4.0 multiplier)
- **Recommended cell size: 20-50 units**

**Tradeoffs:**
- **Too small (5.0):** Multi-cell duplication overhead kills performance
- **Optimal (20-50):** Balance between spatial partitioning benefits and overhead
- **Too large (200+):** Degenerates into brute-force search within huge cells

**Current Default (5.0) is TOO SMALL** - causes 24-32% performance penalty from unnecessary duplication!

### Action Items
- [ ] Change default spatial hash cell size from 5.0 to 20.0 in production code
- [ ] Add cell size as configurable parameter in game_config.ron
- [ ] Consider dynamic cell sizing based on entity density
- [ ] Document multi-cell storage implications in spatial hash design docs

---

## Performance Goals vs Current State

### Target Performance
- **Entities:** 10,000,000 (10M)
- **Tick Rate:** 100 ticks/second (10ms per tick)
- **Frame Rate:** 1000 fps (1ms per frame)

### Current Performance (10,200 entities)
- **Tick Time:** 18-28ms (average ~20ms)
- **Actual Tick Rate:** ~50 ticks/second
- **Gap to Target:** Need **20x faster ticks** with **980x more entities**
- **Total Speedup Required:** ~**19,600x improvement** 🎯

---

## Critical Bottlenecks (Ordered by Impact)

### 1. BOIDS_CACHE - CRITICAL (65-85% of tick time)
**Status:** Active - Needs Investigation  
**Time per tick:** 11-17ms (most commonly 13-14ms)  
**Impact:** This is the single largest bottleneck

**Metrics:**
- Entities: 10,200
- Cache hit rate: 79-82% (20-21% misses)
- Average neighbors per entity: 8.0
- Runs: **Every single simulation tick**

**Issues:**
- Even with 80% cache hit rate, still taking 13ms
- The cache lookup/computation is extremely expensive
- Running on every tick without any throttling
- Processing all 10,200 entities every time

**Optimization Opportunities:**
- [ ] Reduce frequency - don't run every tick (maybe every 2-3 ticks)
- [ ] Profile what's happening inside these 13ms
- [ ] Improve spatial query efficiency
- [ ] Consider GPU acceleration for boids calculations
- [ ] Implement spatial coherence optimizations
- [ ] Use dirty flagging to skip static entities

**Related Code:**
- [src/game/unit/boids.rs](../../src/game/unit/boids.rs)
- [src/game/simulation/collision.rs](../../src/game/simulation/collision.rs)

---

### 2. Collision Detection - HIGH (7ms every 5 ticks)
**Status:** Active - Needs Optimization  
**Time per run:** 4.7-7.1ms  
**Frequency:** Runs every ~5 simulation ticks  

**Metrics:**
- Entities: 10,200
- Neighbor checks: ~390,000 per run (avg 38.1 per entity)
- Duplicate skips: ~185,000 (47% efficiency)
- Actual collisions found: ~14,800
- Hit ratio: **3.7-3.85%** (very low!)
- Search radius multiplier: 2.5x

**Issues:**
- Very low hit ratio (96% of checks are wasted)
- Search radius is too conservative (2.5x)
- Still doing 390k checks even with duplicate skipping
- Max neighbors reaching 149-169 in crowded areas

**Optimization Opportunities:**
- [ ] Tighten search radius (currently 2.5x is wasteful)
- [ ] Improve broad-phase filtering
- [ ] Better duplicate elimination (currently only 47%)
- [ ] Implement hierarchical collision detection
- [ ] Consider separating broad-phase and narrow-phase
- [ ] Add layer-based early rejection (currently 0 filtered)

**Related Documentation:**
- See [SPATIAL_PARTITIONING.md](../Design%20docs/SPATIAL_PARTITIONING.md) Section 9.1 for detailed analysis

---

### 3. Spatial Hash Update - MEDIUM (1.7-3ms per tick)
**Status:** Active - Optimization Needed  
**Time per tick:** 1.7-3.0ms  

**Metrics:**
- Total entities: 10,200
- New entities: 0
- Updated positions: 821-834 entities (~8%)
- Unchanged: ~9,376 entities (~92%)
- Multi-cell entities: 621-625 (~6%)

**Issues:**
- Processing all 10,200 entities even though only 8% move
- Multi-cell tracking overhead
- Grid operations not fully optimized

**Optimization Opportunities:**
- [ ] Early exit for unchanged entities (already mostly doing this)
- [ ] Batch updates more efficiently
- [ ] Optimize multi-cell entity handling
- [ ] Consider lockless data structures
- [ ] Profile grid insertion/removal operations

**Related Code:**
- [src/game/spatial_hash/grid.rs](../../src/game/spatial_hash/grid.rs)

---

### 4. Boids Steering - MEDIUM (~2.3ms)
**Status:** Active - Could be Optimized  
**Time per tick:** 2.263ms  

**Metrics:**
- Units: 10,200
- Relatively efficient per-entity (~0.22µs per unit)

**Issues:**
- Still adds up with many entities
- Runs on all units every tick

**Optimization Opportunities:**
- [ ] SIMD optimization for vector math
- [ ] Reduce calculation frequency for distant units
- [ ] Level-of-detail system (simplified steering for background units)
- [ ] Parallel processing with rayon

**Related Code:**
- [src/game/unit/boids.rs](../../src/game/unit/boids.rs)

---

### 5. Pathfinding Queue Buildup - LOW to MEDIUM
**Status:** Active - Needs Monitoring  
**Direct Time:** ~471µs for FOLLOW_PATH system  

**Metrics:**
- Active paths: 1,678-3,315 units following paths
- Path request queue: **1,874 pending requests** (WARNING)
- Follow path system: 471µs for 3,315 paths (~0.14µs per path)

**Issues:**
- Path request queue building up (1,874 pending)
- May cause pathfinding system to lag behind demand
- Not directly causing tick slowdown but could cascade

**Optimization Opportunities:**
- [ ] Increase pathfinding processing capacity
- [ ] Batch pathfinding requests
- [ ] Implement path request prioritization
- [ ] Consider async pathfinding on separate thread
- [ ] Cache common paths
- [ ] Reduce path recalculation frequency

**Related Code:**
- [src/game/pathfinding/systems.rs](../../src/game/pathfinding/systems.rs)

---

## Architectural Issues

### Current Architecture Limitations:
1. **No parallelization visible** - all systems appear sequential
2. **Per-entity processing** - not leveraging batch operations
3. **Fixed tick rate** - no dynamic LOD or update frequency
4. **Every-tick updates** - even for static or distant entities
5. **CPU-bound** - no GPU compute for massive parallel operations

### Suggested Architectural Changes:
- [ ] **Parallel ECS queries** using rayon for all major systems
- [ ] **GPU compute shaders** for boids, collision, spatial partitioning
- [ ] **LOD system** - reduce update frequency/quality for distant units
- [ ] **Spatial chunking** - only update active chunks
- [ ] **Multi-threaded simulation** - separate threads for different systems
- [ ] **Async pathfinding** - dedicated thread pool
- [ ] **SIMD vectorization** - for all math-heavy operations

---

## Immediate Action Items (Priority Order)

1. **Profile BOIDS_CACHE system** - understand what's taking 13ms
2. **Reduce BOIDS_CACHE frequency** - skip ticks or use dirty flags
3. **Tighten collision search radius** - reduce from 2.5x to improve hit ratio
4. **Add parallel processing** - use rayon for boids/collision
5. **Implement LOD system** - reduce quality for distant units
6. **Address pathfinding queue** - prevent request buildup
7. **GPU acceleration** - move boids/collision to compute shaders

---

## Scaling Path to 10M Entities

### Phase 1: Optimize Current Systems (Target: 100K entities @ 100 tps)
- Fix BOIDS_CACHE performance (13ms → 1ms for 10K entities)
- Parallelize all major systems
- Implement basic LOD

### Phase 2: Architectural Shift (Target: 1M entities @ 100 tps)
- GPU compute shaders for collision and boids
- Spatial chunking for world updates
- Dedicated pathfinding thread pool

### Phase 3: Extreme Optimization (Target: 10M entities @ 100 tps)
- Full GPU simulation pipeline
- Hierarchical LOD system
- Distributed processing across cores

---

## Test Data Source
- **Log File:** peregrine_20260109_230728.log
- **Test Scenario:** 10,200 units on 500x500 map with pathfinding commands
- **Date:** January 9, 2026
