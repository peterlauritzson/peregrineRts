# Peregrine Pathfinding Architecture: Hierarchical Grid Navigation with Convex Decomposition

## File Structure

**Graph Building**: `src/game/pathfinding/graph_build.rs` (~270 lines)
- Incremental graph construction state machine
- Portal discovery (vertical and horizontal)
- Routing table generation

**Graph Build Helpers**: `src/game/pathfinding/graph_build_helpers.rs` (115 lines)
- Cluster initialization and connectivity
- Intra-cluster portal connections via local A*
- Flow field precomputation for cached navigation
- Connected component analysis for reachability

## 1. The End Goal
To support **10 million units** on **large maps (2048x2048+)** with **dynamic obstacles** (building placement), we need:
- **O(1) lookups** for movement decisions (no dynamic A*)
- **Pre-allocated memory** (no runtime allocations for 10M+ units)
- **Robust local navigation** that doesn't break when units are in the same cell as their target

The solution is **Hierarchical Grid Navigation with Convex Micro-Region Decomposition**. This approach divides the map into clusters (macro-level) and further decomposes each cluster into convex regions (micro-level) for complete O(1) pathfinding at all scales.

### Key Features
*   **Scalability:** Movement cost is O(1) array lookups regardless of unit count
*   **Dynamic Updates:** Building placement triggers re-baking of only the affected cluster
*   **Memory Efficient:** Pre-allocated fixed-size arrays, no runtime allocations
*   **Robust Movement:** Units never get stuck due to convexity guarantees and proper handling of disconnected regions
*   **"Last Mile" Solution:** Proper pathfinding even when unit and target are in the same cluster

---

## 2. The Three-Level Hierarchy

### Level 0: The Tile Grid (Raw Terrain Data)
*   **Data:** Dense array of `u8` (1 = Walkable, 255 = Obstacle)
*   **Usage:** Source data for generating navigation structures
*   **Size:** 2048×2048 = 4MB for large map

### Level 1: The Macro Grid (Clusters / Spatial Hash)
*   **Purpose:** O(1) spatial indexing - instantly find which cluster contains a position
*   **Concept:** Map divided into fixed-size square chunks (e.g., 50×50 tiles)
*   **Structure:**
    ```rust
    struct Cluster {
        id: (usize, usize),              // Grid coordinates
        regions: [Region; MAX_REGIONS],  // Fixed-size array of convex regions
        region_count: usize,             // Actual number of regions in use
        
        // Lookup table: [current_region][target_region] = next_region
        // If regions are in different islands, returns NO_PATH
        local_routing: [[RegionId; MAX_REGIONS]; MAX_REGIONS],
        
        // Portals to neighboring clusters, grouped by island
        neighbor_connectivity: [[IslandId; 4]; MAX_ISLANDS], // [island][direction]
    }
    ```
*   **Why Keep the Grid?** Point location. Without it, finding which region contains position (x, y) requires checking thousands of polygons. With the grid, it's `Grid[x/cluster_size][y/cluster_size]` → only check ~5-10 regions in that cluster.

### Level 2: The Micro-Regions (Convex Decomposition)
*   **Purpose:** Solve the "Last Mile" problem - pathfinding within a cluster
*   **Concept:** Each cluster is decomposed into convex polygons/rectangles
*   **Key Property:** If unit and target are in the same convex region, straight-line movement is always valid (by definition of convexity)
*   **Structure:**
    ```rust
    struct Region {
        bounds: ConvexPolygon,           // Geometry (typically 4-8 vertices)
        island_id: IslandId,             // Which connected component this belongs to
        portals: [Portal; MAX_PORTALS],  // Connections to other regions
        portal_count: usize,
    }
    
    struct Portal {
        edge: LineSegment,               // Shared boundary with adjacent region
        next_region: RegionId,           // Which region this portal leads to
    }
    ```

### Level 3: The Islands (Connected Components)
*   **Purpose:** Handle disconnected/expensive-to-traverse regions within clusters
*   **The Problem:** A cluster might contain regions that are:
    1. **Physically disconnected** (river/wall splits the cluster)
    2. **Expensive to traverse** (U-shaped building requires long detour)
*   **Solution:** Group regions into "Islands" (connected components)
    *   Regions are in the same island if path distance ≤ threshold × euclidean distance
    *   The macro pathfinder routes to `(Cluster, Island)` pairs, not just clusters
    *   This prevents units from entering a cluster on the wrong side of a wall
*   **Structure:**
    ```rust
    struct Island {
        id: IslandId,
        regions: SmallVec<RegionId>,  // Regions in this island
    }
    ```

---

## 3. How Units Move (The Runtime Algorithm)

### A. Command Issued: Move to Target Position

1. **Quantize Target:**
   ```rust
   let target_cluster = get_cluster_id(target_pos);
   let target_region = get_region_id(target_cluster, target_pos);
   let target_island = clusters[target_cluster].regions[target_region].island_id;
   ```

2. **Validate Reachability:**
   ```rust
   let unit_cluster = get_cluster_id(unit.pos);
   let unit_region = get_region_id(unit_cluster, unit.pos);
   let unit_island = clusters[unit_cluster].regions[unit_region].island_id;
   
   if !are_connected(unit_cluster, unit_island, target_cluster, target_island) {
       // Target is unreachable - see "Unreachable Target Handling" section
       return handle_unreachable_target(unit, target_pos);
   }
   ```

3. **Set Path:**
   ```rust
   unit.path = Path::Hierarchical {
       goal: target_pos,
       goal_cluster: target_cluster,
       goal_island: target_island,
   };
   ```

### B. Movement Loop (Every Frame)

**No flow fields needed - pure region-to-region navigation:**

```rust
fn update_unit_movement(unit: &mut Unit, clusters: &ClusterGrid) {
    let current_cluster = get_cluster_id(unit.pos);
    let cluster = &clusters[current_cluster];
    let current_region = get_region_id(cluster, unit.pos);
    
    // Case 1: In target cluster (The "Last Mile")
    if current_cluster == unit.path.goal_cluster {
        let target_region = get_region_id(cluster, unit.path.goal);
        
        // Case 1a: Same region - convexity guarantees straight line is safe
        if current_region == target_region {
            let direction = (unit.path.goal - unit.pos).normalize();
            unit.velocity = direction * UNIT_SPEED;
            return;
        }
        
        // Case 1b: Different region in same cluster
        let next_region = cluster.local_routing[current_region][target_region];
        
        if next_region == NO_PATH {
            // Different islands - shouldn't happen (macro path validates this)
            unit.velocity = FixedVec2::ZERO;
            return;
        }
        
        // Find portal between current and next region
        let portal = cluster.regions[current_region]
            .portals
            .iter()
            .find(|p| p.next_region == next_region)
            .expect("Routing table error");
        
        // Move to portal center (simplest/fastest for 10M units)
        let portal_center = (portal.edge.start + portal.edge.end) / 2.0;
        let direction = (portal_center - unit.pos).normalize();
        unit.velocity = direction * UNIT_SPEED;
        return;
    }
    
    // Case 2: Not in target cluster yet (Macro navigation via regions)
    let exit_portal_id = routing_table
        [(current_cluster, unit.path.current_island)]
        [(unit.path.goal_cluster, unit.path.goal_island)];
    
    let inter_cluster_portal = &graph.inter_cluster_portals[exit_portal_id];
    
    // Find which region in our cluster contains this exit portal
    let portal_region = get_region_id(cluster, inter_cluster_portal.center());
    
    if current_region == portal_region {
        // We're in the exit portal's region - move directly to portal
        let direction = (inter_cluster_portal.center() - unit.pos).normalize();
        unit.velocity = direction * UNIT_SPEED;
    } else {
        // Navigate region-to-region to reach the exit portal's region
        let next_region = cluster.local_routing[current_region][portal_region];
        
        let portal = cluster.regions[current_region]
            .portals
            .iter()
            .find(|p| p.next_region == next_region)
            .expect("Routing table error");
        
        let portal_center = (portal.edge.start + portal.edge.end) / 2.0;
        let direction = (portal_center - unit.pos).normalize();
        unit.velocity = direction * UNIT_SPEED;
    }
}
```

**Key Point:** No flow field sampling! Navigation is:
1. Get current region (point-in-polygon tests)
2. Look up next region (array index into local_routing table)
3. Find portal to next region (small linear search)
4. Move toward portal center (vector math)

All operations are O(1) or O(small constant).

### C. Why This is O(1)
*   **get_cluster_id:** `Grid[x / cluster_size][y / cluster_size]` - array index
*   **get_region_id:** Check ~5-10 polygons (point-in-polygon test)
*   **local_routing lookup:** `table[from][to]` - array index
*   **routing_table lookup:** Precomputed hash/array lookup
*   **No loops, no allocations, no dynamic pathfinding**

### D. Movement Smoothing Techniques

The basic algorithm above works but can produce robotic movement with sharp turns. Here are techniques to make movement organic:

#### Technique 1: Anticipatory Blending (Cluster Boundaries)

**Problem:** When crossing cluster boundaries, the movement vector changes abruptly, causing a visible "snap."

**Solution:** Look ahead and blend flow vectors from current and next cluster.

```rust
fn get_steering_vector(unit: &Unit, clusters: &ClusterGrid) -> FixedVec2 {
    const BLEND_DIST: FixedNum = FixedNum::from_num(3.0); // tiles
    const LOOKAHEAD: FixedNum = FixedNum::from_num(2.0);
    
    // Get base movement vector
    let v_current = get_movement_vector(unit, clusters);
    
    // Check if near cluster boundary
    let dist_to_boundary = distance_to_next_cluster_boundary(unit.pos, unit.current_cluster);
    
    if dist_to_boundary < BLEND_DIST {
        // Project future position
        let future_pos = unit.pos + v_current.normalize() * LOOKAHEAD;
        let next_cluster = get_cluster_id(future_pos);
        
        if next_cluster != unit.current_cluster {
            // Sample movement vector from next cluster
            let v_next = get_movement_vector_at_position(future_pos, next_cluster, clusters);
            
            // Blend based on distance to boundary
            let blend_factor = FixedNum::ONE - (dist_to_boundary / BLEND_DIST);
            return v_current.lerp(v_next, blend_factor);
        }
    }
    
    v_current
}
```

**Cost:** ~5 extra operations per unit per frame  
**Benefit:** Eliminates visible "kinks" at cluster boundaries

#### Technique 2: Clamped Projection Refinement (Portal Selection)

**Problem:** Unit enters portal center but target is far right in next region → sharp turn after crossing.

**Solution:** Instead of projecting straight to portal edge, project the **target direction** onto the portal.

```rust
fn get_optimal_portal_crossing(
    unit_pos: FixedVec2,
    target_pos: FixedVec2,  // Final destination, not just next region
    portal: &LineSegment,
) -> FixedVec2 {
    // Direction from unit to ultimate target
    let direction_to_target = (target_pos - unit_pos).normalize();
    
    // Project this direction onto the portal edge
    let portal_vec = portal.end - portal.start;
    let portal_len_sq = portal_vec.length_squared();
    
    if portal_len_sq < FixedNum::EPSILON {
        return portal.start; // Degenerate portal
    }
    
    // Find where the "ideal path" would cross the portal
    // This is the intersection of line(unit_pos, target_pos) with portal line
    let to_portal = portal.start - unit_pos;
    let denom = cross_2d(direction_to_target, portal_vec.normalize());
    
    if denom.abs() < FixedNum::EPSILON {
        // Parallel - use center of portal
        return portal.start + portal_vec * FixedNum::from_num(0.5);
    }
    
    let t = cross_2d(to_portal, portal_vec.normalize()) / denom;
    let intersection = unit_pos + direction_to_target * t;
    
    // Clamp to portal bounds (with unit_radius clearance)
    clamp_to_segment(intersection, portal.start, portal.end, unit_radius)
}

fn cross_2d(a: FixedVec2, b: FixedVec2) -> FixedNum {
    a.x * b.y - a.y * b.x
}
```

**Effect:** Unit "cuts the corner" intelligently, flowing toward where it will actually go next.

#### Technique 3: Portal-to-Portal Flow Fields (Optional Enhancement, Not Recommended for 10M Units)

**CRITICAL: Flow fields are NOT needed for the core system to work!**

The region-based approach handles all navigation through region-to-region transitions. Flow fields are a purely optional visual enhancement for smoother movement in specific scenarios.

**Why Flow Fields Aren't Necessary:**

*Old system problem:*
- Units needed to navigate *within* a cluster to reach portals
- No proper intra-cluster pathfinding → used flow fields as workaround
- "How do I get to the North portal from here?" → Sample flow field vector

*New system solution:*
- Regions provide proper intra-cluster pathfinding via connectivity graph
- "How do I get to the North portal?" → Navigate region-to-region:
  1. Current region → look up next region in local_routing table
  2. Find portal between current and next region
  3. Move to portal center
  4. Repeat until at exit portal
- No flow field sampling required!

**If You Do Want Flow Fields (Not Recommended for 10M Units):**

**Optional Enhancement:** In addition to regions, add directional flow fields between portals for smoother movement.

**Concept:**
- Bake flow fields not just "to North portal" but "from South portal to North portal"
- Creates smooth streamlines through clusters
- Units entering from different directions flow organically

**Memory Cost:**
```rust
// For a cluster with 4 neighbor directions
// Permutations: 4 entry × 3 exits = 12 flow fields
// Size: 25×25 tiles × (2 bytes per vector) = 1.25 KB per field
// Total: 12 × 1.25 KB = 15 KB per cluster

// For 2048×2048 map with 1,680 clusters:
// 1,680 × 15 KB ≈ 25 MB (much less than current 504 MB!)
```

**When to Use:**
- **Regions (recommended):** Better for complex interiors, dynamic obstacles, lower memory
- **Portal-to-Portal Fields:** Better for very large open spaces, smoother flow, simpler implementation

**Hybrid Approach:**
```rust
struct Cluster {
    // Primary: Convex regions (always present)
    regions: [Option<Region>; MAX_REGIONS],
    local_routing: [[u8; MAX_REGIONS]; MAX_REGIONS],
    
    // Optional: Portal-to-portal flow fields (for large open clusters)
    portal_flow_fields: Option<BTreeMap<(PortalId, PortalId), FlowField>>,
}

fn navigate_within_cluster(unit: &Unit, cluster: &Cluster) {
    // Use flow fields if available (smoother but more memory)
    if let Some(fields) = &cluster.portal_flow_fields {
        if let Some(field) = fields.get(&(unit.entry_portal, unit.exit_portal)) {
            return field.sample(unit.pos);
        }
    }
    
    // Fall back to region-based navigation (always works)
    navigate_via_regions(unit, cluster)
}
```

#### Technique 4: Wall Repulsion Cost (During Baking)

**Applies to:** Both flow fields and region decomposition

**Problem:** Units scrape walls and look robotic

**Solution:** During navigation data generation, bias paths away from obstacles

**For Flow Fields:**
```rust
fn generate_flow_field_with_clearance(cluster: Cluster, goal: Portal) {
    // Precompute clearance map
    let clearance_map = calculate_distance_to_obstacles(cluster);
    
    // During Dijkstra expansion, add cost based on clearance
    for neighbor in neighbors(current) {
        let base_cost = 1.0;
        let clearance = clearance_map[neighbor];
        
        let wall_penalty = if clearance < 2.0 {
            5.0  // Near wall - heavily discouraged
        } else if clearance < 4.0 {
            2.0  // Somewhat near wall
        } else {
            0.0  // Open space - no penalty
        };
        
        let total_cost = base_cost + wall_penalty;
        // ... continue Dijkstra with total_cost
    }
}
```

**For Convex Regions:**
```rust
fn decompose_with_clearance_bias(cluster: Cluster) {
    // When merging rectangles, prefer larger open areas
    // Avoid creating narrow regions along walls
    // This naturally guides units to cluster centers
}
```

**Effect:** Units naturally flow through the center of corridors, only moving near walls when necessary.

---

## 4. The Baking Process (Precomputation)

### Phase 1: Decompose Cluster into Convex Regions

**Input:** Cluster's walkable/obstacle tiles

**Algorithm:** Grid-Based NavMesh Generation
1. **Rasterize:** Treat cluster as tilemap (walkable = 1, obstacle = 0)
2. **Maximal Rectangles:** Merge walkable tiles into largest possible convex rectangles
3. **Trace Contours:** Find outlines of walkable areas
4. **Convex Partitioning:** Break complex shapes into convex polygons
   - Use triangulation + merging, or Hertel-Mehlhorn algorithm
5. **Cull Tiny Regions:** Remove regions smaller than unit radius

**Output:** Array of 5-30 convex regions per cluster

### Phase 2: Build Region Connectivity Graph

1. **Identify Portals:**
   - For each pair of regions, check if they share an edge
   - Store the shared edge as a Portal

2. **Local Pathfinding:**
   - For each pair of regions, calculate shortest path distance
   - If regions share an edge: distance ≈ euclidean
   - If regions don't touch: run BFS/Dijkstra on region graph

3. **Build Routing Table:**
   ```rust
   for start_region in 0..region_count {
       for end_region in 0..region_count {
           let path = dijkstra(start_region, end_region);
           local_routing[start_region][end_region] = path.first_step;
       }
   }
   ```

### Phase 3: Identify Islands (Connected Components)

**Purpose:** Handle physically disconnected or expensive-to-traverse areas

**Algorithm:**
```rust
fn identify_islands(regions: &[Region], local_routing: &RoutingTable) -> Vec<Island> {
    let mut islands = vec![];
    let mut assigned = vec![false; regions.len()];
    
    for start_region in 0..regions.len() {
        if assigned[start_region] { continue; }
        
        let mut island_regions = vec![start_region];
        let mut queue = vec![start_region];
        assigned[start_region] = true;
        
        while let Some(current) = queue.pop() {
            for next in 0..regions.len() {
                if assigned[next] { continue; }
                
                // Check if regions are "well-connected"
                let path_dist = calculate_path_distance(current, next, &local_routing);
                let euclidean_dist = distance(regions[current].center, regions[next].center);
                let tortuosity = path_dist / euclidean_dist;
                
                // Threshold: if path is >3x longer than straight line, separate islands
                if tortuosity < TORTUOSITY_THRESHOLD {
                    island_regions.push(next);
                    queue.push(next);
                    assigned[next] = true;
                }
            }
        }
        
        islands.push(Island { id: islands.len(), regions: island_regions });
    }
    
    islands
}
```

**The Tortuosity Threshold:**
*   **Low threshold (1.5x):** More aggressive splitting, units take safer/longer routes around obstacles
*   **High threshold (5.0x):** Fewer islands, units willing to navigate complex interiors
*   **Recommended:** 2.5-3.0x for good balance

### Phase 4: Build Global Routing Table

**Input:** All clusters with their islands and portals

**Algorithm:**
1. **Build Macro Graph:**
   - Nodes: `(ClusterId, IslandId)` pairs
   - Edges: Connections between nodes based on portal connectivity
   
2. **All-Pairs Shortest Path:**
   ```rust
   for each (start_cluster, start_island) {
       run_dijkstra_from((start_cluster, start_island));
       for each (goal_cluster, goal_island) {
           routing_table[(start_cluster, start_island)][(goal_cluster, goal_island)] 
               = first_portal_on_path;
       }
   }
   ```

**Memory Cost:**
*   2048×2048 map = ~1700 clusters (50×50 tiles each)
*   Average 1.2 islands per cluster = ~2000 nodes
*   Routing table: 2000 × 2000 × 8 bytes = 32MB (acceptable)

---

## 5. Dynamic Updates (Building Placement)

**Event:** User places a wall/building in cluster (x, y)

**Update Process:**

1. **Lock Cluster:**
   ```rust
   let cluster = &mut clusters[x][y];
   cluster.is_dirty = true;
   ```

2. **Re-bake Cluster:**
   - Run Phase 1 (Convex Decomposition) on updated tilemap
   - Run Phase 2 (Region Connectivity)
   - Run Phase 3 (Island Identification)
   - **Time:** ~100 microseconds for typical cluster

3. **Handle Region ID Changes:**
   - Old regions may no longer exist or have different IDs
   - Units in this cluster need to relocate themselves:
     ```rust
     for unit in units_in_cluster(x, y) {
         unit.current_region = get_region_id(cluster, unit.pos);
         if unit.current_region == INVALID {
             // Unit is now inside obstacle - push to nearest valid region
             unit.pos = snap_to_nearest_walkable(unit.pos);
             unit.current_region = get_region_id(cluster, unit.pos);
         }
     }
     ```

4. **Update Global Routing:**
   - If islands changed, update macro graph edges
   - Re-run Dijkstra only for affected `(cluster_id, island_id)` nodes
   - **Time:** ~1-10ms depending on graph size

5. **Overflow Handling (Edge Case):**
   - If new decomposition exceeds `MAX_REGIONS` (e.g., user builds a maze):
     ```rust
     if region_count > MAX_REGIONS {
         // Fallback: Merge smallest regions until within limit
         merge_smallest_regions_until(region_count <= MAX_REGIONS);
         // OR: Mark cluster as "complex" and use slower fallback pathfinding
     }
     ```

**Result:** Cluster is updated in ~1-10ms, units automatically adapt

### Building Placement Constraints (Simplifying the System)

**The Problem:** Without constraints, players can create pathological cases that break the system's assumptions:
- Maze-like structures exceeding `MAX_REGIONS`
- Clusters split into 5+ disconnected islands
- Narrow choke points creating hundreds of tiny regions
- Buildings that completely disconnect large areas of the map

**The Insight:** We can dramatically simplify pathfinding logic by **preventing problematic buildings from being placed** rather than handling them at runtime.

#### Recommended Constraints

**1. Island Count Limit (Per Cluster)**

**Rule:** A building placement is rejected if it would create more than `MAX_ISLANDS_PER_CLUSTER` islands (recommended: 2-3).

**Rationale:**
- **Simplicity:** Most clusters have 1 island (fully connected). Allowing 2-3 handles natural terrain (rivers, cliffs).
- **Routing Complexity:** Each island multiplies the macro graph size. 3 islands per cluster = manageable. 10 islands = exponential explosion.
- **Entry Points:** With 1-2 islands per cluster, the macro pathfinder doesn't need to consider "which side of the cluster to enter from" for most of the map.

**Implementation:**
```rust
fn validate_building_placement(cluster: &Cluster, building: &Building) -> Result<(), PlacementError> {
    // Simulate the building placement
    let simulated_cluster = cluster.clone();
    simulated_cluster.add_obstacle(building.footprint);
    
    // Re-run island detection
    let islands = identify_islands(&simulated_cluster);
    
    if islands.len() > MAX_ISLANDS_PER_CLUSTER {
        return Err(PlacementError::TooManyIslands {
            current: cluster.island_count,
            would_create: islands.len(),
            max: MAX_ISLANDS_PER_CLUSTER,
        });
    }
    
    Ok(())
}
```

**User Feedback:**
- Red overlay on building preview: "Would split area into too many disconnected regions"
- Suggest alternative placement nearby

**2. Minimum Island Size**

**Rule:** A building is rejected if it would create an island smaller than `MIN_ISLAND_SIZE` tiles (recommended: 16×16 = 256 tiles).

**Rationale:**
- **Memory:** Tiny islands still require full routing table entries but are useless
- **Stuck Units:** Small islands trap units with nowhere to go
- **Clutter:** Prevents "Swiss cheese" terrain with pockets everywhere

**Implementation:**
```rust
fn validate_building_placement(cluster: &Cluster, building: &Building) -> Result<(), PlacementError> {
    let islands = identify_islands_after_placement(&cluster, &building);
    
    for island in islands {
        let size = island.regions.iter().map(|r| r.area()).sum();
        if size < MIN_ISLAND_SIZE && size > 0 {
            return Err(PlacementError::IslandTooSmall {
                size,
                min: MIN_ISLAND_SIZE,
            });
        }
    }
    
    Ok(())
}
```

**Special Case:** If placement would make an existing large island too small, also reject.

**3. Region Count Limit (Per Cluster)**

**Rule:** Reject buildings that would cause `region_count > MAX_REGIONS` (recommended: 32).

**Rationale:**
- **Fixed Memory:** We allocated `local_routing[MAX_REGIONS][MAX_REGIONS]` per cluster
- **Performance:** Region lookups scale with region count
- **Complexity:** 32+ regions indicates maze-like terrain

**Implementation:**
```rust
fn validate_building_placement(cluster: &Cluster, building: &Building) -> Result<(), PlacementError> {
    let simulated_regions = decompose_into_convex_regions(&cluster, &building);
    
    if simulated_regions.len() > MAX_REGIONS {
        return Err(PlacementError::TooManyRegions {
            would_create: simulated_regions.len(),
            max: MAX_REGIONS,
        });
    }
    
    Ok(())
}
```

**User Feedback:**
- "Area too complex for navigation - try larger/simpler buildings"

**4. Critical Path Protection**

**Rule:** Reject buildings that would completely disconnect major regions of the map.

**Rationale:**
- Prevents players from accidentally (or maliciously) cutting the map in half
- Maintains global connectivity for all units

**Implementation:**
```rust
fn validate_building_placement(
    cluster: &Cluster, 
    building: &Building,
    global_graph: &MacroGraph,
) -> Result<(), PlacementError> {
    // Simulate placement and re-bake cluster
    let new_cluster = simulate_placement(cluster, building);
    
    // Check if any previously-connected clusters are now disconnected
    for neighbor_cluster in cluster.neighbors() {
        if was_connected_before(cluster.id, neighbor_cluster.id, global_graph) {
            if !would_be_connected_after(&new_cluster, neighbor_cluster) {
                return Err(PlacementError::WouldDisconnectMap {
                    from: cluster.id,
                    to: neighbor_cluster.id,
                });
            }
        }
    }
    
    Ok(())
}
```

**User Feedback:**
- "Cannot place - would block critical path"
- Highlight the affected connection on minimap

#### Benefits of Constraint-Based Approach

**1. Simplified Routing Logic:**
```rust
// WITHOUT constraints: Must handle arbitrary island counts
fn get_next_portal(current: ClusterId, goal: (ClusterId, IslandId)) -> PortalId {
    // Need to check if current cluster even HAS the right island to exit from
    // May need to route WITHIN cluster to different island first
    // Complex multi-level decision tree
}

// WITH constraints (max 2-3 islands per cluster):
fn get_next_portal(current: (ClusterId, IslandId), goal: (ClusterId, IslandId)) -> PortalId {
    // Simple lookup - we know islands are well-formed
    routing_table[current][goal]  // O(1)
}
```

**2. Predictable Performance:**
- No pathological cases that cause 100ms update times
- Memory usage bounded and known at compile time
- No emergency fallbacks or "complex cluster" special cases

**3. Better User Experience:**
- Players get immediate feedback ("can't build here") instead of mysterious pathfinding failures later
- Game stays responsive even with aggressive building
- Clear rules about what's allowed

**4. Enables Assumptions:**
```rust
// We can now safely assume:
assert!(cluster.island_count <= MAX_ISLANDS);
assert!(cluster.region_count <= MAX_REGIONS);

// No need for defensive coding like:
if region_id >= cluster.regions.len() { /* panic recovery */ }
```

#### Implementation Strategy

**Phase 1: Soft Warnings (Development)**
- Log warnings when constraints would be violated
- Allow placement anyway
- Collect data on how often it happens in real gameplay

**Phase 2: Hard Validation (Production)**
- Reject invalid placements with clear error messages
- Add visual feedback (red overlay, reason tooltip)
- Fine-tune thresholds based on Phase 1 data

**Phase 3: Smart Suggestions (Polish)**
- When placement fails, suggest nearby valid locations
- Show "heat map" of regions where building would be allowed
- Auto-rotate buildings to fit constraints if possible

#### Edge Cases

**Scenario: Player builds wall across entire map**
- Each cluster along the wall gets split into 2 islands (North and South)
- This is ALLOWED (≤ MAX_ISLANDS)
- But now the map is disconnected
- **Solution:** Critical Path Protection (Rule 4) prevents the final wall segment from being placed

**Scenario: Very large building spanning 4 clusters**
- Need to validate ALL affected clusters
- If even one cluster would violate constraints, reject entire building
- **Implementation:** `validate_multi_cluster_placement()`

**Scenario: Natural terrain already has 2 islands, player adds 3rd**
- If MAX_ISLANDS = 3, this is allowed
- If MAX_ISLANDS = 2, reject
- **Design Choice:** Reserve 1 island "budget" for natural terrain, 1 for buildings

#### Configuration

```rust
pub struct PathfindingConstraints {
    pub max_islands_per_cluster: usize,    // Recommended: 3
    pub min_island_size: usize,            // Recommended: 256 tiles
    pub max_regions_per_cluster: usize,    // Recommended: 32
    pub enforce_critical_paths: bool,      // Recommended: true
    
    // Advanced
    pub allow_temporary_violations: bool,  // For scripted events
    pub max_global_islands: usize,         // Recommended: num_clusters * 2
}

impl Default for PathfindingConstraints {
    fn default() -> Self {
        Self {
            max_islands_per_cluster: 3,
            min_island_size: 256,
            max_regions_per_cluster: 32,
            enforce_critical_paths: true,
            allow_temporary_violations: false,
            max_global_islands: usize::MAX,  // No global limit by default
        }
    }
}
```

#### Testing Constraints

**Unit Tests:**
- Place building that splits cluster → Verify rejection
- Place building in empty cluster → Verify acceptance
- Place 100 buildings that respect constraints → All succeed

**Integration Tests:**
- Player builds long wall → Verify last segment rejected if splits map
- Player builds maze → Verify rejection when exceeds region limit
- Scripted event creates complex terrain → Works if `allow_temporary_violations = true`

**Performance Tests:**
- Validation should take <1ms even for multi-cluster buildings
- Cache island/region counts to avoid re-computation

---

## 6. Unreachable Target Handling (Design Decisions)

### Scenarios and Behavior

When a unit is commanded to an unreachable target, the system must handle it gracefully. Here are the scenarios and recommended behaviors:

#### Scenario 1: Target on Disconnected Island (Physical)
**Example:** Target is across an impassable river, on a separate landmass

**Behavior (RECOMMENDED):**
```rust
// During path request validation
if !are_connected(unit_cluster, unit_island, target_cluster, target_island) {
    // Option A: Fail immediately - don't set Path component
    warn!("Target {target_pos:?} is unreachable from {unit.pos:?}");
    // Optionally emit event for UI feedback (red X on minimap, etc.)
    emit_event(PathFailed { unit_id, reason: Unreachable });
    return;
    
    // Option B: Get as close as possible
    let nearest_reachable = find_nearest_reachable_point(unit, target_pos);
    unit.path = Path::Hierarchical {
        goal: nearest_reachable,
        goal_cluster: get_cluster_id(nearest_reachable),
        goal_island: get_island_id(nearest_reachable),
    };
    emit_event(PathPartial { unit_id, original_goal: target_pos, actual_goal: nearest_reachable });
}
```

**Design Decision:** **Fail immediately (Option A)** for clarity
- Players need to know their command failed
- "Get as close as possible" can be confusing (units stop at random places)
- UI should indicate unreachable targets (e.g., red targeting cursor)

#### Scenario 2: Target Inside Obstacle
**Example:** Player clicks inside a wall/building

**Behavior (RECOMMENDED):**
```rust
if !is_walkable(target_pos) {
    // Snap to nearest walkable position
    let snapped_target = snap_to_nearest_walkable(target_pos);
    unit.path = Path::Hierarchical {
        goal: snapped_target,
        goal_cluster: get_cluster_id(snapped_target),
        goal_island: get_island_id(snapped_target),
    };
}
```

**Design Decision:** **Snap to nearest walkable** - this is standard RTS behavior
- Most RTSs do this automatically
- Provides least surprising behavior for players

#### Scenario 3: Target Becomes Unreachable Mid-Path (Dynamic Obstacle)
**Example:** Building placed that cuts off the unit's current path

**Behavior (RECOMMENDED):**
```rust
// During movement update
let next_cluster_island = routing_table[current_cluster][(goal_cluster, goal_island)];

if next_cluster_island == NO_PATH {
    // Path was valid when requested, but is now broken
    
    // Option A: Re-validate and fail if still unreachable
    if !are_connected(current_cluster, current_island, goal_cluster, goal_island) {
        warn!("Path became unreachable for unit {unit_id}");
        unit.path = None;
        emit_event(PathInvalidated { unit_id, reason: ObstacleBlocked });
        return;
    }
    
    // Option B: Request re-path (triggers full validation again)
    emit_event(PathRequest { unit_id, target: unit.path.goal });
}
```

**Design Decision:** **Fail and notify** - let higher-level systems decide
- Don't auto-repath every frame (expensive with 10M units)
- Game logic layer can batch re-path requests
- Player might issue new commands anyway

#### Scenario 4: Target in Same Cluster, Different Island
**Example:** Unit enters cluster but is on wrong side of a U-shaped building

**Behavior (THIS SHOULD NEVER HAPPEN):**
```rust
// This case indicates a bug in the macro pathfinder
// The routing table should route around to enter from the correct side

if current_cluster == goal_cluster && current_island != goal_island {
    error!("Navigation error: In goal cluster but wrong island!");
    // Emergency fallback: treat as unreachable
    unit.path = None;
    emit_event(PathFailed { unit_id, reason: NavigationError });
}
```

**Design Decision:** **This is a critical error** - log and fix the pathfinding logic
- The island system exists specifically to prevent this
- If it happens, the baking phase has a bug

### Implementation Checklist

```rust
// 1. Validation during path request
fn process_path_request(unit: &Unit, target: Vec2, clusters: &ClusterGrid) -> Result<Path, PathError> {
    // Snap targets inside obstacles
    let walkable_target = if !is_walkable(target) {
        snap_to_nearest_walkable(target)
    } else {
        target
    };
    
    let target_cluster = get_cluster_id(walkable_target);
    let target_region = get_region_id(target_cluster, walkable_target);
    let target_island = clusters[target_cluster].regions[target_region].island_id;
    
    let unit_cluster = get_cluster_id(unit.pos);
    let unit_region = get_region_id(unit_cluster, unit.pos);
    let unit_island = clusters[unit_cluster].regions[unit_region].island_id;
    
    // Check reachability using connected components
    if !connected_components.are_connected(
        (unit_cluster, unit_island),
        (target_cluster, target_island)
    ) {
        return Err(PathError::Unreachable {
            from: (unit_cluster, unit_island),
            to: (target_cluster, target_island),
        });
    }
    
    Ok(Path::Hierarchical {
        goal: walkable_target,
        goal_cluster: target_cluster,
        goal_island: target_island,
    })
}

// 2. Graceful failure during movement
fn update_unit_movement(unit: &mut Unit, clusters: &ClusterGrid) {
    let current_cluster = get_cluster_id(unit.pos);
    
    if current_cluster != unit.path.goal_cluster {
        // Macro navigation
        let key = (current_cluster, unit.path.goal_cluster, unit.path.goal_island);
        
        match routing_table.get(key) {
            Some(next_portal) => navigate_to_portal(unit, next_portal),
            None => {
                // Path became invalid
                warn!("Routing table entry missing: {key:?}");
                unit.path = None;
                emit_event(PathInvalidated { 
                    unit_id: unit.id, 
                    reason: RoutingTableMissing 
                });
            }
        }
    } else {
        // Micro navigation (see Section 3)
        navigate_within_cluster(unit, clusters);
    }
}
```

---

## 7. Implementation Roadmap

We will build this iteratively to ensure the game remains playable at every step.

### Phase 1: Convex Decomposition (The Foundation)
*Goal: Replace flow fields with region-based navigation*

1. **Implement Rectangular Decomposition:**
   - For each cluster, merge walkable tiles into maximal rectangles
   - Store as fixed-size array `Region[MAX_REGIONS]`
   - Verify: Can represent open terrain (1 region) and complex rooms (~10 regions)

2. **Point-in-Region Lookup:**
   - Implement `get_region_id(cluster, position)`
   - Test: 10M lookups should take <10ms

3. **Basic Movement:**
   - Update unit movement to use regions
   - Same region? Move directly
   - Different region? Move to center of shared edge

**Success Metric:** Units navigate correctly within a single cluster

### Phase 2: Local Routing Tables
*Goal: Solve the "Last Mile" problem*

1. **Build Region Connectivity Graph:**
   - Identify shared edges between regions (portals)
   - Run BFS to build `local_routing[from_region][to_region]`

2. **Clamped Projection:**
   - Instead of moving to portal center, project target onto portal edge
   - Clamp to stay within portal bounds
   - Test: Units flow smoothly through doorways without bunching

3. **Validate Convexity:**
   - Same region? Verify straight-line movement avoids obstacles
   - If failures occur, decomposition algorithm needs refinement

**Success Metric:** Units navigate complex single-cluster layouts without getting stuck

### Phase 3: Islands / Connected Components
*Goal: Handle disconnected regions*

1. **Flood Fill Connectivity:**
   - Group regions into islands based on connectivity
   - Tag each region with `island_id`

2. **Tortuosity-Based Splitting:**
   - Calculate path_distance / euclidean_distance for region pairs
   - If ratio > 3.0, treat as separate islands
   - Test: U-shaped buildings create 2 islands

3. **Update Routing Table:**
   - Routing table key becomes `(cluster_id, island_id)` instead of just `cluster_id`
   - Units route to correct island, preventing wrong-side entry

**Success Metric:** Units don't enter clusters on the wrong side of obstacles

### Phase 4: Global Routing with Islands
*Goal: Macro-level pathfinding*

1. **Build Island-Aware Macro Graph:**
   - Nodes: `(ClusterId, IslandId)` pairs
   - Edges: Based on portal connectivity between islands

2. **All-Pairs Shortest Path:**
   - Run Dijkstra from each `(cluster, island)` node
   - Store in `routing_table[(cluster, island)][(goal_cluster, goal_island)] = next_portal`

3. **Unified Movement System:**
   - Combine macro (cluster-to-cluster) and micro (region-to-region) navigation
   - Use island-aware routing table for macro lookups

**Success Metric:** Units route correctly across entire map, avoiding disconnected areas

### Phase 5: Unreachable Target Handling
*Goal: Graceful failure for invalid commands*

1. **Path Validation:**
   - Check `connected_components.are_connected()` before setting Path
   - Return error for unreachable targets

2. **Target Snapping:**
   - Implement `snap_to_nearest_walkable()` for targets inside obstacles

3. **Dynamic Invalidation:**
   - Handle routing table misses during movement
   - Emit events for UI feedback

4. **UI Integration:**
   - Show red cursor for unreachable targets
   - Display warnings when paths become invalid

**Success Metric:** No undefined behavior or panics with invalid targets

### Phase 6: Dynamic Updates
*Goal: Handle map changes during gameplay*

1. **Cluster Re-baking:**
   - Detect when building placement affects a cluster
   - Re-run decomposition, connectivity, and island analysis
   - Update local routing tables

2. **Unit Relocation:**
   - Units in affected cluster re-acquire their region_id
   - Snap units pushed into obstacles back to walkable space

3. **Global Routing Update:**
   - Re-run Dijkstra for affected `(cluster, island)` nodes
   - Update routing table entries

4. **Path Invalidation:**
   - Detect when active paths are broken by new obstacles
   - Emit events for re-pathing

**Success Metric:** Building placement updates navigation in <10ms, units adapt automatically

### Phase 7: Optimization & Polish
*Goal: Performance at scale*

1. **Region Overflow Handling:**
   - Merge smallest regions when exceeding MAX_REGIONS
   - Add "complex cluster" fallback pathfinding

2. **Routing Table Caching:**
   - HashMap cache for hot routing entries (O(log n) → O(1))

3. **Memory Profiling:**
   - Verify fixed allocation, no runtime allocations
   - Optimize MAX_REGIONS based on actual map complexity

4. **Load Testing:**
   - Test with 1M, 10M units
   - Verify movement system stays <5ms per frame

**Success Metric:** Sustained 60 FPS with 10M units moving

---

## 8. Performance Characteristics

### Memory Footprint

**Per Cluster (50×50 tiles):**
- Regions: `32 regions × 64 bytes = 2 KB`
- Local Routing: `32 × 32 × 1 byte = 1 KB`
- Neighbor Connectivity: `4 islands × 4 directions × 1 byte = 16 bytes`
- **Total: ~3 KB per cluster**

**Global (2048×2048 map):**
- Clusters: `1,680 clusters × 3 KB = 5 MB`
- Routing Table: `(1,680 × 1.2 islands)² × 8 bytes = 32 MB`
- **Total: ~37 MB** (acceptable)

**Per Unit:**
- Path Component: `24 bytes` (goal + cluster + island)
- **10M units: 240 MB** (down from 1+ GB with portal lists)

### Runtime Performance

**Path Request (Validation Only):**
- Snap to walkable: O(1) spatial query
- Connectivity check: O(1) hash lookup
- **Total: <100 nanoseconds**

**Movement Update (Per Unit):**
- Get cluster: O(1) array index
- Get region: O(10) point-in-polygon checks
- Routing table lookup: O(log n) ≈ 10-20 nanoseconds
- Move calculation: O(1) vector math
- **Total: <200 nanoseconds per unit**

**10M Units Moving:**
- `10M × 200ns = 2 seconds`
- **Parallelized across 16 cores: 125ms** (acceptable for 60 FPS)

### Comparison to Current System

| Metric | Old (Portal Lists + Flow Fields) | New (Convex Regions) |
|--------|----------------------------------|----------------------|
| Path Request | 19ms / 100k units | <1ms / 100k units |
| Memory per Unit | 100+ bytes | 24 bytes |
| "Last Mile" Nav | Flow fields (memory-heavy) | Direct/Clamped (O(1)) |
| Unreachable Handling | Undefined | Validated & Graceful |
| Island Awareness | Partial (ConnectedComponents) | Full (baked into routing) |
| Dynamic Updates | Flow field regeneration | Region re-baking (faster) |

---

## 9. Future Optimizations

### Group Leadership & Shared Pathfinding

**Problem:** Even with O(1) lookups, processing 10M individual path requests has overhead

**Solution:** Leader-based pathfinding
- Select leaders for spatial groups of ~20-50 units
- Leaders get full pathfinding
- Followers use leader's goal with local steering (boids + obstacle avoidance)

**Benefits:**
- 95% reduction in path requests (1 per 20 units)
- Emergent formations from local forces
- Scales to 100M+ units

**Integration:**
- Works seamlessly with convex region system
- Leaders use normal `Path::Hierarchical`
- Followers use `Path::Follow { leader_id, offset }`

### Movement Approach Comparison

When implementing the navigation system, you have choices for how units move within clusters:

#### Option A: Convex Regions + Clamped Projection (Recommended)

**Pros:**
- Lowest memory: ~3KB per cluster (~5MB total for large map)
- Handles dynamic obstacles perfectly (just re-decompose affected cluster)
- Convexity guarantees - no stuck units in same region
- Works with any cluster complexity

**Cons:**
- Requires clamped projection math (slightly more complex)
- May need anticipatory blending for smooth cluster transitions
- Regions can be tricky to debug visually

**Best For:**
- Games with dynamic obstacles (building placement)
- Memory-constrained scenarios
- Complex indoor environments with many small rooms

#### Option B: Portal-to-Portal Flow Fields

**Pros:**
- Smoother, more organic movement automatically
- Simpler implementation (just sample vector field)
- Easier to visualize/debug
- Natural "spine following" through corridors

**Cons:**
- Higher memory: ~15KB per cluster (~25MB total)
- Regenerating fields after obstacles is expensive
- Doesn't solve "last mile" for arbitrary positions
- Requires clearance map for wall avoidance

**Best For:**
- Games with mostly static maps
- Large open areas where smoothness is critical
- If you already have flow field infrastructure

#### Option C: Hybrid Approach

**Combine both:**
```rust
struct Cluster {
    regions: [Option<Region>; MAX_REGIONS],           // Always present
    local_routing: [[u8; MAX_REGIONS]; MAX_REGIONS],  // Always present
    
    // Optional: Portal-to-portal fields for large/open clusters
    portal_flow_fields: Option<PortalFlowFieldCache>,
}
```

**Decision Logic:**
- **Small clusters (<10 regions):** Use regions only
- **Large open clusters (1-3 regions):** Add portal-to-portal flow fields
- **Complex clusters (>20 regions):** Use regions only (flow fields would be too many permutations)

**When Moving:**
```rust
fn navigate_within_cluster(unit: &Unit, cluster: &Cluster) {
    // Prefer flow fields if available (smoother)
    if cluster.is_open_terrain() && cluster.portal_flow_fields.is_some() {
        return use_portal_flow_field(unit, cluster);
    }
    
    // Fall back to region-based (always works)
    use_region_navigation(unit, cluster)
}
```

**Result:** Best of both worlds - smooth movement in open areas, robust handling of complex areas.

---

## 10. Algorithm Clarifications

### Subdivided Portals vs. Convex Regions

**Question:** How do subdivided portals (splitting North portal into Left/Center/Right) relate to convex regions?

**Answer:** They solve the same problem at different levels:

**Subdivided Portals (Inter-Cluster):**
- Splits the **boundary between clusters** into segments
- Helps macro pathfinder choose correct entry point
- Example: Unit going to far-right of target cluster can route to "East Portal Right segment" instead of "East Portal Center"

**Convex Regions (Intra-Cluster):**
- Splits the **interior of a cluster** into navigable areas
- Helps micro navigation within the cluster
- Example: Unit inside cluster can move directly (same region) or via portals (different regions)

**Relationship:**
- Subdivided portals reduce the need for many convex regions near cluster edges
- If you have good convex decomposition, you may not need subdivided portals
- The clamped projection technique approximates portal subdivision automatically

**Recommendation:** Start with simple (non-subdivided) inter-cluster portals and convex regions. Only add portal subdivision if profiling shows units making poor macro routing choices.

### Flow Fields vs. Clamped Projection

**Question:** When crossing from one region to another, why use clamped projection instead of a flow field?

**Answer:** 

**Clamped Projection:**
```rust
// Memory: 0 bytes (no storage, pure math)
// Compute: ~10 operations
let portal_crossing = project_target_onto_portal(target, portal.edge);
move_toward(portal_crossing);
```

**Flow Field Alternative:**
```rust
// Memory: ~1KB per portal pair (can be hundreds)
// Compute: ~2 operations (array lookup)
let vector = flow_field[current_region][target_region].sample(position);
move_in_direction(vector);
```

**When to Prefer Clamped Projection:**
- Target position is arbitrary (player clicked anywhere)
- Cluster has many regions (flow field permutations explode)
- Dynamic obstacles (re-baking flow fields is expensive)

**When to Prefer Flow Fields:**
- Common, repeated paths (portals to portals)
- Static map (bake once)
- Open terrain (few permutations)

**Hybrid Strategy:**
```rust
// Use flow fields for inter-cluster movement (predictable portal-to-portal)
// Use clamped projection for intra-cluster movement (arbitrary targets)

if moving_between_clusters {
    use_portal_flow_field(entry_portal, exit_portal);
} else {
    use_clamped_projection(current_region, target_position);
}
```

### Anticipatory Blending Timing

**Question:** When exactly should blending happen?

**Answer:**

**At Cluster Boundaries (Always):**
```rust
if distance_to_cluster_edge(unit.pos) < 3.0 {
    blend_with_next_cluster_vector();
}
```
This eliminates sharp turns when crossing clusters.

**At Region Boundaries (Optional):**
```rust
if distance_to_region_edge(unit.pos) < 1.5 {
    blend_with_next_region_vector();
}
```
This smooths movement between regions, but adds overhead. Only needed if visual quality demands it.

**At Portal Crossings (Recommended for Portal-to-Portal Fields):**
```rust
if approaching_portal && has_portal_flow_field {
    // Look ahead to which portal we'll enter in next cluster
    let future_entry = get_opposite_portal(current_exit_portal);
    let next_exit = routing_table[next_cluster];
    
    // Blend current "to exit" field with next "from entry to exit" field
    blend(current_field, next_field[future_entry][next_exit]);
}
```

**Rule of Thumb:** Blend at every discontinuity in the navigation data. The cost is negligible (~5 operations) compared to the visual quality improvement.

---

## 11. Future Optimizations

### Clamped Projection Refinement (Funnel Algorithm Approximation)

The current clamped projection can be enhanced for smoother movement:

```rust
fn get_steering_point(unit_pos: Vec2, target_pos: Vec2, portal: Edge) -> Vec2 {
    // Take the vector from unit to actual final target
    let direction = (target_pos - unit_pos).normalize();
    
    // Project this line onto the portal edge
    let portal_direction = (portal.end - portal.start).normalize();
    let to_portal = portal.start - unit_pos;
    let distance_along_portal = direction.dot(portal_direction);
    
    // Find the point on the portal that best aligns with our goal
    let projection = portal.start + portal_direction * distance_along_portal;
    
    // Clamp to stay within the portal bounds (plus unit radius for clearance)
    clamp_to_segment(projection, portal.start, portal.end, unit_radius)
}
```

This approximates the Funnel Algorithm without iteration, allowing units to "look through" doorways at their final target.

### Routing Table Caching

For hot paths (common destinations), cache routing table lookups:

```rust
struct RoutingCache {
    cache: HashMap<(ClusterId, ClusterId, IslandId), PortalId>,
}

impl RoutingCache {
    fn get_next_portal(&mut self, current: ClusterId, goal: ClusterId, island: IslandId) -> PortalId {
        let key = (current, goal, island);
        
        if let Some(&portal) = self.cache.get(&key) {
            return portal; // O(1)
        }
        
        // Fall back to routing table BTreeMap lookup
        let portal = self.routing_table[current][(goal, island)]; // O(log n)
        self.cache.insert(key, portal);
        portal
    }
}
```

**Benefits:**
- Popular routes: O(log n) → O(1)
- Automatic cache warming
- Memory: ~100KB for 6,000 hot entries
- Expected speedup: 2-3x for common destinations

---

## 12. Region Fragmentation with Circular Obstacles

### The Problem

**What it is:**
When using axis-aligned rectangle decomposition with circular (or irregular) obstacles, the system creates excessive region fragmentation. A single circular obstacle can generate 10-30 small rectangular regions as the algorithm tries to "trace" the curved boundary with horizontal and vertical strips.

**Why it happens:**
- The maximal rectangles algorithm scans horizontally, creating strips of walkable tiles
- Circular obstacles have concave exterior curves
- Each "step" down a circle's edge creates a new thin rectangle
- This occurs even with large gaps between obstacles - it's the curvature itself that causes fragmentation

**Example:**
```
Map with 2 circular obstacles (radius 5):
- Expected: ~5-10 regions in the cluster
- Actual: 20-40 regions (many thin strips following curves)
- Result: Exceeds MAX_REGIONS or MAX_ISLANDS limits
```

**Why it matters:**
1. **Memory overhead:** More regions → larger local routing tables (`[MAX_REGIONS][MAX_REGIONS]`)
2. **Island fragmentation:** Small regions often form separate islands (tortuosity threshold)
3. **Performance impact:** More region lookups, more portal traversals
4. **Routing table size:** More islands → exponentially larger routing tables
5. **Visual noise:** Debug visualization shows many tiny rectangles instead of clean regions

### Proposed Solutions

#### **Solution 1: Core + Fringe Decomposition** ⭐ Recommended

**Concept:** Separate open areas from obstacle boundaries

**Algorithm:**
1. Compute clearance map (distance from each cell to nearest obstacle)
2. Identify "safe zones" - contiguous areas with clearance ≥ threshold (e.g., 3 tiles)
3. Decompose safe zones into large rectangles (few regions)
4. Decompose remaining boundary areas with current algorithm (many small regions)
5. Optionally: Boundary regions form separate islands for localized routing

**Pros:**
- 90% of cluster area becomes 1-5 large regions
- Small regions only exist near obstacles where needed
- Preserves support for arbitrary obstacle shapes
- Most pathfinding uses simple routing through large regions
- Small regions only matter when entering/exiting tight spaces

**Cons:**
- More complex decomposition algorithm
- Need to implement clearance map (simple flood fill)
- Two different region types to handle

**Implementation complexity:** Medium

**Expected improvement:** 70-90% reduction in region count

---

#### **Solution 2: Dead-End Region Merging**

**Concept:** After rectangle generation, merge regions with limited connectivity

**Algorithm:**
1. Build regions using current algorithm
2. Analyze portal configuration for each region
3. Identify "edge-following" regions:
   - Has portals on ≤ 2 edges (obstacle-bounded on 2-3 sides)
   - AND has high aspect ratio (length/width > 4:1)
   - OR has small area (< threshold)
4. Merge these regions with their largest or most-connected neighbor
5. Accept that merged regions may be slightly non-convex

**Pros:**
- Directly targets the curve-following artifacts
- Uses connectivity information already being computed
- Can reduce region count by 50-80%
- Simple heuristic to implement

**Cons:**
- Merged regions lose strict convexity
- Need relaxed point-in-region test (still fast, just different)
- Merge order matters (need good heuristic)
- Might create slightly suboptimal paths within merged regions

**Implementation complexity:** Low-Medium

**Expected improvement:** 50-80% reduction in region count

---

#### **Solution 3: Aspect Ratio Filtering**

**Concept:** Detect and merge thin strips during decomposition

**Algorithm:**
1. During rectangle generation, compute aspect ratio for each rectangle
2. If `length/width > threshold` (e.g., 5:1) AND region has ≤ 2 portal edges
3. Mark as "detail region" and merge with neighbor immediately
4. Prefer merging into regions with similar orientation

**Pros:**
- Easy to implement (add check during existing algorithm)
- Targets exact problem (long thin regions)
- Low complexity

**Cons:**
- Might catch legitimate corridors (false positives)
- Hard to distinguish "curve follower" from "narrow passage"
- May need careful threshold tuning

**Implementation complexity:** Low

**Expected improvement:** 40-60% reduction in region count

---

#### **Solution 4: Portal Density Scoring**

**Concept:** Score regions by connectivity density, merge high-scoring small regions

**Algorithm:**
1. After decomposition, compute score: `portal_count / region_area`
2. High score = many portals for small area (likely detail region)
3. Low score = dead-end or large important region
4. Merge high-scoring regions below size threshold into neighbors

**Pros:**
- More nuanced than simple portal counting
- Preserves large important regions automatically
- Can be combined with other approaches

**Cons:**
- Requires tuning two thresholds (density and size)
- May not catch all problematic regions

**Implementation complexity:** Low-Medium

**Expected improvement:** 30-50% reduction in region count

---

#### **Solution 5: Restrict Obstacle Shapes**

**Concept:** Enforce rectangular obstacles only

**Options:**
- **Strict:** Only axis-aligned rectangles allowed
- **Moderate:** Axis-aligned shapes (rects + straight walls)
- **Flexible:** Compound rectangles (L/T/U shapes from multiple rects)

**Implementation:**
- Editor snaps to grid, enforces shape constraints
- Optional "complexity budget" - limit obstacles per cluster
- Visual feedback showing invalid placements

**Pros:**
- Perfect clean regions (natural alignment)
- No fragmentation at all
- Very predictable performance
- Still allows complex layouts (via multiple rectangles)

**Cons:**
- Less artistic freedom for level designers
- Can't represent organic/natural obstacles
- Requires editor changes
- Users might try to "cheat" with many small rectangles

**Implementation complexity:** Medium (requires editor changes)

**Expected improvement:** 95%+ reduction (near-perfect regions)

---

#### **Solution 6: Obstacle Dilation for Pathfinding**

**Concept:** Treat obstacles as larger for navigation purposes

**Algorithm:**
1. During decomposition, use dilated obstacles (add 1-2 tile radius)
2. Small gaps between circles become filled
3. Actual collision detection still uses real obstacle bounds
4. Units maintain clearance from obstacles automatically

**Pros:**
- Dramatically reduces fragmentation (gaps disappear)
- Improves movement realism (units need space to maneuver)
- Simple to implement (just modify decomposition input)
- Better performance (fewer regions)

**Cons:**
- Some technically-reachable areas become impassable
- Narrow passages may close completely
- Changes gameplay (can't squeeze through tight gaps)

**Implementation complexity:** Very Low

**Expected improvement:** 60-80% reduction in region count

**Gameplay consideration:** May actually improve realism - units shouldn't squeeze through 1-tile gaps anyway

---

#### **Solution 7: Quadtree/BSP Decomposition**

**Concept:** Recursively split space instead of horizontal scanning

**Algorithm:**
```
function decompose_quadtree(bounds):
    if bounds is mostly walkable:
        return single region
    if bounds is mostly obstacle:
        return empty
    if bounds is mixed:
        split into 4 quadrants
        return regions from all quadrants
```

**Pros:**
- Natural hierarchy (big regions in open space)
- Adapts to obstacle distribution
- Well-studied algorithm

**Cons:**
- Regions are axis-aligned squares (not optimally shaped)
- Different algorithm entirely (major rewrite)
- May still create many small regions near boundaries
- More complex to implement

**Implementation complexity:** High (complete algorithm replacement)

**Expected improvement:** Variable (40-70% depending on obstacle layout)

---

#### **Solution 8: Two-Tier Region System**

**Concept:** Use different region granularity based on context

**Structure:**
- **Macro regions:** Large, possibly non-convex, for strategic routing
- **Micro regions:** Small convex, only used when near obstacles
- Units switch between modes based on environment complexity

**Algorithm:**
1. Create both macro (coarse) and micro (fine) regions
2. Units far from obstacles use macro regions only
3. Units near obstacles switch to micro regions
4. Routing table uses macro regions to save memory

**Pros:**
- Best of both worlds (speed in open areas, precision near obstacles)
- Can tune granularity independently
- Flexible system

**Cons:**
- More complex runtime logic (mode switching)
- Doubled memory (store both region types)
- Need heuristic for "near obstacles"

**Implementation complexity:** High

**Expected improvement:** Effective region count reduced 60-80% for routing

---

#### **Solution 9: Cluster Complexity Budget**

**Concept:** Accept fragmentation but handle it differently

**Algorithm:**
1. If cluster has > threshold regions (e.g., 16), mark as "complex cluster"
2. Complex clusters use different storage:
   - Store region adjacency graph instead of full routing table
   - Use A* within cluster instead of lookup tables
   - Or: Approximate as single region for external routing
3. Most clusters remain simple (fast lookup tables)

**Pros:**
- No algorithm changes to decomposition
- Heterogeneous approach (optimize for common case)
- Graceful degradation (complex areas slower but still work)

**Cons:**
- Inconsistent performance (some clusters fast, some slow)
- More complex data structures
- Need to maintain two code paths

**Implementation complexity:** Medium

**Expected improvement:** No reduction in regions, but mitigates performance impact

---

#### **Solution 10: Relax Convexity Requirement**

**Concept:** Accept non-convex regions, adapt movement logic

**Changes:**
- Allow region merging even if result is L-shaped or U-shaped
- Use proper point-in-polygon test (ray casting) instead of convex test
- Movement: Always go to portal centers (don't assume straight line valid)

**Pros:**
- Solves fragmentation completely
- Can merge aggressively
- Still works for pathfinding (just different movement)

**Cons:**
- Loses "straight line guarantee" within regions
- More complex point-in-polygon test (still fast, ~10 operations)
- Need to change movement logic
- Potential for units taking suboptimal paths within regions

**Implementation complexity:** Medium

**Expected improvement:** 80-95% reduction (aggressive merging)

---

### Recommendation

**Immediate fix:** Solution 6 (Obstacle Dilation) - one line change, big impact
**Best long-term:** Solution 1 (Core + Fringe) - optimal results, preserves flexibility
**Quick improvement:** Solution 2 or 3 (Dead-End Merging or Aspect Ratio) - moderate effort, good results

**Hybrid approach:**
1. Start with obstacle dilation (quick win)
2. Implement aspect ratio filtering (another quick win)
3. If still needed, add core + fringe decomposition (sophisticated solution)

**Decision criteria:**
- **Need perfect results?** → Solution 1 or 5
- **Want quick fix?** → Solution 6 or 3
- **Okay with approximate?** → Solution 9 or 10
- **Limited obstacle shapes acceptable?** → Solution 5

### Current Status

**As implemented:** Basic maximal rectangles algorithm with no fragmentation mitigation
**Observed:** 1990 regions across 400 clusters (avg 5 per cluster), with 144 clusters exceeding MAX_ISLANDS=4
**Impact:** System functional but suboptimal; routing table size acceptable (~13MB) but could be 5-10× smaller with fragmentation fixes
