# Peregrine Collision Handling & Physics

This document details the collision detection and resolution systems currently implemented in Peregrine.

## 1. Core Philosophy
*   **Determinism:** All physics calculations use fixed-point arithmetic (`FixedNum`, `FixedVec2`) to ensure identical results across different machines (crucial for RTS lockstep networking).
*   **Performance:** We prioritize throughput (10k+ units) over perfect physical accuracy. Collisions are "soft" (separation forces) rather than rigid body solves.
*   **Simplicity:** Units are treated as circles with a fixed radius.

## 2. Broad Phase: Spatial Hashing
To avoid $O(N^2)$ collision checks, we use a **Spatial Hash Grid**.
*   **Structure:** A 2D grid covering the map. Each cell contains a list of Entity IDs.
*   **Lifecycle:**
    1.  **Clear:** The grid is cleared at the start of every physics tick.
    2.  **Insert:** Every unit inserts itself into the cell corresponding to its position.
    3.  **Query:** To check for collisions, a unit queries its own cell and the 8 surrounding neighbor cells.
*   **Optimization:** The cell size is tuned to be slightly larger than the unit diameter to minimize the number of cells to check.

## 3. Unit-Unit Collision
### Detection
*   We iterate through all potential neighbors found via the Spatial Hash.
*   Collision is detected if `distance_squared < (radius_A + radius_B)^2`.

### Resolution (Soft Collisions)
*   We do not use hard constraints (teleporting units out).
*   Instead, we apply a **Separation Impulse**:
    $$ \text{Impulse} = \text{Direction} \times \text{Overlap} \times \text{Stiffness} $$
*   This results in units "squishing" slightly when crowded but pushing apart smoothly over time.

### Arrival Crowding
*   **Problem:** When 50 units try to reach the exact same point, they fight forever.
*   **Solution:** If a unit is close to its target but collides with another unit that has *already stopped* (no target), the moving unit considers itself "Arrived" and stops immediately. This creates organic formations around the goal.

## 4. Unit-Obstacle Collision
### Static Obstacles (Walls)
*   **Data Structure:** We use the `MapFlowField` (a dense grid) to store static obstacles (walls, buildings).
*   **Detection:**
    *   Units check the grid cells in their immediate vicinity (3x3 area).
    *   If a cell is marked as an obstacle (value 255), it is treated as a circular collider centered on that tile.
*   **Resolution:**
    *   **Push Back:** Similar to unit-unit collision, a repulsion force pushes the unit out of the wall.
    *   **Wall Sliding:** In the movement system (`follow_direct_target`), we project the desired velocity onto the wall tangent.
        $$ \text{Velocity} = \text{DesiredVelocity} - (\text{DesiredVelocity} \cdot \text{WallNormal}) \times \text{WallNormal} $$
        This allows units to slide along walls instead of getting stuck pushing into them.

## 5. Map Boundaries
*   A hard constraint system clamps unit positions to the map dimensions (`map_width`, `map_height`).
*   Velocity components perpendicular to the boundary are zeroed out to prevent "sticking" or jittering.

## 6. Collision Filtering & Layers (The 10M Unit Strategy)

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
| Type | Collides With | Logic |
| :--- | :--- | :--- |
| **Ground Unit** | Ground Units, Buildings, Terrain | Soft collision (push), Hard collision (slide) |
| **Air Unit** | Air Units (Soft), Anti-Air Projectiles | "Flocking" separation, but ignores ground/buildings |
| **Projectile** | Target Unit, Buildings, Terrain | Hit detection (destroy self, damage other). Ignores other projectiles. |
| **Building** | Ground Units, Projectiles | Static hard collider. |

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

## 7. Future Improvements
*   **Hard Collisions:** For gameplay reasons, we might want "hard" collisions that absolutely prevent overlap (e.g., for blocking units).
*   **Mass/Weight:** Currently all units have equal weight. We may add mass so tanks can push infantry.
*   **Push Priority:** Moving units should push idle units out of the way (bumping).
