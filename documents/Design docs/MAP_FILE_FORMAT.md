# Map File Format Design Document

## Goal
To significantly reduce game startup time by precomputing and saving expensive map data (Flow Fields, Hierarchical Graph, etc.) into a file, which can be loaded directly instead of regenerating it every time.

## Overview
The map file will store both the definition of the map (dimensions, obstacles, start positions) and the derived data required for pathfinding and simulation (Cost Field, Hierarchical Graph, Cached Flow Fields).

## File Format
We will use a binary format for efficiency, likely using `bincode` with `serde` for serialization/deserialization of a `MapData` struct.
Extension: `.pmap` (Peregrine Map) or `.bin`.

## Data Structure

The `MapData` struct will contain:

### 1. Metadata / Header
Used to validate if the precomputed data is compatible with the current game version and configuration.
*   `version`: u32 - File format version.
*   `map_width`: FixedNum - World width.
*   `map_height`: FixedNum - World height.
*   `cell_size`: FixedNum - Size of a flow field cell (must match `CELL_SIZE`).
*   `cluster_size`: usize - Size of a cluster in cells (must match `CLUSTER_SIZE`).
*   `timestamp`: u64 - Creation time (optional).

### 2. Level Definition (Source Data)
Data used to generate the map. Useful for editing or regenerating if constants change.
*   `obstacles`: Vec<MapObstacle>
    *   `position`: FixedVec2
    *   `radius`: FixedNum
*   `start_locations`: Vec<StartLocation> (Future proofing)
    *   `player_id`: u8
    *   `position`: FixedVec2

### 3. Precomputed Data (Derived)
The expensive data.
*   `cost_field`: Vec<u8> - The grid of walkability (1 = walkable, 255 = obstacle).
    *   Dimensions are derived from `map_width / cell_size` and `map_height / cell_size`.
*   `graph`: HierarchicalGraphData
    *   `nodes`: Vec<PortalData>
    *   `edges`: Map<PortalId, Vec<(TargetPortalId, Cost)>>
    *   `clusters`: Map<(ClusterX, ClusterY), ClusterData>

#### ClusterData
*   `portals`: Vec<PortalId> - Portals belonging to this cluster.
*   `flow_field_cache`: Map<PortalId, LocalFlowFieldData>
    *   `width`: usize
    *   `height`: usize
    *   `vectors`: Vec<FixedVec2> - The precomputed direction vectors for this portal within the cluster.

## Loading Process

1.  **Startup**:
    *   The game starts. `GameConfig` is loaded asynchronously.
    *   Map generation/loading is deferred until `GameConfig` is ready.

2.  **Map Initialization**:
    *   System checks if a map file is specified (e.g., in `GameConfig` or command line arguments) or defaults to a specific map.
    *   **Attempt Load**: Try to read the map file.
        *   **Validation**: Check if `cell_size` and `cluster_size` in the file match the current game constants. Check if `map_width`/`map_height` match the requested configuration (or update the simulation config to match the map).
        *   **Success**: Deserialize `MapData`. Populate `MapFlowField` and `HierarchicalGraph` resources directly.
        *   **Failure (Mismatch/Corrupt/Missing)**: Fallback to generation.
            *   Use the `obstacles` from the map file (if readable) or default hardcoded obstacles.
            *   Generate `CostField`.
            *   Build `HierarchicalGraph` (expensive).
            *   (Optional) Save the generated map to file for next time.

## Integration Steps

1.  **Define `MapData` Structs**: Create serializable structs in a new module (e.g., `src/game/map.rs`).
2.  **Serialization**: Implement `save_map` function using `bincode`.
3.  **Deserialization**: Implement `load_map` function.
4.  **Refactor `setup_game`**: Remove hardcoded obstacles. Instead, trigger a "Load Map" state or event.
5.  **Refactor `update_sim_from_config`**:
    *   Currently, it initializes `MapFlowField` and resets `Graph`.
    *   It should instead trigger the Map Loading process.
    *   If loading from file, skip the `FlowField::new` and `graph.reset()`/`build_graph` steps.
6.  **Map Resource**: Introduce a `CurrentMap` resource to track map state.

## Considerations

*   **FixedPoint Arithmetic**: Ensure `FixedNum` and `FixedVec2` are serializable (they likely are if they derive `Serialize`/`Deserialize`).
*   **File Size**: `vectors` in `LocalFlowField` can be large.
    *   Size = `Width * Height * VectorSize`.
    *   Cluster = 25x25 cells.
    *   Vector = 2 * 64-bit (or 32-bit) fixed point.
    *   If many portals, this grows.
    *   Compression (e.g., `flate2`) might be needed for the file.
*   **Versioning**: If we change `FixedNum` representation or `HierarchicalGraph` logic, we need to invalidate old maps.

## Map Editor & Saving

To facilitate map creation and updates, we will implement a basic in-game editor or developer tools.

### Saving Process
1.  **Trigger**: A developer command (e.g., key press `F5` or console command) triggers the save.
2.  **Gather Data**:
    *   Collect current `SimConfig` (width, height).
    *   Collect all `StaticObstacle` entities (position, radius).
    *   Collect `StartLocation` entities (if any).
    *   Collect the current `MapFlowField` (cost field).
    *   Collect the current `HierarchicalGraph` (nodes, edges, clusters, cached flow fields).
3.  **Serialize**: Create a `MapData` struct with this information.
4.  **Write**: Serialize to `.pmap` file using `bincode` + `flate2` (Zlib).

### Editor Features (Planned)
*   **Obstacle Placement**: Mouse click to place/remove obstacles.
*   **Re-bake**: A button/key to regenerate the Flow Field and Hierarchical Graph based on current obstacles. This is required before saving to ensure the precomputed data matches the visual obstacles.
*   **Validation**: Ensure the map is in a valid state (e.g., start locations are reachable) before saving.

### Workflow
1.  Start game (loads default map or generates empty one).
2.  Use debug keys/editor to place obstacles.
3.  Press "Re-bake" (runs `build_graph` and updates flow field).
4.  Press "Save" (writes to `assets/maps/default.pmap`).
5.  Restart game -> Fast load from the new map file.

