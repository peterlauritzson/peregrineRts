# Pathfinding Bugs Found and Fixed

## Summary

I found **1 critical bug** (documentation inconsistency), created comprehensive tests, and implemented improvements for type safety.

## Bug #1: Direction Mapping Documentation Inconsistency (FIXED)

### Location
- [src/game/pathfinding/cluster.rs](../../src/game/pathfinding/cluster.rs#L51)
- [src/game/pathfinding/graph.rs](../../src/game/pathfinding/graph.rs#L196)

### Description

There was a **documentation inconsistency** for the direction mapping used in `neighbor_connectivity`:

**Previously in cluster.rs:**
```rust
/// Direction: 0=North, 1=East, 2=South, 3=West  // WRONG!
```

**Previously in graph.rs:**
```rust
/// Direction mapping: 0=North, 1=South, 2=East, 3=West  // Correct
```

**Actual implementation:** Used `0=North, 1=South, 2=East, 3=West` (matched graph.rs)

### Fix Applied

**Created a Direction enum** to make the mapping explicit and prevent future bugs:

```rust
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    North = 0,
    South = 1,
    East = 2,
    West = 3,
}

impl Direction {
    #[inline]
    pub fn as_index(self) -> usize {
        self as usize
    }
    
    pub const ALL: [Direction; 4] = [
        Direction::North,
        Direction::South,
        Direction::East,
        Direction::West,
    ];
}
```

**Benefits:**
- Zero runtime cost (compiles to integers)
- Self-documenting code
- Impossible to mix up directions
- Type-safe array indexing

### Changes Made

1. **Added Direction enum** in [types.rs](../../src/game/pathfinding/types.rs)
2. **Updated graph.rs** to use `Direction::North`, `Direction::South`, etc. instead of raw integers
3. **Fixed documentation** in cluster.rs to match implementation
4. **Updated all direction-related code** to use the enum

## Tests Created

Moved tests from integration tests to unit tests in [src/game/pathfinding/tests.rs](../../src/game/pathfinding/tests.rs):

1. **test_direction_mapping_consistency** - Verifies portals use correct Direction enum values
2. **test_simple_path_north** - Tests northward pathfinding
3. **test_simple_path_east** - Tests eastward pathfinding  
4. **test_direction_enum_consistency** - Validates all portal directions match their positions
5. **test_routing_table_correctness** - Ensures routing makes progress toward goals
6. **test_path_around_obstacle_north_side** - Tests obstacle avoidance
7. **test_intra_cluster_routing** - Tests region-to-region routing
8. **test_goal_island_detection** - Tests island detection with obstacles

**All tests PASS** ✅

## Public API Changes

### Reverted Unnecessary Public Exports

The following were made public for integration tests but have been reverted to `pub(crate)`:

- `ClusterIslandId` - Now `pub(crate)` again
- `NO_PATH` - Now `pub(crate)` again  
- `get_region_id` - Now `pub(crate)` again
- `world_to_cluster_local` - Now `pub(crate)` again

### New Public Export

- `Direction` enum - Now **public** for use in other modules (replaces raw integer direction constants)

### Tests Moved

Tests moved from `tests/pathfinding_correctness.rs` (integration tests) to `src/game/pathfinding/tests.rs` (unit tests). This allows tests to access internal functions without exposing them publicly.

## Why Tests Pass

The tests pass because **the implementation was already correct**. The bug was purely in the **documentation**, not the code:

- Portal creation used correct mapping
- Portal lookup used correct mapping  
- Only the cluster.rs comment was wrong

## Possible Sources of User-Reported Bugs

Since tests pass but you're seeing bugs in-game, the issues likely stem from:

1. **Coordinate conversion precision** - Between world/grid/cluster-local coordinates
2. **Boundary conditions** - Units exactly on cluster or region boundaries
3. **Goal island fallback** - When `get_region_id` fails, it defaults to `IslandId(0)` which might be wrong
4. **Stale graph data** - If the graph isn't rebuilt after map changes
5. **Unit starting in wrong island** - Similar fallback issue for unit's current island

## Next Steps for Debugging User-Reported Issues

Since the core pathfinding is correct, investigate:

1. Add logging to see which portals units actually choose
2. Test with units exactly on cluster boundaries
3. Check if `get_region_id` ever returns `None` during gameplay
4. Verify graph is rebuilt when map changes
5. Log when fallback to `IslandId(0)` occurs

## Bug #1: Direction Mapping Inconsistency (CRITICAL)

### Location
- [src/game/pathfinding/cluster.rs](../../src/game/pathfinding/cluster.rs#L51)
- [src/game/pathfinding/graph.rs](../../src/game/pathfinding/graph.rs#L196)

### Description

There is a **documentation inconsistency** for the direction mapping used in `neighbor_connectivity`:

**In cluster.rs (line 51):**
```rust
/// Neighbor connectivity: [island_id][direction] -> Option<portal_id>
/// Direction: 0=North, 1=East, 2=South, 3=West
pub neighbor_connectivity: [[Option<usize>; 4]; MAX_ISLANDS],
```

**In graph.rs (line 196):**
```rust
/// Populate neighbor_connectivity: link each island to portals in each direction
/// 
/// For each cluster, determines which portals each island can access.
/// Direction mapping: 0=North, 1=South, 2=East, 3=West
```

**Actual implementation in graph.rs (lines 217-223):**
```rust
let direction = if portal.node.y == cluster_y_tiles {
    1 // South edge
} else if portal.node.y == cluster_max_y {
    0 // North edge
} else if portal.node.x == cluster_x_tiles {
    3 // West edge
} else if portal.node.x == cluster_max_x {
    2 // East edge
```

### Impact

This is **CRITICAL** because:
- The implementation uses: `0=North, 1=South, 2=East, 3=West`
- The cluster.rs documentation says: `0=North, 1=East, 2=South, 3=West`
- While the current code appears to work (tests pass), this inconsistency could lead to bugs if:
  1. Someone modifies the code based on cluster.rs documentation
  2. Other parts of the codebase rely on different direction mappings
  3. Future features need to interpret directions differently

### Why Tests Pass

The tests pass because **the implementation is internally consistent**:
- Portal creation uses the correct mapping (0=N, 1=S, 2=E, 3=W)
- Portal lookup also uses the same mapping
- The bug is purely in the **documentation**, not the code itself

However, this could explain user-reported bugs if:
1. There's code elsewhere that reads the cluster.rs documentation and uses the wrong mapping
2. The direction mapping was recently changed but documentation wasn't updated everywhere
3. There are edge cases in the movement code that interpret directions differently

### Recommended Fix

**Option 1 (Minimal):** Fix the documentation in cluster.rs to match the implementation:
```rust
/// Direction: 0=North, 1=South, 2=East, 3=West
```

**Option 2 (Better):** Create constants to make the mapping explicit and self-documenting:
```rust
const DIR_NORTH: usize = 0;
const DIR_SOUTH: usize = 1;
const DIR_EAST: usize = 2;
const DIR_WEST: usize = 3;
```

## Tests Created

I created comprehensive tests in [tests/pathfinding_correctness.rs](../../tests/pathfinding_correctness.rs) that verify:

1. **test_direction_mapping_consistency** - Verifies portals are in the correct directions
2. **test_simple_path_north** - Tests north-bound paths
3. **test_simple_path_east** - Tests east-bound paths
4. **test_opposite_direction_bug** - Verifies all portals point in the correct direction
5. **test_routing_table_correctness** - Verifies routing table produces paths that make progress toward goal
6. **test_path_around_obstacle_north_side** - Tests pathfinding around obstacles
7. **test_intra_cluster_routing** - Tests region-to-region routing within clusters

All tests currently **PASS**, which means:
- The core pathfinding logic is working correctly
- Portal creation and routing are functioning as intended
- The bug is in documentation, not implementation

## Possible Sources of User-Reported Bugs

Since the tests pass but users report bugs, consider:

1. **Unit movement code interpretation** - Check if [src/game/simulation/systems.rs](../../src/game/simulation/systems.rs) interprets portal directions differently

2. **Portal position precision** - The movement code uses world coordinates while portals use grid coordinates. Check conversion:
   - `world_to_grid` and `grid_to_world` in FlowField
   - `world_to_cluster_local` in region_decomposition

3. **Island detection issues** - If islands are detected incorrectly, units might take wrong portals:
   - Check tortuosity threshold (currently 3.0)
   - Verify island assignment in clusters with obstacles

4. **Goal island determination** - In [systems.rs](../../src/game/pathfinding/systems.rs#L36-48), the code determines goal_island when creating a Path. This might be using stale data or incorrect region lookups.

5. **Region lookup precision** - `get_region_id` might fail in edge cases:
   - Units exactly on region boundaries
   - Floating point precision issues with FixedNum
   - Cluster-local coordinate conversion errors

## Next Steps

1. **DO NOT FIX YET** - As requested, I've only identified bugs and created tests
2. Fix the documentation inconsistency (low risk, high clarity)
3. Add logging to movement system to see which directions units choose
4. Add test cases for:
   - Units on cluster boundaries
   - Units on region boundaries  
   - Goals in different islands within same cluster
5. Profile a failing case to see which portal/direction is chosen incorrectly
