# Active Pathfinding Issues
**Last Updated:** January 10, 2026  
**Project:** Peregrine RTS - Pathfinding system bugs and investigations

---

## Issue 1: "No path found" for seemingly simple requests
**Status:** Needs Investigation  
**Priority:** Medium  
**Reported:** January 5, 2026

### Description
When playing a loaded map (after generating and saving in editor, then loading in "Play Game" mode), pathfinding sometimes reports "No path found" for what appear to be simple, straightforward path requests.

### Symptoms
- Log shows: `warn!("No path found (took {:?})", req_start.elapsed());`
- Occurs with loaded maps in Play Game mode
- May not occur in Editor mode with the same map

### Possible Causes

#### 1. Graph initialization issue
Graph might not be fully initialized when loaded from file
- Check if `graph.initialized` is true
- Check if `graph.clusters` is populated
- Check if `graph.edges` has connections

#### 2. Portal connectivity issue
Loaded graph might have disconnected portals
- Portals exist but might not have valid edges between them
- Could be a serialization/deserialization problem

#### 3. Flow field corruption
Cost field might not match graph structure
- Graph expects walkable areas that are marked as obstacles in flow field
- Desync between `map_data.cost_field` and `map_data.graph`

#### 4. Bounds checking too strict
Start/goal validation might be rejecting valid requests
- Check if world_to_grid is working correctly with loaded map dimensions
- Flow field origin might be incorrect after loading

### Debug Steps to Take

1. **Add logging to show:**
   - Graph node count on load
   - Graph edge count on load  
   - Sample portal positions
   - Whether start/goal clusters exist in loaded graph

2. **Compare behavior:**
   - Editor mode (freshly generated map) vs Play Game (loaded map)
   - Same map, different modes

3. **Check serialization:**
   - Verify saved map file integrity
   - Compare graph before save vs after load

4. **Test with simple scenario:**
   - Small map (e.g., 5x5 cells)
   - No obstacles
   - Two portals
   - Can pathfinding work at all?

### Workaround
Currently none - affects gameplay in Play Game mode.

### Related Code
- [src/game/pathfinding.rs](../../src/game/pathfinding.rs) - `find_path_hierarchical()`
- [src/game/simulation.rs](../../src/game/simulation.rs) - `init_sim_from_initial_config()` (map loading)
- [src/game/map.rs](../../src/game/map.rs) - Serialization

---

## Performance Issue: Path Request Queue Buildup

**Status:** Active - Needs Monitoring  
**Priority:** Medium  
**Reported:** January 9, 2026

### Description
Path request queue accumulating pending requests, indicating pathfinding system may not be keeping up with demand.

### Current Metrics
- Active paths: 1,678-3,315 units following paths
- Path request queue: **1,874 pending requests** (WARNING)
- Follow path system: 471µs for 3,315 paths (~0.14µs per path)

### Issues
- Path request queue building up (1,874 pending)
- May cause pathfinding system to lag behind demand
- Not directly causing tick slowdown but could cascade into larger problem

### Optimization Opportunities
- [ ] Increase pathfinding processing capacity
- [ ] Batch pathfinding requests
- [ ] Implement path request prioritization
- [ ] Consider async pathfinding on separate thread
- [ ] Cache common paths
- [ ] Reduce path recalculation frequency

### Related Code
- [src/game/pathfinding/systems.rs](../../src/game/pathfinding/systems.rs)

---

## Fixed Issues

### ✅ Issue 2: F and G keys don't show visualization in Play Game mode
**Status:** FIXED  
**Priority:** Low (debug feature)  
**Reported:** January 5, 2026  
**Fixed:** January 5, 2026

**Description:**
Flow field (F key) and graph (G key) debug visualizations worked in Editor mode but didn't display anything in Play Game mode.

**Root Cause:**
The map save functionality was saving an empty graph (`graph: Default::default()`) with a comment "Will be generated on load". However, the graph building system only runs in `Loading` or `Editor` states, NOT in `InGame` state.

**Fix Applied:**
1. Changed map save to save the actual built graph: `graph: graph.clone()`
2. Added validation to prevent saving before graph is finalized
3. Enhanced logging to show graph statistics when finalizing and saving
4. Added `Clone` derive to `HierarchicalGraph`

**How to Use:**
1. Generate map in editor
2. Click "Finalize / Bake Map" and **wait** for completion message
3. Once you see "Map finalization COMPLETE!", click "Save Map"
4. Load the map in Play Game mode - F and G keys now work!
