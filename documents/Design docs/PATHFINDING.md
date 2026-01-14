# Peregrine Pathfinding Architecture: Hierarchical Pathfinding (HPA*)

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
To support **10 million units** on **large maps (2048x2048+)** with **dynamic obstacles** (building placement), we cannot use standard A* (too slow) or pure Flow Fields (too much memory for unique targets).

The solution is **Hierarchical Pathfinding A* (HPA*)**. This approach divides the map into local clusters and builds a high-level graph for long-distance navigation.

### Key Features
*   **Scalability:** Pathfinding cost depends on the number of *clusters*, not the number of *tiles*.
*   **Dynamic Updates:** Placing a building only triggers a re-calculation for the specific cluster it touches, not the whole map.
*   **Memory Efficient:** We store one abstract graph, and cache small *local* flow fields only for active clusters.
*   **Robust Movement:** Units use **Local Flow Fields** to navigate within a cluster. This makes them immune to getting "stuck" after collisions, as the map itself guides them back to the path.

---

## 2. Architecture & Data Structures

### Level 0: The Grid (The "Territory")
*   **Existing:** `FlowField` / `CostField`.
*   **Data:** A dense array of `u8` (1 = Walkable, 255 = Obstacle).
*   **Usage:** Used for final collision checks and local steering.

### Level 1: The Clusters (The "Counties")
*   **Concept:** The grid is divided into fixed-size square chunks (e.g., 16x16 tiles).
*   **Structure:**
    ```rust
    struct Cluster {
        id: (usize, usize), // Grid coordinates (e.g., 2, 4)
        portals: Vec<PortalId>,
        // Cache of distances between portals inside this cluster
        intra_cluster_costs: HashMap<(PortalId, PortalId), FixedNum>,
        // NEW: Cache of flow fields for specific exit portals
        flow_field_cache: HashMap<PortalId, LocalFlowField>,
    }
    ```

### Level 2: The Abstract Graph (The "Highway Network")
*   **Nodes (Portals):** A `Portal` is a transition point between two adjacent clusters.
    *   *Definition:* A Portal is a **range of tiles** (an edge segment), not a single point. This allows units to use the full width of a hallway.
*   **Edges:**
    1.  **Inter-Cluster:** Connection between two portals that share a border (Cost ~ 1.0).
    2.  **Intra-Cluster:** Connection between two portals *inside* the same cluster (Cost = calculated via local A*).
*   **Structure:**
    ```rust
    struct AbstractGraph {
        nodes: Vec<Portal>,
        edges: HashMap<PortalId, Vec<(PortalId, FixedNum)>>, // Adjacency list
    }
    ```

---

## 3. The Workflow

### A. Path Request
1.  **Unit** wants to go from `Start` to `Goal`.
2.  **System** identifies `StartCluster` and `GoalCluster`.
3.  **System** adds temporary nodes to the Abstract Graph: `StartNode` and `GoalNode`.
4.  **System** connects `StartNode` to all Portals in `StartCluster`, and `GoalNode` to all Portals in `GoalCluster`.

### B. High-Level Search
1.  Run **A*** on the **Abstract Graph**.
2.  **Result:** A list of Portals (e.g., `Start -> Portal A -> Portal B -> Goal`).

### C. Movement & Refinement (Hybrid Approach)
1.  **High-Level:** The Unit receives the list of Portals as "Waypoints" (e.g., `Portal A -> Portal B`).
2.  **Low-Level (Local Flow Fields):**
    *   The unit identifies which Cluster it is currently in.
    *   It requests a **Local Flow Field** for its immediate target (`Portal A`).
    *   If the field is not cached, the System generates it (using the *entire edge* of Portal A as the target, not just the center).
3.  **Physics:**
    *   The unit reads the vector from the tile under its feet.
    *   This vector is combined with Boids/Separation forces.
    *   **Robustness:** If a unit is pushed by a collision, it simply lands on a new tile and follows the new vector. No re-pathing required.

### D. Dynamic Updates (Building Placement)
1.  User places a wall in `Cluster (2, 2)`.
2.  **System** marks `Cluster (2, 2)` as "Dirty".
3.  **Update Step:**
    *   Re-scan the borders of `Cluster (2, 2)`. Did a Portal get blocked? -> Remove it.
    *   Re-calculate paths between remaining Portals inside `Cluster (2, 2)`.
    *   Update the `AbstractGraph` edges.
4.  **Result:** The global navigation mesh is updated instantly.

---

## 4. Implementation Roadmap

We will build this iteratively to ensure the game remains playable at every step.

### Phase 1: The Interface & Baseline (Standard A*)
*Goal: Decouple movement logic from pathfinding logic.*
1.  Define `PathRequest` Event and `Path` Component.
2.  Implement a **Standard A*** system that runs on the raw grid.
3.  Update `Unit` code to follow a list of waypoints instead of a single target.
    *   *Result:* Units can navigate complex mazes (slowly, but correctly).

### Phase 2: The Abstract Graph (Static)
*Goal: Implement the "Google Maps" layer.*
1.  Define `Cluster` size (e.g., 10x10 for now).
2.  Implement `GraphGenerator`:
    *   Iterate clusters.
    *   Create Portals (centers of edges).
    *   Connect Portals.
3.  Implement `HierarchicalPathfinder`:
    *   Replaces the Standard A* from Phase 1.
    *   Runs A* on the Graph nodes.
    *   Returns the list of Portal positions.

### Phase 3: Local Flow Fields (The "Last Mile")
*Goal: Robust movement and collision recovery.*
1.  Implement `LocalFlowField` struct (small grid, e.g., 10x10).
2.  Implement `FlowFieldCache` in the `Cluster` struct.
3.  Update Unit Movement Logic:
    *   Instead of `seek(target_pos)`, use `get_flow_vector(current_pos)`.
    *   Generate flow fields on demand when a unit requests a portal that isn't cached.

### Phase 4: Dynamic Updates
*Goal: Handle map changes.*
1.  Listen for `ObstacleAdded` events.
2.  Identify affected Clusters.
3.  **Invalidate Cache:** Clear the `flow_field_cache` for that cluster.
4.  Trigger `GraphGenerator::update_cluster(cluster_id)`.
5.  Verify that units re-path if their current path is invalidated.

### Phase 5: Optimization & Polish
*Goal: Smooth movement and performance.*
1.  **Portal Refinement:** Instead of "Center of Edge", find the actual walkable gaps.
2.  **Path Smoothing:** Post-process the path to remove "zig-zags" between portals.
3.  **Async Pathfinding:** Ensure the game doesn't stutter if 100 units request paths at once (Time-slicing).
---

## 5. Future Optimizations: Group Leadership & Shared Pathfinding

### The Problem
With thousands or millions of units, pathfinding every unit individually becomes expensive even with hierarchical algorithms. In typical RTS scenarios, large groups of units often move together toward the same destination, creating redundant pathfinding work.

### Solution: Leader-Based Pathfinding
Instead of computing paths for every unit, designate **leaders** and have nearby units follow them.

#### Implementation Strategy

**1. Spatial Clustering**
- Divide units into spatial groups (can reuse existing spatial hash)
- Groups are dynamic - units can switch groups based on proximity and destination
- Each group size: ~10-50 units depending on density

**2. Leader Selection**
- Per group, select one leader (e.g., first unit to request path, or unit closest to center)
- Leader gets full pathfinding (hierarchical A* through portal graph)
- Followers skip pathfinding and use leader's path as guidance

**3. Formation Following**
- Followers use **leader's waypoints** as general direction
- Local steering (boids + obstacle avoidance) handles formation naturally
- No rigid formation logic needed - emergent behavior from local forces

**4. Dynamic Re-assignment**
- If leader dies/disappears, promote new leader from followers
- Units can leave group if they diverge significantly from leader's path
- Groups can merge/split based on spatial proximity and goal similarity

#### Benefits
- **Reduced pathfinding load:** 90-95% reduction for grouped units (1 path per 10-50 units)
- **Emergent formations:** Natural column/spread based on terrain and boids
- **Scalability:** Enables 100K+ units with reasonable pathfinding costs
- **Realism:** Mimics real-world behavior where units follow squad leaders

#### Example Workload Reduction
- **Before:** 10,000 units × 0.1ms = 1000ms per frame (freeze)
- **After (20 units/group):** 500 leaders × 0.1ms = 50ms per frame (acceptable)

#### Integration Points
- Works with existing hierarchical pathfinding (leaders use portal graph)
- Works with existing flow fields (followers still use local steering)
- Works with existing spatial hash (for group formation)
- Minimal changes to unit movement systems (followers just have different target source)

---

## 6. Performance Optimizations (Jan 2026)

### Cluster Routing Table (Implemented)
**Problem:** Inter-cluster pathfinding was using A* on portal graph for every path request, causing bottlenecks with 10k+ units.

**Solution:** Precomputed cluster-to-cluster routing table
- Built during graph initialization using Dijkstra from each cluster
- Stores optimal first portal for every cluster pair: `routing_table[start_cluster][goal_cluster] = next_portal_id`
- Memory: ~90-270MB for 2048×2048 map (6,724 clusters)
- Eliminates A* search between clusters entirely (O(P log P) → O(1))

**Impact:**
- Before: 100-1000 iterations of portal graph A* per path request
- After: No pathfinding computation needed - routing table built at load time

### Lazy Routing Table Walk (Implemented)
**Key Insight:** Since we precompute ALL cluster-to-cluster paths in the routing table, there's no need to store a portal list on each entity. We can look up the next portal on-demand whenever a unit enters a new cluster.

**Path Component (Minimal):**
```rust
Path::Hierarchical {
    goal: FixedVec2,  // Final destination only
    goal_cluster: (usize, usize),  // Cached for routing table lookups
}
```

**Movement System Logic:**
```rust
// When unit enters new cluster:
let current_cluster = get_cluster_from_position(unit_pos);

if current_cluster == goal_cluster {
    // In final cluster - use flow field to goal position
    navigate_to_goal_via_flow_field(goal);
} else {
    // Lookup next portal from routing table
    let next_portal_id = routing_table[current_cluster][goal_cluster];
    // Navigate to that portal using cluster's cached flow field
    navigate_to_portal_via_flow_field(next_portal_id);
}
```

**Benefits:**
- **Zero path computation:** Path requests just set goal on Path component
- **Zero allocations:** No Vec<usize> for portal lists (eliminated 240MB+ for 10M units)
- **Automatic path sharing:** All units to same destination use identical routing table lookups
- **Memory:** 24 bytes per entity (goal + goal_cluster) vs 100+ bytes with portal lists
- **Perfect batching:** 100k requests = 100k goal writes, no redundant pathfinding

**Performance:**
- Before (portal list approach): 19ms for 100k requests (Vec allocations + routing table walks)
- After (lazy lookup): <1ms for 100k requests (just write goal positions)
- Routing table lookup per cluster transition: O(log n) ≈ 1-2ns

**⚠️ KNOWN LIMITATION: Unreachable Targets**

**Current Behavior (Undefined):**
If `routing_table[current_cluster][goal_cluster]` has no entry (clusters not connected), the movement system has undefined behavior. Units may:
- Get stuck with no path
- Panic if routing table lookup fails
- Wander aimlessly
- Continue trying to path forever

**Why This Happens:**
- Goal is on an island cut off by obstacles
- Goal is inside an unwalkable obstacle
- Map has disconnected regions (e.g., separate landmasses)
- Dynamic obstacles created unreachable zones

**Potential Solutions (Not Yet Implemented):**

1. **Fail Gracefully (Simplest)**
   - Return `None` from routing table lookup if no path exists
   - Unit stops moving and clears Path component
   - Pro: Simple, no unexpected behavior
   - Con: Units give up rather than trying alternatives

2. **ConnectedComponents Validation (Pre-check)**
   - Use existing `ConnectedComponents` resource in `process_path_requests`
   - Check if start and goal clusters are in same component before setting Path
   - If unreachable, don't set Path component at all
   - Pro: Fails fast, prevents wasted movement
   - Con: Adds O(log n) lookup per path request

3. **"Get As Close As Possible" Fallback**
   - Use `ConnectedComponents::closest_cross_component` to find nearest reachable portal
   - Redirect goal to closest point we CAN reach
   - Pro: Units make progress toward unreachable goals
   - Con: More complex, may confuse players when units stop short

4. **Lazy Validation (On Movement)**
   - Allow Path to be set even if unreachable
   - Movement system detects missing routing table entry at cluster transition
   - Fallback to direct navigation or stop with error message
   - Pro: Zero cost until problem actually encountered
   - Con: Units waste movement before discovering unreachability

**Recommendation:**
Start with **#1 (Fail Gracefully)** for robustness, then add **#2 (ConnectedComponents Pre-check)** if unreachable targets become a frequent issue. Option #3 could be added later for better UX in scenarios with dynamic obstacles.

**TODO:**
- [ ] Implement graceful failure when routing table has no entry
- [ ] Add ConnectedComponents validation in process_path_requests (optional)
- [ ] Test behavior with disconnected map regions
- [ ] Consider player feedback for unreachable targets (UI indicator?)

### Flow Field Navigation (Implemented)
All cluster-to-portal navigation uses precomputed flow fields:
- Flow fields generated during graph build for every portal in every cluster
- Movement system queries flow field at unit's grid position
- O(1) vector lookup vs O(n log n) local A*
- Fully robust to collisions and local obstacles

### Future Optimization: Cluster-to-Cluster Cache
**Problem:** Routing table uses BTreeMap lookups (O(log n)). For 10M units, even fast lookups add up.

**Solution:** Cache hot routing table entries in HashMap
```rust
struct ClusterCache {
    routing_cache: HashMap<(usize, usize), PortalId>,  // (current, goal) -> portal
}

// Check cache first (O(1) average)
if let Some(&portal_id) = cache.get(&(current_cluster, goal_cluster)) {
    return portal_id;
}
// Fall back to routing table BTreeMap
let portal_id = routing_table[current_cluster][goal_cluster];
cache.insert((current_cluster, goal_cluster), portal_id);
```

**Benefits:**
- Popular routes cached: O(log n) → O(1)
- Automatic cache warming for common destinations
- Memory: ~100KB for 6,000 hot entries
- Expected speedup: 2-3x for cluster transition lookups
