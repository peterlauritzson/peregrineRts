# Active Map Editor Issues
**Last Updated:** January 10, 2026  
**Project:** Peregrine RTS - Map editor bugs and improvements

---

## ✅ FIXED: Critical Bug - Map Resize Crash
**Status:** FIXED  
**Priority:** Critical  
**Reported:** January 5, 2026  
**Fixed:** January 5, 2026

### What Happened
During stress testing with a larger map (450x450 vs 300x300), the game crashed with a panic:

```
thread 'Compute Task Pool (6)' panicked at src\game\pathfinding.rs:463:33:
no entry found for key
```

**Timeline:**
- Game starts with 300x300 map loaded from `assets/maps/default.pmap`
- User generates new 450x450 map with 75 obstacles via Map Editor
- Obstacles begin applying to flow field
- **PANIC** during obstacle application

### Root Cause

**The Bug:** Array index out of bounds in flow field access

**Location:** [pathfinding.rs:463](../../src/game/pathfinding.rs#L463)
```rust
if map_flow_field.cost_field[map_flow_field.get_index(gx, gy)] == 255 {
```

**Why it happened:**

1. **Map Generation Process:**
   - Editor clears old obstacles (50 items)
   - Creates new FlowField (450x450)
   - Updates SimConfig map size
   - Updates SpatialHash
   - Resets HierarchicalGraph (`graph.reset()`)
   - Spawns 75 new obstacles

2. **The Problem:**
   - `graph.reset()` only **clears** the graph (makes it empty)
   - It does NOT **rebuild** the graph for the new map size
   - When obstacles are applied, `apply_new_obstacles` calls `regenerate_cluster_flow_fields`
   - This function tries to regenerate flow fields for affected clusters
   - BUT the clusters and portals don't exist yet (graph not initialized)
   - OR worse: old portals from the 300x300 map still exist in memory
   - Portal coordinates (e.g., x=320) exceed new flow field bounds
   - Array access panics: `cost_field[320 * 450 + y]` is out of bounds

3. **The Flow:**
```
Map Editor: Generate 450x450 map
  ↓
Clear obstacles, Create flow field, Reset graph
  ↓
Spawn obstacle entities
  ↓
[Next Frame] apply_new_obstacles system runs
  ↓
Tries to regenerate_cluster_flow_fields for obstacle
  ↓
Graph is empty or has stale portals
  ↓
generate_local_flow_field accesses flow_field with invalid coords
  ↓
💥 PANIC: index out of bounds
```

### The Fix

**Two-Part Solution:**

#### 1. Graph Rebuild Timing
The hierarchical graph MUST be fully rebuilt BEFORE obstacles are added, not after.

**Current Flow (Broken):**
```
1. Create FlowField
2. Reset Graph (clear)
3. Add Obstacles → Try to regenerate clusters → PANIC
4. (Never reaches) Build Graph
```

**Fixed Flow:**
```
1. Create FlowField
2. Reset Graph (clear)
3. Build Graph (initialize clusters & portals for new map size)
4. Add Obstacles → Regenerate clusters → Success ✓
```

#### 2. Bounds Checking (Defense-in-Depth)
Even with correct ordering, add bounds checking in `generate_local_flow_field`:

```rust
// Before accessing flow field
let gx = min_x + nx;
let gy = min_y + ny;

// ADD BOUNDS CHECK
if gx >= map_flow_field.width || gy >= map_flow_field.height {
    continue; // Skip out-of-bounds cells
}

// Now safe to access
if map_flow_field.cost_field[map_flow_field.get_index(gx, gy)] == 255 {
    continue;
}
```

### Implementation Details

**File:** [editor.rs](../../src/game/editor.rs) (Map generation function)

**Change:** After creating the new flow field and before spawning obstacles:

```rust
// After this line:
info!("FlowField created successfully in {:?} (total cells: {})", duration, total_cells);

// ADD THIS:
info!("Building hierarchical graph for new map...");
let graph_start = std::time::Instant::now();
crate::game::pathfinding::build_graph(&mut graph, &flow_field);
graph.initialized = true;
info!("Graph built in {:?} - {} clusters, {} portals", 
    graph_start.elapsed(), 
    graph.clusters.len(), 
    graph.nodes.len()
);

// Then spawn obstacles...
```

**File:** [pathfinding.rs](../../src/game/pathfinding.rs#L463)

**Change:** Add bounds checking before flow field access:

```rust
for (nx, ny) in neighbors {
    if nx >= width || ny >= height { continue; }
    
    let gx = min_x + nx;
    let gy = min_y + ny;
    
    // ADD BOUNDS CHECK HERE
    if gx >= map_flow_field.width || gy >= map_flow_field.height {
        continue; // Portal extends beyond map bounds - skip
    }
    
    // Check global obstacle
    if map_flow_field.cost_field[map_flow_field.get_index(gx, gy)] == 255 {
        continue;
    }
    // ... rest of code
}
```

### Performance Observations

**Before crash (300x300 map, 0 units):**
- Sim tick duration: ~40-80μs
- Completely stable

**Map Generation (450x450):**
- FlowField creation: 1.9ms for 202,500 cells (acceptable)
- Map generation total: 5.58ms (excellent)

**After resize (before crash):**
- Sim tick duration jumped to 1.7-1.8ms (20x slower!)
- This is BEFORE units were even spawned
- Suggests the empty graph or pending obstacle application was causing overhead

**Conclusion:** The performance degradation suggests the simulation was already struggling with the incorrect state (empty/stale graph + pending obstacles) before the crash occurred.

### Testing Requirements

After implementing the fix:

1. **Resize Test:** Generate maps of varying sizes (100x100, 300x300, 500x500, 1000x1000)
2. **Obstacle Density Test:** Generate maps with varying obstacle counts (0, 50, 100, 500)
3. **Rapid Regeneration:** Generate multiple maps in quick succession
4. **Stress Test:** Generate large map (1000x1000) with many obstacles (500+)

All tests should complete without panic and maintain stable tick times.

### Lessons Learned

1. **Initialization Order Matters:** Resources that depend on each other (FlowField ← Graph ← Obstacles) must be initialized in the correct sequence.

2. **Reset ≠ Rebuild:** Clearing a data structure is not the same as rebuilding it for new constraints.

3. **Defense in Depth:** Even with correct logic, bounds checking prevents catastrophic failures.

4. **Deferred Execution Risks:** Bevy's ECS defers entity spawning and component addition. Systems processing `Added<T>` components run in the next frame, creating timing hazards if other resources are modified in between.

5. **Test Edge Cases:** Map resizing is an edge case that exposed a critical initialization ordering bug. More edge case testing needed (min/max sizes, empty maps, etc.).

---

## Related Documentation
- [SPATIAL_PARTITIONING.md](../Design%20docs/SPATIAL_PARTITIONING.md) - Spatial hash architecture
- [PATHFINDING.md](../Design%20docs/PATHFINDING.md) - Pathfinding system design
