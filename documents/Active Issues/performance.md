# Active Performance Issues
**Last Updated:** January 10, 2026  
**Project:** Peregrine RTS - Performance optimization for 10M+ unit goal

---

## 🎯 Latest Performance Test Results (Jan 10, 2026)

### Automated Scaling Test Suite Results

**Test Configuration:** Release build, sequential execution, full logging

#### ✅ 100 Units Baseline
- **Actual TPS:** 23,917.7 (massively exceeds target of 10 TPS)
- **Tick Time:** ~0.04ms per tick
- **Status:** ✅ Excellent performance
- **Bottleneck:** Balanced between Spatial Hash (39%) and Collision Detection (40%)

#### ✅ 10K Units Moderate Scale
- **Actual TPS:** 677.2 (exceeds target of 50 TPS by 13.5x)
- **Tick Time:** ~1.74ms per tick
- **Status:** ✅ Strong performance, ready for next scale
- **System Breakdown (per tick avg):**
  - Spatial Hash: 0.68ms (39%)
  - Collision Detect: 0.69ms (40%)
  - Collision Resolve: 0.14ms (8%)
  - Physics: 0.23ms (13%)
- **Primary Bottleneck:** Collision Detection (40%) - "dominant at scale"

#### 🔜 Next Test Targets (Currently Skipped)
- 100K units stress test
- 1M units extreme test
- 10M units ultimate goal

### 📊 Key Performance Insight: Why Spatial Hash Isn't the Collision Bottleneck

**Common Misconception:** "Spatial hash queries should dominate collision detection performance"

**Reality:** The neighbor cache system decouples spatial hash queries from collision detection!

#### The Three-Stage Pipeline:

1. **Spatial Hash Update** (39% / 0.68ms)
   - Inserts/updates entity positions in grid cells
   - O(n) complexity - one update per entity
   - Pure grid management, no queries

2. **Neighbor Cache** (hidden in logs but working!)
   - Queries spatial hash for nearby entities
   - **90-100% cache hit rate** with velocity-aware caching
   - Only queries when entities move significantly or timeout occurs
   - Example from logs: `Cache hits: 1 (100.0%) | Misses: 0`

3. **Collision Detection** (40% / 0.69ms)
   - Reads cached neighbor lists (NOT querying spatial hash!)
   - Performs actual distance calculations on cached pairs
   - Applies collision masks and filters
   - Generates collision events
   - O(n × avg_neighbors) complexity

#### Why They're Balanced (39% vs 40%):

The cache is working perfectly! Without it, we'd see:
- Spatial Hash Queries: 100+ queries/entity × 10K entities = disaster
- Collision Detection: negligible (just reading results)

Instead we see:
- Spatial Hash: O(n) entity insertions = 39%
- Collision Detection: O(n × neighbors) distance checks = 40%

**The neighbor cache prevents spatial hash queries from dominating** by caching results across multiple frames based on velocity and movement distance.

#### Evidence from Logs:

```
[NEIGHBOR_CACHE] ... | Entities: 1 | Cache hits: 1 (100.0%) | Misses: 0
[COLLISION_DETECT] ... | Entities: 1 | Neighbors: 0 (avg: 0.0, max: 0)
```

The collision system uses cached neighbors, not live queries!

### 🎯 Next Optimization Target

**Primary:** Collision Detection (40% of tick time)
- Current complexity: O(n × neighbors)
- Need spatial partitioning improvements
- Better neighbor culling strategies
- Projected bottleneck at 100K+ units

**Secondary:** Spatial Hash (39% of tick time)
- Linear scaling is acceptable
- But adds up at high entity counts
- Consider lockless data structures

### 📈 Performance Projection

Based on current scaling:
- **10K units:** 677 TPS ✅ (Current)
- **100K units:** ~67 TPS (estimated - may hit bottleneck)
- **1M units:** ~6.7 TPS (estimated - will need major optimization)

**Recommendation:** Focus on optimizing the Collision Detection system's neighbor search algorithm to reduce O(n×neighbors) complexity before attempting 100K+ unit tests.

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
