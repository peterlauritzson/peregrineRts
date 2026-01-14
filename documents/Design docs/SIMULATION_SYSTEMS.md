# Simulation Systems Architecture

## File Structure

**Core Systems**: `src/game/simulation/systems.rs` (260 lines)
- Tick management (global simulation tick counter)
- Input processing (converting player commands to pathfinding requests)
- Path following (hierarchical navigation with flow fields)
- Performance tracking (simulation timing and status logging)

**Spatial Systems**: `src/game/simulation/systems_spatial.rs` (180 lines)
- Spatial hash updates (dynamic entity positioning and swap-based optimization)
- Flow field initialization (map grid setup)
- Obstacle application (dynamic obstacle rasterization to cost field)
- Cluster cache invalidation (pathfinding graph updates for new obstacles)

**Configuration Systems**: `src/game/simulation/systems_config.rs` (110 lines)
- SimConfig initialization from InitialConfig (startup configuration)
- Runtime config hot-reloading from GameConfig asset
- Spatial hash configuration and sizing

## System Execution Order

### Startup Phase
1. `init_sim_config_from_initial` - Load config, set fixed timestep, initialize spatial hash
2. `init_flow_field` - Create flow field grid based on map dimensions

### FixedUpdate Schedule (Deterministic Simulation Tick)
```
SimSet::Input:
  - increment_sim_tick (updates global tick counter)
  - process_input (commands → pathfinding requests)

SimSet::Steering:
  - follow_path (navigate using hierarchical graph + flow fields)
  - apply_boids_steering (flocking behavior)

SimSet::Physics:
  - collision_detection
  - collision_resolution
  - apply_forces

SimSet::Integration:
  - integrate_motion (velocity → position)
  - update_spatial_hash (rebuild spatial partitioning)
```

### Update Schedule (Variable Frame Rate Rendering)
- `apply_new_obstacles` (detects Added<StaticObstacle>, updates flow field)
- `update_sim_from_runtime_config` (hot-reload GameConfig asset changes)
- Visual sync systems (Transform ← SimPosition)

## Key Design Decisions

### System Ownership
**Current Architecture**: The simulation module owns core systems that coordinate between other modules (pathfinding, spatial_hash, structures).

**Alternative Considered**: Each submodule owns its systems (e.g., spatial_hash::update_hash, pathfinding::follow_path).

**Why Current Design**:
- **Clear dependency graph**: simulation → pathfinding → spatial_hash
- **Easier testing**: All simulation logic in one module
- **Bevy convention**: Plugin contains all related systems
- **Determinism control**: Simulation owns the execution order critical for determinism

**Trade-offs**:
- ✅ Centralized coordination for deterministic execution
- ✅ Single source of truth for system ordering
- ❌ simulation module has more responsibilities
- ❌ Changes to spatial_hash may require changes to simulation

### Spatial Hash Update Strategy
The `update_spatial_hash` system uses a **swap-based optimization** to avoid shifting arrays when entities change cells:

```rust
// When entity A moves from cell (2,3) to cell (5,7):
1. Remove A from cell (2,3) by swapping with last entity
2. Insert A into cell (5,7)
3. Update the swapped entity's index (it's now at A's old position)
```

**Why**: Avoids O(N) array shifts on every entity movement. Critical for 10K+ moving units.

### Path Following Modes
Units navigate using one of three path types:

1. **Path::Direct** - Straight line to target (close range)
2. **Path::LocalAStar** - Waypoint list from local A* (within cluster)
3. **Path::Hierarchical** - Portal-based navigation with cached flow fields (cross-cluster)

The system switches between modes dynamically based on path request results from the pathfinding module.

### Input Determinism
`process_input` ensures deterministic command processing:
1. Collect all input events into Vec
2. **Sort by player_id** (deterministic ordering)
3. Execute in sorted order

**Why**: Without sorting, HashMap iteration order could vary, breaking determinism in multiplayer.

### Obstacle Application
`apply_new_obstacles` uses `Added<StaticObstacle>` filter to detect new obstacles:
1. Rasterize obstacle circle to cost field (255 = blocked)
2. Find affected clusters (bounding box in cluster space)
3. Clear cluster flow field caches
4. Regenerate flow fields for affected clusters

**Why immediate regeneration**: Prevents units from navigating through obstacles that were just placed. Alternative (lazy regeneration) would require handling invalid cached flow fields.

## Performance Characteristics

### System Costs (3500 units, 30 TPS)

| System | Time/Tick | Bottleneck |
|--------|-----------|------------|
| increment_sim_tick | ~1µs | None |
| process_input | 10-50µs | Event sorting |
| follow_path | 2-4ms | Flow field lookups, portal queries |
| update_spatial_hash | 0.5-1ms | Swap operations, cell iteration |
| apply_new_obstacles | ~1ms/obstacle | Flow field regeneration |

### Scaling Considerations

**10K units**:
- `follow_path` → 8-12ms (linear with unit count)
- `update_spatial_hash` → 2-3ms (optimized with swap)
- Solution: Temporal LOD (update every N ticks for distant units)

**100K units**:
- Requires multi-threading spatial hash updates
- Batch path following across frames
- Consider GPU compute for integration

## Configuration Hot-Reloading

### Static Config (InitialConfig)
Loaded once at startup from `initial_config.ron`. Contains:
- Map dimensions
- Fixed timestep (tick rate)
- Physics constants
- Spatial hash configuration

**Why static**: Changing these requires rebuilding spatial hash, flow field, and pathfinding graph (expensive).

### Runtime Config (GameConfig)
Hot-reloadable from `game_config.ron`. Contains:
- Camera settings
- Debug visualization toggles
- Input key bindings
- UI preferences

**Why hot-reload**: Enables live tuning without restart, useful for balancing and debugging.

## Future Optimizations

### Parallel Spatial Hash Updates
Currently sequential. Could split map into regions and update in parallel:
```rust
let regions_per_axis = sim_config.spatial_hash_regions_per_axis;
// Divide entities into non-overlapping regions
// Update each region on separate thread
```

**Gain**: 4-8× speedup on multi-core (depends on entity distribution)

### Lazy Path Updates
Currently, all units with `Path` component update every tick. Could track:
- Units that have stopped (velocity ≈ 0)
- Units far from camera
- Units in "idle" state

Skip path following for these, reducing computational load.

**Gain**: 30-50% reduction in path following cost for typical scenes

### Incremental Flow Field Cache
Currently regenerate entire cluster flow field on obstacle add. Could:
- Mark dirty regions instead of full cluster
- Regenerate only affected flow field cells
- Use wavefront propagation from obstacle boundary

**Gain**: 5-10× faster obstacle application in sparse maps

## Testing Strategy

### Determinism Tests
```rust
#[test]
fn test_input_ordering_deterministic() {
    // Spawn 100 units with random commands
    // Run simulation twice with same seed
    // Assert all unit positions match exactly
}
```

### Performance Benchmarks
```rust
#[test]
fn bench_path_following_10k_units() {
    // Spawn 10K units with hierarchical paths
    // Measure tick duration
    // Assert < 16ms target (for 60 FPS deterministic sim)
}
```

### Integration Tests
```rust
#[test]
fn test_obstacle_invalidates_paths() {
    // Spawn units with paths crossing future obstacle position
    // Add obstacle
    // Assert units reroute (request new path or update flow field)
}
```

## Related Documentation

- [ARCHITECTURE.md](../Guidelines/ARCHITECTURE.md) - Sim/Render separation, determinism rules
- [PATHFINDING.md](PATHFINDING.md) - Hierarchical graph, flow fields, path types
- [SPATIAL_PARTITIONING.md](SPATIAL_PARTITIONING.md) - Spatial hash implementation details
