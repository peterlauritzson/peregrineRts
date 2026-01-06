# Peregrine Proximity & Collision Systems

This document details the proximity query and collision detection/resolution systems in Peregrine.

> **Architectural Note:** "Collision" is a subset of "proximity detection". The spatial partitioning system serves as a general-purpose proximity query engine for multiple gameplay systems, not just physical collisions.

## 1. Core Philosophy
*   **Determinism:** All physics calculations use fixed-point arithmetic (`FixedNum`, `FixedVec2`) to ensure identical results across different machines (crucial for RTS lockstep networking).
*   **Performance:** We prioritize throughput (10k+ units) over perfect physical accuracy. Collisions are "soft" (separation forces) rather than rigid body solves.
*   **Simplicity:** Units are treated as circles with a fixed radius.
*   **Generality:** The proximity query system supports multiple use cases beyond just collision detection.

## 2. Spatial Partitioning: The Foundation

To avoid $O(N^2)$ proximity checks (which would require 100 billion comparisons for 10,000 units), we use a **Spatial Hash Grid** as the core spatial partitioning structure.

### 2.1 Structure
*   **Grid:** A 2D grid covering the map. Each cell contains a list of Entity IDs and positions.
*   **Cell Size:** Tuned based on typical query radius (usually 2-3x unit radius).
*   **Dynamic:** Rebuilt every physics tick as entities move.

### 2.2 Lifecycle (Every Tick)
1.  **Clear:** The grid is cleared at the start of every physics tick.
2.  **Insert:** Every dynamic entity inserts itself into the cell(s) corresponding to its position.
3.  **Query:** Systems query nearby cells based on their search radius.

### 2.3 Query Types

The spatial hash supports multiple query patterns:

| Query Type | Purpose | Example Usage | Radius |
|------------|---------|---------------|--------|
| **Collision Query** | Find overlapping entities | Physics collision detection | `2 × unit_radius` |
| **Proximity Query** | Find nearby entities | Boids flocking, aggro detection | `neighbor_radius` (5-10 units) |
| **Attack Range Query** | Find targets in range | Combat target selection | `weapon_range` (varies) |
| **AoE Query** | Find entities in area | Explosion damage, heal aura | `effect_radius` (varies) |
| **Layer-Filtered Query** | Find specific entity types | "Find enemy units in range" | Varies |

### 2.4 Query API (Proposed)

```rust
impl SpatialHash {
    /// General proximity query: Find all entities within radius
    /// Used by: Boids, AI, general gameplay
    pub fn query_radius(
        &self, 
        pos: FixedVec2, 
        radius: FixedNum
    ) -> Vec<(Entity, FixedVec2)>;
    
    /// Layer-filtered query: Find entities matching layer mask within radius
    /// Used by: Combat systems, AI target selection
    pub fn query_radius_filtered(
        &self, 
        pos: FixedVec2, 
        radius: FixedNum, 
        layer_mask: u32
    ) -> Vec<(Entity, FixedVec2)>;
    
    /// Legacy collision query (may be deprecated in favor of query_radius)
    pub fn get_potential_collisions(
        &self, 
        pos: FixedVec2, 
        radius: FixedNum
    ) -> Vec<(Entity, FixedVec2)>;
}
```

### 2.5 Design Principles

1. **Single Source of Truth:** One spatial structure for all proximity queries
2. **Self-Exclusion:** Queries never return the querying entity itself
3. **Correctness over Speed:** Spatial hash must return identical results to brute-force O(N) search
4. **Layer Awareness:** Support collision layers for filtering
5. **Performance:** Target O(1) amortized query time

---

## 3. Use Case: Physical Collision Detection
---

## 3. Use Case: Physical Collision Detection

Physical collision is when two entities **overlap** and need to be separated or respond physically.

### 3.1 Unit-Unit Collision

#### Detection (Narrow Phase)
*   Query the spatial hash with radius = `search_radius` (typically 2-4x unit radius)
*   For each potential neighbor, check if `distance_squared < (radius_A + radius_B)^2`
*   Emit `CollisionEvent` if overlapping

#### Resolution (Soft Collisions)
*   We do not use hard constraints (teleporting units out).
*   Instead, we apply a **Separation Impulse**:
    $$ \text{Impulse} = \text{Direction} \times \text{Overlap} \times \text{Stiffness} $$
*   This results in units "squishing" slightly when crowded but pushing apart smoothly over time.

#### Arrival Crowding
*   **Problem:** When 50 units try to reach the exact same point, they fight forever.
*   **Solution:** If a unit is close to its target but collides with another unit that has *already stopped* (no target), the moving unit considers itself "Arrived" and stops immediately. This creates organic formations around the goal.

### 3.2 Unit-Obstacle Collision

#### Static Obstacles (Walls)
*   **Data Structure:** We use the `MapFlowField` (a dense grid) to store static obstacles (walls, buildings).
*   **Detection:**
    *   Units check the grid cells in their immediate vicinity (3x3 area).
    *   If a cell is marked as an obstacle (value 255), it is treated as a circular collider centered on that tile.
*   **Resolution:**
    *   **Push Back:** Similar to unit-unit collision, a repulsion force pushes the unit out of the wall.
    *   **Wall Sliding:** In the movement system, we project the desired velocity onto the wall tangent.
        $$ \text{Velocity} = \text{DesiredVelocity} - (\text{DesiredVelocity} \cdot \text{WallNormal}) \times \text{WallNormal} $$
        This allows units to slide along walls instead of getting stuck pushing into them.

### 3.3 Map Boundaries
*   A hard constraint system clamps unit positions to the map dimensions (`map_width`, `map_height`).
*   Velocity components perpendicular to the boundary are zeroed out to prevent "sticking" or jittering.

---

## 4. Use Case: Boids Flocking (Proximity-Based Steering)

Boids require finding **nearby neighbors** (not necessarily overlapping) to calculate:
- **Separation:** Steer away from very close neighbors
- **Alignment:** Match velocity with nearby neighbors  
- **Cohesion:** Steer toward center of mass of neighbors

### 4.1 Query Pattern
```rust
// Each unit queries for neighbors within flocking radius
let neighbors = spatial_hash.query_radius(pos, neighbor_radius);

// Filter out self (spatial hash should do this automatically)
// Calculate separation, alignment, cohesion forces
```

### 4.2 Performance Impact
- **Without spatial hash:** O(N²) = 100 billion comparisons for 10k units
- **With spatial hash:** O(N) = ~10 million comparisons (assuming 10 neighbors per unit)
- **Speedup:** ~10,000x faster

### 4.3 Critical Implementation Note
The boids system currently uses a brute-force O(N²) approach and does NOT use the spatial hash. This is **Issue #1** in CURRENT_IMPROVEMENTS.md and must be fixed before scaling beyond 1,000 units.

---

## 5. Use Case: Combat & AI (Layer-Filtered Queries)

Future combat and AI systems will need to find specific entity types within range:

### 5.1 Attack Range Queries
```rust
// Find enemy units within weapon range
let targets = spatial_hash.query_radius_filtered(
    unit_pos, 
    weapon_range, 
    layers::ENEMY | layers::BUILDING
);
```

### 5.2 Threat Detection (Aggro)
```rust
// Find any threats within detection radius
let threats = spatial_hash.query_radius_filtered(
    unit_pos,
    detection_radius,
    layers::ENEMY
);

if !threats.is_empty() {
    // Enter combat mode or flee
}
```

### 5.3 Area of Effect Abilities
```rust
// Find all units in explosion radius
let affected = spatial_hash.query_radius(
    explosion_pos,
    blast_radius
);

for (entity, pos) in affected {
    apply_damage(entity, calculate_falloff(pos, explosion_pos));
}
```

---

## 6. Map Boundaries
*   A hard constraint system clamps unit positions to the map dimensions (`map_width`, `map_height`).
*   Velocity components perpendicular to the boundary are zeroed out to prevent "sticking" or jittering.

---

## 7. Collision Filtering & Layers (The 10M Unit Strategy)

To handle 10 million entities (Units, Projectiles, Flying Units, Buildings), we cannot simply check everything against everything. We need a robust **Collision Layer System**.

### The Layer Bitmask
Every physical entity will have a `CollisionLayer` component containing two bitmasks:
1.  `layer`: What I am (e.g., `UNIT_GROUND`, `PROJECTILE`, `UNIT_AIR`).
2.  `mask`: What I collide with (e.g., `UNIT_GROUND | BUILDING`).

```rust
struct CollisionFilter {
    category: u32, // Bitmask of what this object IS
    mask: u32,     // Bitmask of what this object COLLIDES WITH
}
```

### Interaction Matrix
| Type | Collides With | Proximity Queries For | Logic |
| :--- | :--- | :--- | :--- |
| **Ground Unit** | Ground Units, Buildings, Terrain | Allies (Boids), Enemies (Combat) | Soft collision (push), Hard collision (slide) |
| **Air Unit** | Air Units (Soft), Anti-Air Projectiles | Other Air Units (Flocking) | "Flocking" separation, ignores ground/buildings |
| **Projectile** | Target Unit, Buildings, Terrain | Target Layer Only | Hit detection (destroy self, damage other). Ignores other projectiles. |
| **Building** | Ground Units, Projectiles | Units in Range (Turrets) | Static hard collider. |

### Optimization Strategy for 10M Entities
For extreme scale, a single Spatial Hash is inefficient because iterating through 1000 projectiles to find 1 unit in a cell is slow.

**Strategy: Split Spatial Hashes**
We will maintain **separate spatial structures** for distinct physics domains:

1.  **Static Grid (Buildings/Terrain):**
    *   **Structure:** Dense 2D Array / Flow Field.
    *   **Usage:** Read-only for most frames. Extremely fast lookup ($O(1)$).
    *   **Who checks it:** Ground Units, Projectiles (checking for wall hits).

2.  **Dynamic Ground Hash (Units):**
    *   **Structure:** Spatial Hash (as currently implemented).
    *   **Usage:** Updated every frame.
    *   **Who checks it:** Ground Units (separation), Projectiles (hit detection).

3.  **Dynamic Air Hash (Flyers):**
    *   **Structure:** Separate Spatial Hash with larger cell size (flyers move faster/looser).
    *   **Usage:** Updated every frame.
    *   **Who checks it:** Air Units (separation), Anti-Air Projectiles.

4.  **Projectile System (No Hash?):**
    *   Projectiles often don't need to collide with *each other*.
    *   They only need to query the *Ground Hash* or *Static Grid*.
    *   **Optimization:** We do **not** insert projectiles into a spatial hash. Instead, projectiles perform queries against the Unit/Building hashes. This saves millions of insertions per frame.

---

## 8. Testing & Validation

The proximity/collision system is the foundation of gameplay. It must be exhaustively tested.

### 8.1 Correctness Tests
- **Spatial Query Equivalence:** Spatial hash queries must return identical results to brute-force O(N) search
- **No Self-Queries:** Entities must never appear in their own query results
- **No Duplicates:** Each entity appears at most once in query results
- **Complete Coverage:** All entities within radius must be found

### 8.2 Performance Benchmarks
- **Query Time:** O(1) amortized for typical query radii
- **1k Units:** All queries complete in <1ms
- **10k Units:** All queries complete in <10ms
- **100k Units:** All queries complete in <100ms (target)

### 8.3 Stress Tests
- **1000 entities in same cell:** Should still work (degraded performance acceptable)
- **Query radius larger than map:** Should return all entities
- **Entities at map boundaries:** Should handle correctly
- **Negative coordinates:** Should work if map supports them

---

## 9. Performance Analysis & Findings

### 9.1 Known Performance Issues (January 2026)

#### Issue 1: Excessive Collision Search Radius
**Status:** CRITICAL - Major performance impact with 1-2k units

**Problem:** 
The collision detection system uses `collision_search_radius_multiplier = 4.0`, meaning each unit queries a radius **4 times** its own collision radius (0.5 units). This results in:
- Query radius of 2.0 units (4 × 0.5)
- Query area of ~12.56 square units (π × 2²)
- With spatial hash cell size of 2.0, this checks a **3×3 grid** (9 cells) for every unit

**Performance Impact:**
- With 2000 units, this generates **potentially millions** of proximity checks per frame
- Each unit checks ~9 grid cells, each containing multiple entities
- Most of these checks find entities that are NOT actually colliding
- Example: If average cell occupancy is 10 entities, each unit checks ~90 neighbors
  - Total potential checks: 2000 × 90 = **180,000 checks per frame**
  - Actual collisions might be < 1% of these checks

**Evidence:**
- Noticeable lag at 1-2k units even without pathfinding
- All lag occurs during physics phase
- Most CPU time spent in collision detection loop

**Root Cause:**
The multiplier of 4.0x was chosen conservatively but is far too large for the actual collision needs. Units only collide when overlapping (distance < 1.0 unit for two 0.5-radius units), yet we're querying 2.0 units away.

**Potential Solutions (DO NOT IMPLEMENT YET):**
1. Reduce multiplier to 2.5x (query radius 1.25) - still safe but 4x smaller area
2. Implement spatial hash with adaptive cell sizes
3. Use broad-phase/narrow-phase separation
4. Consider octree or KD-tree for very dense areas

---

#### Issue 2: Duplicate Entity Pair Checks
**Status:** MODERATE - Wastes ~50% of narrow-phase checks

**Problem:**
The current collision detection uses entity ID comparison (`if entity > other_entity { continue; }`) to avoid checking the same pair twice. However:
- The spatial hash still returns **both directions** of every pair
- We iterate through all neighbors, then skip half of them
- This wastes CPU cycles on:
  - Query result allocation
  - Layer mask checking  
  - Entity component lookups

**Performance Impact:**
With 2000 units and 180,000 potential checks:
- ~90,000 checks are immediately discarded due to entity ordering
- Each discarded check still required:
  - Vector iteration
  - Conditional check
  - Branch prediction

**Example:**
```
Unit A checks neighbors → finds Unit B → processes collision
Unit B checks neighbors → finds Unit A → SKIPS (A < B, already processed)
                                         ↑ WASTED WORK
```

**Potential Solutions (DO NOT IMPLEMENT YET):**
1. Spatial hash could track "already checked" pairs per frame
2. Use spatial sweep algorithm (sort entities, only check forward)
3. Grid-based collision where each cell is responsible for checking its contents

---

#### Issue 3: Spatial Hash Cell Size Mismatch
**Status:** MINOR - Suboptimal cache usage

**Problem:**
- Spatial hash cell size: **2.0 units**
- Typical collision query radius: **2.0 units** (0.5 × 4.0)
- Unit collision radius: **0.5 units**

This creates inefficiency:
- Each query always checks a 3×3 grid (9 cells) due to how cell boundaries align with query radius
- Many queried cells contain entities that are geometrically impossible to collide with
- Cells are too large relative to unit size (4× the unit diameter)

**Better Configuration:**
- Cell size should be ~2-3× the query radius (not equal to it)
- Current: Cell = 2.0, Query = 2.0 (ratio 1:1) → checks 9 cells
- Optimal: Cell = 4.0, Query = 2.0 (ratio 2:1) → would check 4 cells
- Alternative: Cell = 1.0, Query = 2.0 (ratio 1:2) → would check 25 cells but each cell is smaller

The current 1:1 ratio is neither fish nor fowl - we're checking many cells, each with many entities.

**Trade-off Analysis:**
- **Larger cells** (4.0): Fewer cells to check, but more entities per cell (worse)
- **Smaller cells** (1.0): More cells to check, but fewer entities per cell (better cache)
- **Current** (2.0): Middle ground, but aligned poorly with query patterns

---

#### Issue 4: O(N × M) Free Obstacle Checking
**Status:** MODERATE - Scales poorly with obstacle count

**Problem:**
In `resolve_obstacle_collisions`, the system checks **every unit** against **every free obstacle** (obstacles not in the flow field grid):

```rust
for each unit:
    for each free_obstacle:
        check collision  // O(N × M) where N = units, M = free obstacles
```

**Performance Impact:**
- With 2000 units and 50 free obstacles: **100,000 checks per frame**
- NO spatial partitioning for free obstacles
- Each check involves:
  - Distance calculation (sqrt)
  - Multiple floating-point comparisons
  - Force calculations

**Current Mitigation:**
Flow field grid obstacles ARE spatially partitioned (units check only nearby grid cells). But free obstacles are not.

**Potential Solutions (DO NOT IMPLEMENT YET):**
1. Insert free obstacles into spatial hash
2. Use separate spatial hash for static obstacles
3. Merge free obstacles into flow field grid during initialization

---

#### Issue 5: Redundant Layer Mask Checks
**Status:** MINOR - Small overhead, easy to fix

**Problem:**
Layer filtering happens AFTER spatial query returns all nearby entities:

```rust
let potential = spatial_hash.get_potential_collisions(...);  // Returns 100 entities
for entity in potential {
    if layer_check_fails { continue; }  // Discards 50 of them
    // Actual collision check
}
```

Better approach: Store layers in spatial hash and filter during query construction.

**Performance Impact:**
- Minor: Layer checks are cheap bitwise operations
- But wastes iteration cycles on entities we'll never interact with
- Matters more with 10k+ entities

---

#### Issue 6: Allocation in Hot Path
**Status:** MINOR - GC pressure

**Problem:**
`get_potential_collisions` allocates a new `Vec` on every call:

```rust
pub fn get_potential_collisions(...) -> Vec<(Entity, FixedVec2)> {
    let mut result = Vec::new();  // ← ALLOCATION
    // ...
    result
}
```

With 2000 units × 30 fps × 2 collision systems = **120,000 allocations per second**.

**Potential Solutions (DO NOT IMPLEMENT YET):**
1. Pre-allocate thread-local result buffer
2. Use smallvec for common case (most queries return < 16 neighbors)
3. Iterator-based API instead of returning Vec

---

### 9.2 Performance Logging

To diagnose collision performance issues, comprehensive logging has been added to all collision systems. The logging is designed to be minimally invasive while providing actionable metrics.

#### Logging Strategy

**Frequency:**
- Log every 100 ticks (every ~3.3 seconds at 30 Hz tick rate)
- Log immediately if any system exceeds performance threshold:
  - `detect_collisions`: > 5ms
  - `resolve_collisions`: > 2ms
  - `resolve_obstacle_collisions`: > 2ms
  - `update_spatial_hash`: > 2ms

**Rationale:** Avoid log spam while catching performance regressions quickly.

#### Key Metrics Tracked

**1. Collision Detection (`detect_collisions`)**
```
[COLLISION_DETECT] 12.5ms | Entities: 2000 | Neighbors: 180000 (avg: 90.0, max: 150) |
Potential checks: 180000 | Duplicate skips: 90000 | Layer filtered: 5000 |
Actual collisions: 1200 | Hit ratio: 0.67% | Search radius multiplier: 4.0x
```

Metrics explained:
- **Duration**: Wall-clock time spent in system
- **Entities**: Total units being simulated
- **Neighbors**: Total neighbor entities returned by spatial queries
  - `avg`: Average neighbors found per entity (should be < 20 for good performance)
  - `max`: Highest neighbor count for any single entity (indicates density hotspots)
- **Potential checks**: Total narrow-phase checks attempted
- **Duplicate skips**: Checks skipped due to entity pair ordering (should be ~50% of potential)
- **Layer filtered**: Checks skipped due to collision layer mask mismatch
- **Actual collisions**: Collision events generated (actual overlaps)
- **Hit ratio**: `(actual_collisions / potential_checks) × 100%`
  - **< 1%**: Search radius too large (most checks are wasted)
  - **1-5%**: Acceptable range
  - **> 10%**: Units extremely densely packed or search radius too small

**2. Collision Resolution (`resolve_collisions`)**
```
[COLLISION_RESOLVE] 1.2ms | Collision events processed: 1200
```

- **Duration**: Time to apply separation forces
- **Events processed**: Should match "Actual collisions" from detection

**3. Obstacle Collision Resolution (`resolve_obstacle_collisions`)**
```
[OBSTACLE_RESOLVE] 3.8ms | Units: 2000 | Grid checks: 18000 (avg: 9.0, collisions: 50) |
Free obstacle checks: 100000 (avg: 50.0, collisions: 200)
```

Metrics explained:
- **Grid checks**: Flow field cell checks (should be 9-25 per unit depending on `obstacle_search_range`)
- **Free obstacle checks**: Brute-force checks against non-grid obstacles
  - High numbers here indicate need for spatial partitioning of free obstacles
- **Collisions**: Actual obstacles hit (expect < 5% of checks)

**4. Spatial Hash Update (`update_spatial_hash`)**
```
[SPATIAL_HASH_UPDATE] 0.8ms | Entities inserted: 2000
```

- **Duration**: Time to rebuild spatial hash
- Expected: ~0.0005ms per entity (1ms for 2000 entities)

#### Using Logs to Diagnose Issues

**Symptom: High frame time**
1. Check which collision system has highest duration
2. If `detect_collisions` is > 10ms: Check `avg neighbors` and `hit ratio`
   - High avg neighbors (> 50): Search radius too large OR units too densely packed
   - Low hit ratio (< 1%): Search radius too large
3. If `resolve_obstacle_collisions` is high: Check `free obstacle checks`
   - If > 10,000: Too many free obstacles, need spatial partitioning

**Symptom: Stutter/spikes**
- Check `max neighbors` value
- High value (> 200) indicates spatial hotspot (e.g., 300 units in one area)
- Solution: May need adaptive collision (skip checks in extremely dense areas)

**Symptom: Low actual collisions despite lag**
- Check `hit ratio`
- If < 0.5%: Collision search radius is far too large for actual density
- Reduce `collision_search_radius_multiplier` in config

---

## 10. Future Improvements
*   **Hard Collisions:** For gameplay reasons, we might want "hard" collisions that absolutely prevent overlap (e.g., for blocking units).
*   **Mass/Weight:** Currently all units have equal weight. We may add mass so tanks can push infantry.
*   **Push Priority:** Moving units should push idle units out of the way (bumping).
