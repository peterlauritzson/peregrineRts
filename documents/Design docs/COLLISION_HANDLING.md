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

## 9. Future Improvements
*   **Hard Collisions:** For gameplay reasons, we might want "hard" collisions that absolutely prevent overlap (e.g., for blocking units).
*   **Mass/Weight:** Currently all units have equal weight. We may add mass so tanks can push infantry.
*   **Push Priority:** Moving units should push idle units out of the way (bumping).
