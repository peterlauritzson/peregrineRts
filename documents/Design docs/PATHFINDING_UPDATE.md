# Pathfinding Implementation Gap Analysis

**Date:** January 18, 2026  
**Status:** Implementation Review vs. Design Document

This document identifies gaps between the current pathfinding implementation and the design specified in [PATHFINDING.md](PATHFINDING.md).

---

## Executive Summary

The current implementation has made **significant progress** toward the region-based hierarchical pathfinding system, but is **missing several critical components** outlined in the design document:

### ✅ Implemented (Working)
- Basic region decomposition (maximal rectangles)
- Region connectivity graph and local routing tables
- Island detection using tortuosity threshold
- Island-aware routing table
- Inter-cluster portal creation
- Basic movement integration in simulation systems

### ⚠️ Partially Implemented
- Shared edge navigation (uses portals within regions, not pure shared edges)
- Island detection (all islands, not boundary-focused)
- Point-in-region lookups

### ❌ Missing (Critical Gaps)
1. **Dangerous region flag** - No `is_dangerous` field on regions
2. **Shared edge navigation** - Still uses `RegionPortal` objects instead of computing shared edges on-demand
3. **Boundary-focused island detection** - Creates islands for ALL disconnected regions, not just boundary regions
4. **Caching system** - No cached region/cluster on units
5. **Skip-frame validation** - No frame-skipping for region validation
6. **Region fragmentation mitigation** - None of the proposed solutions (dilation, aspect ratio filtering, etc.)
7. **Dangerous region handling** - No local A* fallback for non-convex regions
8. **Unreachable target handling** - No snap-to-walkable or proper error handling
9. **Dynamic update support** - No building placement handling or path invalidation

---

## 1. Region Representation Issues

### Issue 1.1: Missing `is_dangerous` Flag

**Design Doc Says:**
```rust
pub struct Region {
    pub id: RegionId,
    pub bounds: Rect,
    pub vertices: SmallVec<[FixedVec2; 8]>,
    pub island: IslandId,
    pub portals: SmallVec<[RegionPortal; 8]>,
    pub is_dangerous: bool,  // SHOULD BE HERE
}
```

**Current Implementation:**
```rust
// src/game/pathfinding/types.rs:109
pub struct Region {
    pub id: RegionId,
    pub bounds: Rect,
    pub vertices: SmallVec<[FixedVec2; 8]>,
    pub island: IslandId,
    pub portals: SmallVec<[RegionPortal; 8]>,
    // is_dangerous MISSING!
}
```

**Impact:**
- Cannot mark non-convex merged regions for special handling
- No way to identify regions that need local A* pathfinding
- System assumes all regions are convex (incorrect for complex areas)

**Fix Required:**
1. Add `is_dangerous: bool` field to `Region` struct
2. During decomposition, mark regions that:
   - Were merged from multiple rectangles
   - Have > 4 vertices
   - Failed convexity test
3. In movement code, check `is_dangerous` and use different logic

---

### Issue 1.2: Portal Objects Instead of Shared Edges

**Design Doc Says:**
> **NO "Portal Objects":** Regions connect via their shared geometry, not through separate portal entities. The shared edge IS the connection.

**Current Implementation:**
```rust
// src/game/pathfinding/types.rs:117
pub struct RegionPortal {
    pub edge: LineSegment,      // ✅ Has the edge
    pub center: FixedVec2,       // ⚠️ Pre-computed (should be computed on-demand)
    pub next_region: RegionId,   // ✅ Correct
}

// src/game/pathfinding/types.rs:112
pub struct Region {
    pub portals: SmallVec<[RegionPortal; 8]>,  // ⚠️ Should not store portals
}
```

**Why This Matters:**
- Design doc emphasizes **compute-on-demand** to save memory
- Storing portals per region duplicates data (each edge stored twice)
- Violates the "shared edge, not portal object" principle

**Current Behavior:**
The system DOES store the shared edge in `RegionPortal`, so it's **partially correct**. The main deviation is:
1. Pre-computes `center` instead of computing on-demand
2. Stores portals in array instead of computing from neighbor list

**Recommendation:**
This is a **design deviation, not a bug**. The current approach trades memory (minimal) for speed (no edge recomputation). Consider this **acceptable** unless memory becomes an issue.

**If you want strict adherence to design doc:**
```rust
// Replace portals array with just neighbor list
pub struct Region {
    pub neighbors: SmallVec<[RegionId; 8]>,  // Just IDs, no portal data
}

// Compute shared edge on-demand during movement
fn get_shared_edge(region_a: &Region, region_b: &Region) -> LineSegment {
    // Compare vertices to find overlap (design doc section 2.2)
}
```

---

## 2. Island Detection Issues

### Issue 2.1: Not Boundary-Focused

**Design Doc Says (Section 3.3):**
> **CRITICAL:** Islands represent "sides of cross-cluster obstacles", NOT every disconnected pocket.
> 
> **Algorithm:**
> 1. Identify boundary regions (touch cluster edges or inter-cluster portals)
> 2. Create islands from boundary regions using tortuosity threshold
> 3. **Merge interior regions into nearest boundary island**

**Current Implementation:**
```rust
// src/game/pathfinding/island_detection.rs:16
pub(crate) fn identify_islands(cluster: &mut Cluster) {
    // Process each unassigned region
    for seed in 0..cluster.region_count {
        if assigned[seed] {
            continue;
        }
        
        // Start a new island (NO CHECK if it's a boundary region)
        // ...
    }
}
```

**Impact:**
- Creates islands for ALL disconnected regions, not just boundary regions
- This is why you see "144 clusters exceeding MAX_ISLANDS" warnings
- Interior isolated pockets create separate islands (wasteful)
- Routing table explodes in size (more island pairs)

**Evidence from Design Doc:**
> Without boundary-focused islands: ~20,000 nodes  
> With boundary-focused islands: ~8,000 nodes  
> **6x reduction in routing table size**

**Fix Required:**
1. Implement `is_boundary_region()` check:
   ```rust
   fn is_boundary_region(
       region: &Region, 
       cluster_bounds: Rect, 
       portals: &[Portal]
   ) -> bool {
       // Check if region touches cluster edge
       if region.bounds.intersects_rect_edge(cluster_bounds) {
           return true;
       }
       // Check if region contains inter-cluster portal
       for portal in portals {
           if region.bounds.contains(portal.center()) { 
               return true; 
           }
       }
       false
   }
   ```

2. Only create islands from boundary regions:
   ```rust
   let mut boundary_regions = vec![];
   for region in &cluster.regions {
       if is_boundary_region(region, cluster_bounds, portals) {
           boundary_regions.push(region);
       }
   }
   
   // Start flood fill only from boundary regions
   ```

3. Merge interior isolated regions:
   ```rust
   for interior_region in non_boundary_regions {
       let nearest_island = find_nearest_boundary_island(interior_region);
       interior_region.island = nearest_island;
   }
   ```

**Expected Result:**
- Island count drops from ~500 to ~100
- No more "exceeding MAX_ISLANDS" warnings
- Routing table size reduced 5-6x

---

## 3. Movement System Issues

### Issue 3.1: No Cached Region/Cluster

**Design Doc Says (Section 2.3):**
```rust
#[derive(Component)]
struct Unit {
    pos: Vec2,
    velocity: Vec2,
    cached_region: RegionId,        // Cache current region!
    cached_cluster: ClusterId,      // Cache current cluster
    frames_since_validation: u8,    // Track when to revalidate
}
```

**Current Implementation:**
```rust
// No caching at all! Every frame does:
let current_grid = flow_field.world_to_grid(pos.0);
let cx = gx / CLUSTER_SIZE;
let cy = gy / CLUSTER_SIZE;
let current_region = world_to_cluster_local(...)
    .and_then(|local_pos| get_region_id(...));
```

**Impact:**
- **Performance:** Does full region lookup EVERY FRAME (expensive)
- Units rarely change regions, so 95%+ of these lookups are wasted
- Design doc estimates **3.75x speedup** from caching

**Fix Required:**
1. Add to unit component:
   ```rust
   #[derive(Component)]
   pub struct PathCache {
       pub cached_cluster: (usize, usize),
       pub cached_region: RegionId,
       pub frames_since_validation: u8,
   }
   ```

2. In movement system:
   ```rust
   // Only revalidate every 4 frames
   if cache.frames_since_validation >= 4 {
       cache.frames_since_validation = 0;
       revalidate_cached_region(&mut cache, pos, flow_field);
   } else {
       cache.frames_since_validation += 1;
   }
   
   // Use cached values for routing
   let current_cluster = cache.cached_cluster;
   let current_region = cache.cached_region;
   ```

**Expected Performance Gain:**
- Without caching: ~75ns per unit per frame
- With skip-frame validation: ~20ns per unit per frame
- **3.75x speedup** (per design doc section 2.3)

---

### Issue 3.2: No Skip-Frame Validation

**Design Doc Says:**
```rust
// Only verify cached region every 4 frames
if frame_count % 4 == 0 {
    verify_cached_region();  // ~20ns
} else {
    // Trust cache           // ~5ns (just array lookup)
}
```

**Current Implementation:**
- Validates region **every single frame**
- No frame skipping logic

**Fix Required:**
Add `frames_since_validation` counter (see Issue 3.1)

---

### Issue 3.3: No Dangerous Region Handling

**Design Doc Says (Section 1.3):**
```rust
fn micro_navigate(
    unit_pos: Vec2,
    target_pos: Vec2,
    region: &Region,
) -> Result<Vec2, MovementError> {
    if !region.is_dangerous {
        // Straight line is guaranteed safe
        return Ok((target_pos - unit_pos).normalize());
    }
    
    // Case 2: Dangerous region (non-convex or complex)
    // TODO: IMPROVE - Add local A* for dangerous regions
    warn!("Moving through dangerous region - using direct path");
    Ok((target_pos - unit_pos).normalize())
}
```

**Current Implementation:**
```rust
// src/game/simulation/systems.rs:231
(Some(curr_reg), Some(goal_reg)) if curr_reg == goal_reg => {
    // Same region - move directly (convexity guarantees no obstacles)
    // ⚠️ Assumes ALL regions are convex!
    seek(pos.0, *goal, vel.0, &mut acc.0, speed, max_force);
}
```

**Impact:**
- Assumes all regions are convex
- Will fail in merged/complex regions
- No fallback for non-convex areas

**Fix Required:**
1. Add `is_dangerous` flag (Issue 1.1)
2. Check before direct movement:
   ```rust
   if curr_reg == goal_reg {
       if region.is_dangerous {
           warn_once!("Moving through dangerous region - direct movement");
           // TODO: Add local A* here
       }
       seek(pos.0, *goal, ...);
   }
   ```

---

## 4. Region Decomposition Issues

### Issue 4.1: No Fragmentation Mitigation

**Design Doc Section 12** lists 10 solutions for circular obstacle fragmentation. **NONE are implemented.**

**Current Implementation:**
```rust
// src/game/pathfinding/region_decomposition.rs
// Just basic maximal rectangles algorithm
// No dilation, no aspect ratio filtering, no merging
```

**Impact:**
- Circular obstacles create 10-30 small rectangular regions
- Region count frequently exceeds MAX_REGIONS
- This is the root cause of the island explosion

**Design Doc Recommendations:**

**Immediate Quick Wins:**
1. **Solution 6: Obstacle Dilation** (Very Low complexity, one-line change)
   ```rust
   fn decompose_cluster_into_regions(
       cluster_id: (usize, usize),
       flow_field: &FlowField,
   ) -> Vec<Region> {
       // Dilate obstacles by 1-2 tiles
       let dilated_field = dilate_obstacles(flow_field, 1);
       
       // Use dilated field for decomposition
       let strips = find_horizontal_strips(..., &dilated_field);
       // Rest of algorithm unchanged
   }
   ```
   **Expected:** 60-80% reduction in region count

2. **Solution 3: Aspect Ratio Filtering** (Low complexity)
   ```rust
   fn merge_strips_into_rectangles(...) -> Vec<Rect> {
       let mut rectangles = /* existing algorithm */;
       
       // Filter out thin strips
       rectangles.retain_mut(|rect| {
           let aspect_ratio = rect.width().max(rect.height()) / 
                             rect.width().min(rect.height());
           
           if aspect_ratio > 5.0 && rect.area() < MIN_REGION_AREA {
               // Merge into neighbor
               return false;
           }
           true
       });
       
       rectangles
   }
   ```
   **Expected:** 40-60% reduction

**Long-term Solution:**
- **Solution 1: Core + Fringe Decomposition** (best results, medium complexity)
- See design doc lines 2479-2532 for full algorithm

---

### Issue 4.2: No Convexity Testing

**Design Doc Says:**
```rust
// Step 4: Mark non-convex regions as "dangerous"
for region in &mut final_regions {
    region.is_dangerous = !region.bounds.is_convex();
}
```

**Current Implementation:**
```rust
// src/game/pathfinding/region_decomposition.rs:58
regions.push(Region {
    id: RegionId(i as u8),
    bounds: rect,
    vertices,
    island: IslandId(0),
    portals: SmallVec::new(),
    // No is_dangerous field at all!
});
```

**Fix Required:**
1. Add convexity test helper:
   ```rust
   fn is_convex_polygon(vertices: &[FixedVec2]) -> bool {
       if vertices.len() != 4 {
           return false;  // Only rectangles guaranteed convex
       }
       
       // For rectangles, check if all angles are ~90 degrees
       // Or just assume axis-aligned rectangles are convex
       true
   }
   ```

2. Mark regions during decomposition:
   ```rust
   let is_dangerous = !is_convex_polygon(&vertices);
   ```

---

## 5. Path Request System Issues

### Issue 5.1: No Snap-to-Walkable

**Design Doc Says (Section 6.2):**
```rust
fn snap_to_walkable(pos: Vec2) -> Result<Vec2, PathError> {
    if is_walkable(pos) {
        return Ok(pos);
    }
    
    // Search in expanding radius for walkable tile
    const MAX_SEARCH_RADIUS: f32 = 10.0;
    for radius in 1..=(MAX_SEARCH_RADIUS as i32) {
        for angle in 0..8 {
            let test_pos = pos + Vec2::from_angle(angle) * radius;
            if is_walkable(test_pos) {
                return Ok(test_pos);
            }
        }
    }
    
    Err(PathError::NoWalkableNearby)
}
```

**Current Implementation:**
```rust
// src/game/pathfinding/systems.rs:35
pub fn process_path_requests(...) {
    for request in path_requests.read() {
        let goal_node_opt = flow_field.world_to_grid(request.goal);
        
        if let Some(goal_node) = goal_node_opt {
            // Just uses goal as-is, no walkability check!
            // No snap-to-walkable
        }
    }
}
```

**Impact:**
- Player clicks inside wall → path to unwalkable location
- Units get stuck or behave erratically
- No user feedback about invalid targets

**Fix Required:**
```rust
pub fn process_path_requests(...) {
    for request in path_requests.read() {
        // Snap to walkable first
        let walkable_goal = match snap_to_walkable(request.goal, flow_field) {
            Ok(pos) => pos,
            Err(_) => {
                warn!("Goal {:?} is unreachable - no walkable tile nearby", request.goal);
                continue;  // Don't create path
            }
        };
        
        // Use walkable_goal instead of request.goal
        let goal_node_opt = flow_field.world_to_grid(walkable_goal);
        // ...
    }
}
```

---

### Issue 5.2: No Reachability Validation

**Design Doc Says (Section 2.1):**
```rust
pub fn request_path(...) -> Result<PathRequest, PathError> {
    // ...
    
    // Validate reachability
    if !are_islands_connected(
        ClusterIslandId(start_cluster, start_island),
        ClusterIslandId(goal_cluster, goal_island),
        routing_table,
    ) {
        return Err(PathError::Unreachable);
    }
    
    Ok(PathRequest { ... })
}
```

**Current Implementation:**
```rust
// No reachability check!
// Always creates Path component even if unreachable
```

**Impact:**
- Units try to path to unreachable locations
- Waste computation on impossible paths
- No user feedback that target is unreachable

**Fix Required:**
```rust
// Before creating Path component
let start_island_id = ClusterIslandId::new(start_cluster, start_island);
let goal_island_id = ClusterIslandId::new(goal_cluster, goal_island);

if graph.get_next_portal_for_island(start_island_id, goal_island_id).is_none() {
    warn!("No route from {:?} to {:?}", start_island_id, goal_island_id);
    // Don't create Path component
    continue;
}
```

---

## 6. Missing Optimizations

### Issue 6.1: No Routing Table Cache

**Design Doc Says (Section 9.3):**
```rust
struct RoutingCache {
    cache: HashMap<(ClusterIslandId, ClusterIslandId), PortalId>,
    capacity: usize,
    hits: usize,
    misses: usize,
}

// Expected: 90%+ hit rate for common destinations
```

**Current Implementation:**
- BTreeMap lookup every frame (O(log n))
- No caching of hot paths

**Recommendation:**
This is a **future optimization**, not critical for correctness. Implement after basic system is working.

---

### Issue 6.2: No Group Leadership Pathfinding

**Design Doc Says (Section 9.2):**
```rust
struct FormationGroup {
    leader: Entity,
    followers: Vec<Entity>,
}

// Leader gets full pathfinding
// Followers use local steering (boids)
// 95% reduction in path requests (1 per 20 units)
```

**Current Implementation:**
- Every unit does full pathfinding
- No group formations

**Recommendation:**
This is a **future optimization** for scaling to 10M units. Not needed initially.

---

## 7. Dynamic Updates (Missing Entirely)

### Issue 7.1: No Building Placement Support

**Design Doc Section 5** describes dynamic updates when buildings are placed:

```rust
pub fn on_building_placed(
    cluster_id: (usize, usize),
    graph: &mut HierarchicalGraph,
    flow_field: &FlowField,
) {
    // 1. Lock cluster
    // 2. Re-decompose into regions
    // 3. Update island detection
    // 4. Update routing tables
    // 5. Invalidate affected paths
}
```

**Current Implementation:**
- No dynamic update support
- Building placement would break pathfinding

**Impact:**
- Cannot place buildings during gameplay
- Map must be static after initial generation

**Fix Required:**
Implement as per design doc section 5, lines 1090-1150.

---

### Issue 7.2: No Path Invalidation

**Design Doc Says (Section 6.3):**
```rust
// When path becomes invalid mid-route
if next_cluster_island == NO_PATH {
    // Path was valid when requested, but is now broken
    commands.entity(unit.entity).remove::<Path>();
    emit_event(PathFailed { unit_id, reason: DynamicObstacle });
}
```

**Current Implementation:**
```rust
// src/game/simulation/systems.rs:364
if let Some(next_portal_id) = graph.get_next_portal_for_island(...) {
    // Use portal
} else {
    // Just falls back to direct movement!
    // No path invalidation
    warn_once!("NO ROUTE FOUND! Falling back to direct movement");
    seek(pos.0, *goal, ...);
}
```

**Impact:**
- Units continue moving even when path is invalid
- No notification to player
- Confusing behavior

**Fix Required:**
```rust
if let Some(next_portal_id) = graph.get_next_portal_for_island(...) {
    // Use portal
} else {
    // Invalidate path
    commands.entity(entity).remove::<Path>();
    warn!("Path from {:?} to {:?} no longer valid", current, goal);
    continue;
}
```

---

## 8. Testing Gaps

### Issue 8.1: No Integration Tests

**Design Doc Section 9** describes integration tests:
```rust
#[test]
fn test_full_path_across_map() {
    let map = create_test_map_with_obstacles();
    let path = request_path(start, goal, &graph);
    
    for _ in 0..10000 {
        update_unit_movement(&mut unit, &path);
    }
    
    assert!(unit.pos.distance(goal) < 1.0);
}
```

**Current Implementation:**
- Unit tests exist in `tests.rs`
- No full path integration tests
- No map crossing tests
- No obstacle avoidance tests

**Recommendation:**
Add tests as described in design doc section 9.

---

## 9. API Mismatches

### Issue 9.1: Path Component Structure

**Design Doc Says:**
```rust
struct PathRequest {
    goal: Vec2,
    goal_cluster: ClusterId,
    goal_region: RegionId,
    goal_island: IslandId,
}
```

**Current Implementation:**
```rust
// src/game/pathfinding/types.rs:67
pub enum Path {
    Direct(FixedVec2),
    LocalAStar { waypoints: Vec<FixedVec2>, current_index: usize },
    Hierarchical {
        goal: FixedVec2,
        goal_cluster: (usize, usize),
        goal_island: IslandId,
        // goal_region is MISSING!
    }
}
```

**Impact:**
- Cannot do meso-navigation efficiently
- Has to re-lookup goal region every frame
- Should be cached in Path component

**Fix Required:**
```rust
Hierarchical {
    goal: FixedVec2,
    goal_cluster: (usize, usize),
    goal_region: RegionId,  // ADD THIS
    goal_island: IslandId,
}
```

---

## 10. Documentation Gaps

### Issue 10.1: No TODO Comments in Code

**Design Doc Section 7** lists specific TODO comments to add throughout codebase:

```rust
// TODO: IMPROVE - Add local A* for dangerous regions
// TODO: IMPROVE - Add clamped projection for smoother edge crossing
// TODO: IMPROVE - Use connectivity distance instead of euclidean
```

**Current Implementation:**
- No TODO markers in code
- Unclear what's stubbed vs. complete

**Fix Required:**
Add TODO comments as specified in design doc.

---

## Summary of Priority Fixes

### Critical (Blocking Correctness)
1. **Boundary-focused island detection** (Issue 2.1) - Fixes MAX_ISLANDS warnings
2. **Add `is_dangerous` flag** (Issue 1.1) - Required for convexity guarantee
3. **Snap-to-walkable** (Issue 5.1) - Prevents stuck units
4. **Reachability validation** (Issue 5.2) - Prevents impossible paths

### High Priority (Major Performance/Quality)
5. **Cached region/cluster** (Issue 3.1) - 3.75x speedup
6. **Skip-frame validation** (Issue 3.2) - Further speedup
7. **Region fragmentation mitigation** (Issue 4.1) - Reduces region explosion
8. **Dangerous region handling** (Issue 3.3) - Handles non-convex areas

### Medium Priority (Polish)
9. **Path invalidation** (Issue 7.2) - Better UX
10. **Add `goal_region` to Path** (Issue 9.1) - Cleaner API
11. **Convexity testing** (Issue 4.2) - Required for dangerous flag

### Low Priority (Future Optimizations)
12. **Routing table cache** (Issue 6.1) - Micro-optimization
13. **Group leadership** (Issue 6.2) - For 10M+ units
14. **Building placement support** (Issue 7.1) - Dynamic gameplay
15. **Integration tests** (Issue 8.1) - Quality assurance

---

## Recommended Implementation Order

### Phase 1: Fix Critical Bugs (1-2 days)
1. Add `is_dangerous` flag to Region struct
2. Implement snap-to-walkable in path requests
3. Add reachability validation
4. Mark regions as dangerous during decomposition

### Phase 2: Optimize Island Detection (1 day)
5. Implement boundary-focused island detection
6. Add interior region merging
7. Test island count reduction

### Phase 3: Add Performance Caching (1 day)
8. Add PathCache component to units
9. Implement skip-frame validation
10. Measure performance improvements

### Phase 4: Fix Region Fragmentation (2 days)
11. Implement obstacle dilation (quick win)
12. Add aspect ratio filtering
13. Consider core+fringe decomposition

### Phase 5: Polish & Testing (1-2 days)
14. Add dangerous region handling in movement
15. Implement path invalidation
16. Add integration tests
17. Add TODO comments throughout

**Total Estimated Time: 6-8 days of focused work**

---

## Conclusion

The current implementation has built the **foundational structure** correctly:
- Region decomposition works
- Local routing tables work
- Island detection works (just creates too many islands)
- Island-aware routing works

The main gaps are:
1. **Missing the boundary-focused island optimization** (causing MAX_ISLANDS warnings)
2. **No performance caching** (missing 3.75x speedup)
3. **No region fragmentation fixes** (root cause of island explosion)
4. **Missing error handling** (snap-to-walkable, reachability checks)

Fixing these will bring the implementation into full alignment with the design document and enable the system to handle large-scale pathfinding at the performance levels specified.
