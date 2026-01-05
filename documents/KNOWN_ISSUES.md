# Known Issues & Future Investigations

## Pathfinding Issues

### Issue 1: "No path found" for seemingly simple requests
**Status:** Needs Investigation  
**Priority:** Medium  
**Reported:** 2026-01-05

**Description:**
When playing a loaded map (after generating and saving in editor, then loading in "Play Game" mode), pathfinding sometimes reports "No path found" for what appear to be simple, straightforward path requests.

**Symptoms:**
- Log shows: `warn!("No path found (took {:?})", req_start.elapsed());`
- Occurs with loaded maps in Play Game mode
- May not occur in Editor mode with the same map

**Possible Causes:**
1. **Graph initialization issue**: Graph might not be fully initialized when loaded from file
   - Check if `graph.initialized` is true
   - Check if `graph.clusters` is populated
   - Check if `graph.edges` has connections

2. **Portal connectivity issue**: Loaded graph might have disconnected portals
   - Portals exist but might not have valid edges between them
   - Could be a serialization/deserialization problem

3. **Flow field corruption**: Cost field might not match graph structure
   - Graph expects walkable areas that are marked as obstacles in flow field
   - Desync between `map_data.cost_field` and `map_data.graph`

4. **Bounds checking too strict**: Start/goal validation might be rejecting valid requests
   - Check if world_to_grid is working correctly with loaded map dimensions
   - Flow field origin might be incorrect after loading

**Debug Steps to Take:**
1. Add logging to show:
   - Graph node count on load
   - Graph edge count on load  
   - Sample portal positions
   - Whether start/goal clusters exist in loaded graph

2. Compare behavior:
   - Editor mode (freshly generated map) vs Play Game (loaded map)
   - Same map, different modes

3. Check serialization:
   - Verify saved map file integrity
   - Compare graph before save vs after load

4. Test with simple scenario:
   - Small map (e.g., 5x5 cells)
   - No obstacles
   - Two portals
   - Can pathfinding work at all?

**Workaround:**
Currently none - affects gameplay in Play Game mode.

**Related Code:**
- [src/game/pathfinding.rs](../src/game/pathfinding.rs) - `find_path_hierarchical()`
- [src/game/simulation.rs](../src/game/simulation.rs) - `init_sim_from_initial_config()` (map loading)
- [src/game/map.rs](../src/game/map.rs) - Serialization

---

## Debug Visualization Issues

### Issue 2: F and G keys don't show visualization in Play Game mode
**Status:** ✅ FIXED  
**Priority:** Low (debug feature)  
**Reported:** 2026-01-05  
**Fixed:** 2026-01-05

**Description:**
Flow field (F key) and graph (G key) debug visualizations work in Editor mode but don't display anything in Play Game mode, even after successfully loading a map.

**Root Cause:**
The map save functionality was saving an empty graph (`graph: Default::default()`) with a comment "Will be generated on load". However, the graph building system only runs in `Loading` or `Editor` states, NOT in `InGame` state. So loaded maps had:
- `graph.initialized = true` (marked as ready)
- `graph.nodes.len() = 0` (no portals)
- `graph.clusters.len() = 0` (no clusters)

**Fix Applied:**
1. Changed map save to save the actual built graph: `graph: graph.clone()`
2. Added validation to prevent saving before graph is finalized
3. Enhanced logging to show graph statistics when finalizing and saving
4. Added `Clone` derive to `HierarchicalGraph`

**New Behavior:**
- "Save Map" button now checks if graph is initialized
- If not initialized, shows warning: "Cannot save map - graph not finalized yet!"
- When finalized, shows: "Map finalization COMPLETE! Graph has X portals and Y clusters"
- When saving, shows: "Saving map with X portals and Y clusters"

**How to Use:**
1. Generate map in editor
2. Click "Finalize / Bake Map" and **wait** for completion message
3. Once you see "Map finalization COMPLETE!", click "Save Map"
4. Load the map in Play Game mode - F and G keys now work!

---

## Template for New Issues

### Issue N: [Brief Description]
**Status:** [Needs Investigation / In Progress / Blocked]  
**Priority:** [Critical / High / Medium / Low]  
**Reported:** [Date]

**Description:**
[Detailed description of the issue]

**Symptoms:**
- [Observable behavior]

**Possible Causes:**
1. [Theory 1]
2. [Theory 2]

**Debug Steps to Take:**
1. [Step 1]
2. [Step 2]

**Workaround:**
[If any exists]

**Related Code:**
- [File/function references]
