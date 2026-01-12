# Peregrine Spatial Partitioning & Proximity Systems

This document details the spatial partitioning infrastructure and its various use cases: proximity queries, collision detection/resolution, boids flocking, combat targeting, and AI systems.

> **Architectural Note:** The spatial hash is a **general-purpose proximity query engine** for all gameplay systems. Physical collision detection is just one of many use cases. This system will also power boids flocking, enemy detection, attack range queries, area-of-effect abilities, and more.

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

### 2.2 Storage Strategy: Staggered Multi-Resolution Grids

**CRITICAL DESIGN DECISION (January 2026):**

The spatial hash uses a **staggered multi-resolution architecture** where:
1. **Multiple cell sizes** handle entities of different radii (small units vs huge obstacles)
2. **Each cell size has TWO offset grids** (Grid A and Grid B) with centers staggered by half_cell
3. **Entities are always single-cell** - inserted into whichever grid they're closest to the center of

This eliminates multi-cell complexity while maintaining correctness for all entity sizes.

**Why Staggered Grids Solve the Boundary Problem:**

**The Problem:** With a single grid, entities near cell boundaries need multi-cell storage:
```
Grid with cell_size=40:
- Entity at (38, 50): Near boundary of cells [0,1] and [1,1]
- Needs to be in BOTH cells to be found by queries
- Result: Complex multi-cell tracking
```

**The Solution:** Dual offset grids where entities are always near-center in at least one grid:
```
Grid A: Centers at (0, 40, 80, 120, ...)
Grid B: Centers at (20, 60, 100, 140, ...) - offset by 20 units

Entity at (38, 50):
- Distance to Grid A center (40, 40): 10.2 units - NEAR CENTER ✓
- Distance to Grid B center (20, 60): 22.4 units - near boundary
- Insert into Grid A only (single cell)

Entity at (60, 40):
- Distance to Grid A center (40, 40): 20.0 units - on boundary
- Distance to Grid B center (60, 40): 0.0 units - AT CENTER ✓
- Insert into Grid B only (single cell)
```

**Every entity is near-center in exactly one of the two grids!**

**Multi-Resolution for Variable Entity Sizes:**

Different entity sizes use different cell sizes to maintain radius << cell_size:

```rust
// Example configuration
Cell Size 40:  For entities radius 0.1 - 10.0 (units, small obstacles)
Cell Size 100: For entities radius 10.0 - 25.0 (medium buildings)
Cell Size 200: For entities radius 25.0+ (huge obstacles)

Each cell size has Grid A and Grid B (staggered)
```

**Component for Tracking:**
```rust
#[derive(Component)]
struct OccupiedCell {
    size_class: u8,    // Which cell size (0, 1, 2, ...)
    grid_offset: u8,   // 0 = Grid A, 1 = Grid B
    col: usize,        // Cell column
    row: usize,        // Cell row
    vec_idx: usize,    // Index in cell's Vec (for O(1) removal)
}
```

**Memory Savings:**
- Old multi-cell: `SmallVec<[(usize, usize, usize); 4]>` = 96 bytes
- New single-cell: 5 fields = ~25 bytes
- **71 bytes saved per entity** (500k entities = 35.5 MB saved)

### 2.3 Lifecycle (Every Tick)
1.  **Insert New Entities:** Entities without `OccupiedCell` are classified by size, then inserted into the nearest grid center (single cell)
2.  **Update Moved Entities:** Check if entity moved closer to opposite grid's center; if so, remove from old cell and insert into new
3.  **Static Entities:** Calculate cell once on spawn, never update (zero ongoing cost)
4.  **Query:** Systems query both Grid A and Grid B for the appropriate size classes

### 2.4 Query Types

The spatial hash supports multiple query patterns:

| Query Type | Purpose | Example Usage | Radius |
|------------|---------|---------------|--------|
| **Collision Query** | Find overlapping entities | Physics collision detection | `2 × unit_radius` |
| **Proximity Query** | Find nearby entities | Boids flocking, aggro detection | `neighbor_radius` (5-10 units) |
| **Attack Range Query** | Find targets in range | Combat target selection | `weapon_range` (varies) |
| **AoE Query** | Find entities in area | Explosion damage, heal aura | `effect_radius` (varies) |
| **Layer-Filtered Query** | Find specific entity types | "Find enemy units in range" | Varies |

**Query Correctness Guarantee:**
With staggered grids, queries check BOTH Grid A and Grid B for each relevant size class. This guarantees finding all entities within the search radius - even entities on cell boundaries are guaranteed to be in the grid where they're near-center.

### 2.5 Query API (Proposed)

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

### 2.6 Design Principles

1. **Multi-Cell Storage:** Entities occupy all cells their radius overlaps (guarantees correctness)
2. **Single Source of Truth:** One spatial structure for all proximity queries
3. **Self-Exclusion:** Queries never return the querying entity itself
4. **Correctness over Speed:** Spatial hash must return identical results to brute-force O(N) search
5. **Layer Awareness:** Support collision layers for filtering
6. **Performance:** Target O(1) amortized query time
7. **Future-Proof:** Handles variable entity sizes (small dogs to capital ships)

### 2.7 Performance Characteristics

**Memory Cost:**
- **Per entity:** 25 bytes (vs 96 bytes for old multi-cell approach)
- **Per grid:** 2× storage (Grid A + Grid B) but entities in only one
- **Per size class:** Independent grids, total scales with entity size distribution
- **500k entities:** ~12.5 MB for components (vs 48 MB multi-cell)

**Update Cost:**
- Static obstacles: **Zero** (inserted once, never updated)
- Dynamic units: Only update when crossing midpoint between grid centers
- Update threshold: ~half_cell distance (20 units for cell_size=40)
- **Much rarer than multi-cell updates** (which triggered at cell_size/4 distance)

**Query Cost:**
- Must query both Grid A and Grid B (2× grid queries)
- But queries can be parallelized or memory-interleaved
- With neighbor caching (already implemented): Query cost mostly irrelevant (95% cache hit)
- Each grid query is simpler (no duplicate detection within grid)

### 2.8 Spatial Hash Update Optimizations (January 2026)

**CRITICAL PERFORMANCE INSIGHT:**
With staggered grids, spatial hash updates become **dramatically simpler** because:
1. **Entities are always single-cell** - no multi-cell calculation
2. **Update threshold is large** (~half_cell = 20 units for cell_size=40)
3. **Check is trivial** - just distance comparison to two grid centers

**Update Logic:**

```rust
fn should_update_cell(
    pos: FixedVec2,
    occupied: &OccupiedCell,
    spatial_hash: &StaggeredMultiResolutionHash,
) -> bool {
    let size_class = &spatial_hash.size_classes[occupied.size_class as usize];
    
    // Get current grid (A or B)
    let current_grid = if occupied.grid_offset == 0 {
        &size_class.grid_a
    } else {
        &size_class.grid_b
    };
    
    // Calculate center of current cell
    let current_center = current_grid.calculate_cell_center(occupied.col, occupied.row);
    
    // Calculate center of cell in opposite grid
    let opposite_grid = if occupied.grid_offset == 0 {
        &size_class.grid_b
    } else {
        &size_class.grid_a
    };
    
    let opposite_center = opposite_grid.closest_cell_center(pos);
    
    // Update if now closer to opposite grid's center
    let dist_to_current = (pos - current_center).length_squared();
    let dist_to_opposite = (pos - opposite_center).length_squared();
    
    dist_to_opposite < dist_to_current
}
```

**Performance:**
- **Cost per check:** 2 distance calculations (no divisions!)
- **Update frequency:** Only when entity crosses midpoint between grids
- **Typical movement:** Entity moves ~20 units before update (vs 10 units in old system)
- **Combined skip rate:** 95%+ per tick (entities rarely cross grid midpoints)

**No Complex Optimizations Needed:**
- No velocity-based prediction
- No distance thresholds  
- No multi-cell symmetric difference
- Just: "Am I closer to the other grid now?"

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

### 7.1 Staggered Multi-Resolution Spatial Hash Architecture

**DESIGN (January 2026) - Implementation Target**

The spatial hash uses a **staggered multi-resolution** design that combines:
1. **Multiple cell sizes** for different entity size ranges
2. **Dual offset grids** (Grid A and Grid B) for each cell size
3. **Single-cell insertion** for all entities (no multi-cell complexity)

This architecture eliminates the problems of both naive single-grid and multi-cell approaches.

---

#### Architecture Overview

```rust
/// Main spatial hash resource
struct StaggeredMultiResolutionHash {
    /// Array of size classes, each with staggered grids
    size_classes: Vec<SizeClass>,
    
    /// Map from entity radius to size class index
    /// Precomputed during initialization
    radius_to_class: Vec<(FixedNum, u8)>,  // (max_radius, class_index)
}

/// One size class = one cell_size with two staggered grids
struct SizeClass {
    cell_size: FixedNum,
    grid_a: StaggeredGrid,  // Centers at (0, cell_size, 2*cell_size, ...)
    grid_b: StaggeredGrid,  // Centers at (cell_size/2, 3*cell_size/2, ...)
    entity_count: usize,    // For query optimization (skip empty classes)
}

/// One grid in a staggered pair
struct StaggeredGrid {
    cells: Vec<Vec<Entity>>,  // Flat storage: cells[row * cols + col]
    cols: usize,
    rows: usize,
    cell_size: FixedNum,
    offset: FixedVec2,        // Grid A: (0, 0), Grid B: (cell_size/2, cell_size/2)
}

/// Component tracking entity's location
#[derive(Component)]
struct OccupiedCell {
    size_class: u8,    // Index into size_classes array
    grid_offset: u8,   // 0 = Grid A, 1 = Grid B
    col: usize,        // Cell column
    row: usize,        // Cell row
    vec_idx: usize,    // Index in cell's Vec (for O(1) removal)
}
```

---

#### Initialization

The spatial hash is initialized with:
1. **Array of expected entity radii** (e.g., [0.5, 2.0, 10.0, 50.0])
2. **Radius-to-cell-size ratio** (e.g., 4.0 means cell_size = 4 × radius)

**Initialization Algorithm:**

```rust
fn initialize(
    map_width: FixedNum,
    map_height: FixedNum,
    entity_radii: &[FixedNum],  // Expected entity sizes in game
    radius_to_cell_ratio: f32,   // Desired ratio (e.g., 4.0)
) -> StaggeredMultiResolutionHash {
    // Step 1: Determine unique cell sizes needed
    let mut cell_sizes = Vec::new();
    for &radius in entity_radii {
        let cell_size = radius * FixedNum::from_num(radius_to_cell_ratio);
        
        // Merge similar cell sizes (within 20% of each other)
        let existing = cell_sizes.iter().find(|&&cs| {
            let ratio = (cs / cell_size).to_num::<f32>();
            ratio >= 0.8 && ratio <= 1.2
        });
        
        if existing.is_none() {
            cell_sizes.push(cell_size);
        }
    }
    
    // Sort cell sizes (smallest to largest)
    cell_sizes.sort();
    
    // Step 2: Create size classes with staggered grids
    let size_classes: Vec<SizeClass> = cell_sizes.iter().map(|&cell_size| {
        SizeClass {
            cell_size,
            grid_a: StaggeredGrid::new(
                map_width, map_height, cell_size,
                FixedVec2::ZERO,  // No offset
            ),
            grid_b: StaggeredGrid::new(
                map_width, map_height, cell_size,
                FixedVec2::new(cell_size / 2, cell_size / 2),  // Offset by half_cell
            ),
            entity_count: 0,
        }
    }).collect();
    
    // Step 3: Build radius-to-class mapping
    let mut radius_to_class = Vec::new();
    for (idx, &cell_size) in cell_sizes.iter().enumerate() {
        let max_radius = cell_size / FixedNum::from_num(radius_to_cell_ratio);
        radius_to_class.push((max_radius, idx as u8));
    }
    
    StaggeredMultiResolutionHash {
        size_classes,
        radius_to_class,
    }
}
```

**Example Configuration:**

```ron
// In game config
spatial_hash_config: (
    entity_radii: [0.5, 10.0, 25.0],  // Units, medium obstacles, huge obstacles
    radius_to_cell_ratio: 4.0,         // cell_size = 4 × radius
)
```

**Result:**
- Size Class 0: `cell_size = 2.0` for radius 0.1-0.5 (units)
  - Grid A: Centers at (0, 2, 4, 6, ...)
  - Grid B: Centers at (1, 3, 5, 7, ...)
- Size Class 1: `cell_size = 40.0` for radius 0.5-10.0 (medium obstacles)
  - Grid A: Centers at (0, 40, 80, ...)
  - Grid B: Centers at (20, 60, 100, ...)
- Size Class 2: `cell_size = 100.0` for radius 10.0-25.0 (huge obstacles)
  - Grid A: Centers at (0, 100, 200, ...)
  - Grid B: Centers at (50, 150, 250, ...)

---

#### Insertion

**Classify entity by radius:**
```rust
fn classify_entity(radius: FixedNum, hash: &StaggeredMultiResolutionHash) -> u8 {
    // Binary search through radius_to_class mapping
    for &(max_radius, class_idx) in &hash.radius_to_class {
        if radius <= max_radius {
            return class_idx;
        }
    }
    // Default to largest size class
    (hash.size_classes.len() - 1) as u8
}
```

**Insert into nearest grid center:**
```rust
fn insert(
    hash: &mut StaggeredMultiResolutionHash,
    entity: Entity,
    pos: FixedVec2,
    radius: FixedNum,
) -> OccupiedCell {
    // 1. Determine size class
    let size_class_idx = classify_entity(radius, hash);
    let size_class = &mut hash.size_classes[size_class_idx as usize];
    
    // 2. Find nearest center in Grid A
    let (col_a, row_a) = size_class.grid_a.pos_to_cell(pos);
    let center_a = size_class.grid_a.cell_center(col_a, row_a);
    let dist_a_sq = (pos - center_a).length_squared();
    
    // 3. Find nearest center in Grid B
    let (col_b, row_b) = size_class.grid_b.pos_to_cell(pos);
    let center_b = size_class.grid_b.cell_center(col_b, row_b);
    let dist_b_sq = (pos - center_b).length_squared();
    
    // 4. Insert into whichever grid is closer
    let (grid_offset, col, row, vec_idx) = if dist_a_sq < dist_b_sq {
        let idx = size_class.grid_a.insert_entity(col_a, row_a, entity);
        (0, col_a, row_a, idx)
    } else {
        let idx = size_class.grid_b.insert_entity(col_b, row_b, entity);
        (1, col_b, row_b, idx)
    };
    
    size_class.entity_count += 1;
    
    OccupiedCell {
        size_class: size_class_idx,
        grid_offset,
        col,
        row,
        vec_idx,
    }
}
```

**Key Insight:** Entity always goes into exactly ONE cell - the one whose center it's closest to.

---

#### Removal

**O(1) removal using vec_idx:**
```rust
fn remove(
    hash: &mut StaggeredMultiResolutionHash,
    occupied: &OccupiedCell,
) -> Option<Entity> {
    let size_class = &mut hash.size_classes[occupied.size_class as usize];
    
    let grid = if occupied.grid_offset == 0 {
        &mut size_class.grid_a
    } else {
        &mut size_class.grid_b
    };
    
    let removed = grid.remove_entity(occupied.col, occupied.row, occupied.vec_idx);
    
    if removed.is_some() {
        size_class.entity_count -= 1;
    }
    
    removed
}
```

---

#### Update (Movement)

**Check if entity switched grids:**
```rust
fn update(
    hash: &mut StaggeredMultiResolutionHash,
    entity: Entity,
    old_pos: FixedVec2,
    new_pos: FixedVec2,
    occupied: &mut OccupiedCell,
) {
    let size_class = &hash.size_classes[occupied.size_class as usize];
    
    // Get current grid center
    let current_grid = if occupied.grid_offset == 0 {
        &size_class.grid_a
    } else {
        &size_class.grid_b
    };
    let current_center = current_grid.cell_center(occupied.col, occupied.row);
    
    // Get opposite grid center
    let opposite_grid = if occupied.grid_offset == 0 {
        &size_class.grid_b
    } else {
        &size_class.grid_a
    };
    let (opp_col, opp_row) = opposite_grid.pos_to_cell(new_pos);
    let opposite_center = opposite_grid.cell_center(opp_col, opp_row);
    
    // Check if now closer to opposite grid
    let dist_current = (new_pos - current_center).length_squared();
    let dist_opposite = (new_pos - opposite_center).length_squared();
    
    if dist_opposite < dist_current {
        // Remove from current grid
        remove(hash, occupied);
        
        // Insert into opposite grid
        let new_occupied = insert(hash, entity, new_pos, /* radius from query */);
        *occupied = new_occupied;
    }
    // Otherwise: Entity still in same cell, no update needed!
}
```

**Performance:** Entities typically move 20-40 units before switching grids (much rarer than multi-cell updates).

---

#### Query

**Query must check both grids in each relevant size class:**

```rust
fn query_radius(
    hash: &StaggeredMultiResolutionHash,
    pos: FixedVec2,
    search_radius: FixedNum,
    exclude_entity: Option<Entity>,
) -> Vec<Entity> {
    let mut results = Vec::new();
    
    // Iterate through each size class
    for size_class in &hash.size_classes {
        // Skip empty size classes
        if size_class.entity_count == 0 {
            continue;
        }
        
        // Query Grid A
        let cells_a = size_class.grid_a.cells_in_radius(pos, search_radius);
        for &cell_entity in cells_a.iter().flat_map(|cell| cell.iter()) {
            if Some(cell_entity) != exclude_entity {
                results.push(cell_entity);
            }
        }
        
        // Query Grid B
        let cells_b = size_class.grid_b.cells_in_radius(pos, search_radius);
        for &cell_entity in cells_b.iter().flat_map(|cell| cell.iter()) {
            if Some(cell_entity) != exclude_entity {
                results.push(cell_entity);
            }
        }
    }
    
    results
}
```

**Optimization: Query grids can be parallelized:**
```rust
use rayon::prelude::*;

let results_a: Vec<Entity> = hash.size_classes.par_iter()
    .flat_map(|sc| sc.grid_a.query(...))
    .collect();
    
let results_b: Vec<Entity> = hash.size_classes.par_iter()
    .flat_map(|sc| sc.grid_b.query(...))
    .collect();
    
results_a.extend(results_b);
```

**With neighbor caching (already implemented):**
- 95% of queries use cached neighbor lists
- Only 5% perform actual spatial hash query
- 2× query cost (Grid A + Grid B) × 5% = **0.1× effective cost**

---

#### Memory Layout Optimization

**Interleaved storage for cache efficiency:**

```rust
struct StaggeredGrid {
    // Interleave Grid A and Grid B cells in same Vec
    // cells[i * 2] = Grid A cell i
    // cells[i * 2 + 1] = Grid B cell i
    cells_interleaved: Vec<Vec<Entity>>,
    // ...
}
```

**Benefits:**
- Grid A and Grid B likely to be in same cache lines
- Querying both grids has better spatial locality
- Reduces effective query overhead from 2× to ~1.3×

---

#### Performance Characteristics

**Memory:**
- 2× grid storage (Grid A + Grid B) per size class
- But: **25 bytes per entity** (vs 96 bytes multi-cell) - **saves 35 MB for 500k entities**
- Net: Slight increase in grid storage, massive decrease in component storage

**Update Performance:**
- **95%+ skip rate** (entities rarely cross grid midpoints)
- No divisions needed (just distance comparisons)
- Typical update threshold: 20-40 units (vs 10 units multi-cell)

**Query Performance:**
- 2× queries (Grid A + Grid B) per size class
- But: Neighbor caching makes this irrelevant (5% query rate)
- Can be parallelized (rayon)
- Interleaved memory improves cache locality

**Compared to Multi-Cell Approach:**

| Metric | Multi-Cell | Staggered Multi-Res | Improvement |
|--------|------------|---------------------|-------------|
| Component size | 96 bytes | 25 bytes | **4× smaller** |
| Update cost | 4 divisions + multi-cell calc | 2 distance checks | **10× faster** |
| Update frequency | Every 10 units | Every 20-40 units | **2-4× less frequent** |
| Query cost | 1× (single grid) | 2× (dual grids) | 2× slower (but cached) |
| Code complexity | High (multi-cell logic) | Low (single-cell) | **Much simpler** |

---

#### Example Use Cases

**Small RTS game (current):**
```ron
entity_radii: [0.5, 10.0]  // Units (radius 0.5), Static obstacles (radius ~10)
radius_to_cell_ratio: 80.0  // Large ratio for aggressive caching
```

Result:
- Size Class 0: cell_size=40 for units (Grid A + Grid B)
- Size Class 1: cell_size=800 for obstacles (Grid A + Grid B)

**Large-scale RTS:**
```ron
entity_radii: [0.5, 5.0, 25.0, 100.0]  // Units, vehicles, buildings, super-weapons
radius_to_cell_ratio: 4.0
```

Result:
- Size Class 0: cell_size=2 for tiny units
- Size Class 1: cell_size=20 for vehicles  
- Size Class 2: cell_size=100 for buildings
- Size Class 3: cell_size=400 for super-weapons

Each with Grid A and Grid B staggered.

---

#### Implementation Status

- ❌ **Not yet implemented** - current system uses multi-cell approach
- ✅ Neighbor caching already in place (critical for query performance)
- 🎯 Target for next major refactor
- 📈 Expected gains: 4× memory savings, 10× faster updates, simpler code

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

## 9. Spatial Query Performance Analysis & Findings

> **CRITICAL:** These performance issues affect **ALL proximity queries**, not just collision detection. As boids, combat systems, and AI are added, these bottlenecks will multiply. Current lag with 1-2k units is purely from collision queries - adding neighbor queries for boids will likely double the cost.

### 9.0 **DESIGN EVOLUTION: From Multi-Cell to Staggered Grids (January 2026)**

**Status:** RESOLVED through architecture redesign  
**Previous Problem:** Multi-cell storage complexity and large entity detection failures

**Historical Context:**

The original spatial hash used center-point insertion, which failed catastrophically with variable-sized entities:

**Example Failure:**
- Obstacle at `(55.96, 147.65)` with radius `19.74`
- Unit at `(55.51, 152.13)` with radius `0.50`  
- Distance: 4.5 units - **Unit clearly INSIDE obstacle**
- But: Collision NOT detected because unit's query couldn't reach obstacle's center!

**First Solution (Implemented):** Multi-Cell Storage
- Insert entities into ALL cells their radius overlaps
- Large obstacle (radius 20) → 100+ cells
- Component tracking: `Vec<(col, row, vec_idx)>` = 96 bytes
- Update logic: Complex symmetric difference calculations

**Problems with Multi-Cell:**
- Large memory overhead (96 bytes per entity)
- Complex update logic (calculate all cells, compare old vs new, update diff)
- Expensive for huge entities (radius 50 → 625 cells!)
- Stage 0/1 optimizations broke for multi-cell entities

**Final Solution (Current Design):** Staggered Multi-Resolution Grids
- Multiple cell sizes for different entity radii
- Each cell size has TWO offset grids (Grid A and Grid B)
- Entity inserted into whichever grid it's closest to center of
- **Always single-cell** - no entity occupies multiple cells

**Why Staggered Grids Work:**
```
Entity near boundary in Grid A → Near center in Grid B → Insert in Grid B
Entity near boundary in Grid B → Near center in Grid A → Insert in Grid A
Every entity is near-center in at least one grid!
```

**Benefits:**
- ✅ 25 bytes per entity (vs 96 bytes) - **4× memory savings**
- ✅ Trivial update logic (just distance comparison)
- ✅ No multi-cell complexity
- ✅ Works for all entity sizes (tiny to huge)
- ✅ Update threshold 2-4× larger (entities move further before update)

**Trade-off:**
- ❌ Must query both Grid A and Grid B (2× query cost)
- ✅ But neighbor caching makes this irrelevant (95% cache hit rate)

### 9.1 Known Performance Issues (January 2026)

#### Issue 1: Excessive Query Radius for Collision Detection
**Status:** CRITICAL - Major performance impact with 1-2k units  
**Affects:** Collision detection (current), boids neighbor queries (future), combat range queries (future)

**Problem:** 
The collision detection system uses `collision_search_radius_multiplier = 4.0`, meaning each unit queries a radius **4 times** its own collision radius (0.5 units). While collision detection is the only current user of the spatial hash, boids and combat systems will add similar or larger queries. This results in:
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
**Affects:** Any symmetric proximity query (collision, boids separation, mutual threat detection)

**Problem:**
The current collision detection uses entity ID comparison (`if entity > other_entity { continue; }`) to avoid checking the same pair twice. This pattern will be needed for boids (units checking neighbors symmetrically) and other systems. However:
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

#### Issue 7: Unit Pile-Up Creates O(N²) Hotspots
**Status:** CRITICAL - Observed in stress testing (January 2026)  
**Affects:** All proximity queries when units converge on same location

**Problem:**
When many units pathfind to the exact same target point, they create a massive spatial hotspot where collision detection degrades to O(N²) within that cluster.

**Evidence from Stress Testing (2150 units):**
- One entity had **1503 max neighbors** in spatial query
- This means 1000+ units converged at one location
- That single entity performed 1503 distance checks, layer checks, etc.
- While average neighbors was only ~23, the max created a massive spike

**Performance Impact:**
- Normal entity: 23 neighbors × cheap checks = ~microseconds
- Hotspot entity: 1503 neighbors × cheap checks = milliseconds **for one entity**
- This creates O(N²) behavior localized to the pile-up area
- Spatial hash cannot help when all entities are in the same cell

**Root Cause:**
Units pathfind to the exact same target position with no arrival spacing:
```rust
// Current behavior:
Unit A: target = (100, 100)
Unit B: target = (100, 100)  // EXACT SAME
Unit C: target = (100, 100)  // EXACT SAME
// Result: 1000 units pile up at (100, 100)
```

**Potential Solutions (DO NOT IMPLEMENT YET):**
1. **Arrival spacing:** Units stop ~1 unit before target if destination crowded
2. **Formation offsets:** Assign each unit slight offset from group target
3. **Adaptive collision:** Skip collision checks if > 500 neighbors detected
4. **Flow field improvement:** Flow field should spread units to nearby free cells

**Impact on Future Systems:**
This will also affect boids (1503 neighbors for separation/alignment/cohesion calculations) and combat (finding targets in massive blob).

---

### 9.2 Performance Logging

To diagnose spatial query performance issues, comprehensive logging has been added to all collision systems (the current primary users of the spatial hash). **This logging will also be useful when adding boids, combat, and other proximity-based systems** - the metrics track fundamental spatial query efficiency regardless of use case.

The logging is designed to be minimally invasive while providing actionable metrics.

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

> **Note:** These diagnostics apply to current collision systems. When boids/combat systems are added, similar logging patterns should be used with the same metrics (neighbors found, hit ratio, etc.).

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
- **CRITICAL:** Values > 1000 indicate catastrophic pile-up (all units at same point)
  - Example: 1350 max neighbors = 1350+ units stacked in same location
  - This causes O(N²) behavior for that cluster alone
  - Likely cause: Units pathfinding to exact same target point
- Solution: May need adaptive collision (skip checks in extremely dense areas)

**Symptom: Low actual collisions despite lag**
- Check `hit ratio`
- If < 0.5%: Collision search radius is far too large for actual density
- If 1-2%: Search radius is moderately excessive (current observed behavior)
- If < 5%: Consider reducing `collision_search_radius_multiplier` in config
- Ideal: 5-10% hit ratio means most proximity checks find actual collisions

#### Real-World Example (January 2026, 2150 units)

From actual stress testing logs:
```
[COLLISION_DETECT] 1.83ms | Entities: 2150 | Neighbors: 49337 (avg: 22.9, max: 1350) | 
Potential checks: 49337 | Duplicate skips: 26035 | Layer filtered: 0 | 
Actual collisions: 852 | Hit ratio: 1.73% | Search radius multiplier: 4.0x
```

**Analysis:**
- **Hit ratio of 1.73%**: Only ~2 out of 100 proximity checks find actual collisions (98% wasted)
- **Max neighbors = 1350**: One entity checked 1350+ neighbors (massive pile-up at one location)
- **Avg neighbors = 22.9**: Most entities have reasonable density (~23 neighbors)
- **Duplicate skips = 52%**: Expected behavior (checking pairs twice)

**Diagnosis:** 
- Main issue: Spatial hotspot where 1000+ units converged on same point
- Secondary issue: 4.0x search radius is 2-3x larger than needed (should target 5-10% hit ratio)
- Free obstacle checks (105k) are brute-force but only take ~340µs (not primary bottleneck)

**Recommended Actions:**
1. **Immediate:** Investigate why units pile up (likely pathfinding to exact same target)
2. **Short-term:** Reduce `collision_search_radius_multiplier` from 4.0 to 2.5-3.0
3. **Long-term:** Implement arrival spacing (units stop short when destination crowded)

---

## 10. Proposed Performance Optimizations

This section provides concrete, actionable solutions to the performance issues identified in Section 9. Solutions are ranked by **impact × ease of implementation**.

### 10.1 Quick Wins (Low Effort, High Impact)

#### Solution 1A: Reduce Collision Search Radius Multiplier
**Addresses:** Issue #1 (Excessive Query Radius)  
**Impact:** 🔥🔥🔥 High - Could reduce collision checks by 50-75%  
**Effort:** ⚡ Trivial - One config value change  
**Risk:** 🟢 Low - Easy to test and revert

**Current State:**
- `collision_search_radius_multiplier = 4.0`
- Query radius = 2.0 units (0.5 × 4.0)
- Checks 9 grid cells (3×3) on average

**Proposed Change:**
```ron
// In initial_config.ron
collision_search_radius_multiplier: 2.5  // Was 4.0
```

**Expected Results:**
- Query radius = 1.25 units (0.5 × 2.5)
- Query area reduced by 4× (π × 1.25² vs π × 2²)
- Checks 4-9 grid cells instead of always 9
- Hit ratio should increase from 1.73% to 4-5%
- **Estimated speedup: 2-3× for collision detection**

**Testing:**
1. Change value to 2.5
2. Spawn 2000 units
3. Verify no units "phase through" each other
4. If stable, try 2.0; if units clip, go back to 2.5

**Acceptance Criteria:**
- No visible unit overlap/clipping
- Hit ratio increases to 3-5%
- Collision detection time drops below 1ms for 2000 units

---

#### Solution 1B: Arrival Spacing to Prevent Pile-Ups
**Addresses:** Issue #7 (Unit Pile-Up)  
**Impact:** 🔥🔥🔥🔥 Critical - Eliminates O(N²) hotspots  
**Effort:** ⚡⚡ Low-Medium - Modify movement system  
**Risk:** 🟡 Medium - Could affect gameplay feel

**Current Problem:**
```rust
// All units converge on exact same point
Unit A target: (100.0, 100.0)
Unit B target: (100.0, 100.0)
Unit C target: (100.0, 100.0)
// Result: 1000+ units stacked → 1503 neighbor checks
```

**Proposed Implementation:**
Add "soft arrival" logic to `follow_path` system:

```rust
// In follow_path system
const ARRIVAL_RADIUS: FixedNum = FixedNum::from_num(0.5); // Stop 0.5 units from target
const CROWDING_THRESHOLD: usize = 50; // Number of nearby units to consider "crowded"

if distance_to_target < ARRIVAL_RADIUS {
    // Check if destination is crowded
    let nearby_stopped_units = spatial_hash.query_radius(
        entity, 
        target_pos, 
        FixedNum::from_num(2.0)
    ).iter()
    .filter(|(e, _)| has_no_path(*e))  // Count only stopped units
    .count();
    
    if nearby_stopped_units > CROWDING_THRESHOLD {
        // Destination crowded - consider self "arrived" early
        path.is_complete = true;
        commands.entity(entity).remove::<Path>();
        // Apply braking
        acceleration = -velocity * braking_force;
    }
}
```

**Expected Results:**
- Units stop in ~2 unit radius around target instead of exact point
- Max neighbors drops from 1500 → 50-100
- Forms organic "blob" around destination instead of pile
- **Estimated speedup: 10-20× for hotspot entities**

**Side Effects:**
- Units won't reach exact target position (stop ~0.5-2 units away)
- May need to adjust for precision-critical systems (building construction, etc.)
- Formation will be circular blob instead of tight cluster

**Testing:**
1. Add logging for "early arrival due to crowding"
2. Spawn 2000 units to same target
3. Verify max neighbors drops below 200
4. Check if arrival feels natural

---

### 10.2 Medium-Term Improvements (Moderate Effort, High Impact)

#### Solution 2A: Cache Spatial Queries (Temporal Coherence)
**Addresses:** Issue #1 (Excessive Query Radius) - Alternative approach  
**Impact:** 🔥🔥🔥 High - Could reduce queries by 90%+  
**Effort:** ⚡⚡⚡ Medium - New component + invalidation logic  
**Risk:** 🟡 Medium - Cache invalidation bugs could cause missed collisions

**Key Insight:**
Entities move slowly between frames. At 30 fps with speed 10 units/sec:
- Movement per frame: ~0.33 units
- Search radius: 2.0 units
- **95% of neighbors from frame N are still neighbors in frame N+1**

Instead of querying spatial hash every frame, **cache neighbor lists and incrementally update**.

**Implementation:**

```rust
#[derive(Component)]
struct CachedNeighbors {
    neighbors: Vec<(Entity, FixedVec2)>,
    last_query_pos: FixedVec2,
    frames_since_update: u32,
}

const CACHE_UPDATE_THRESHOLD: FixedNum = FixedNum::from_num(0.5); // Update if moved > 0.5 units
const MAX_FRAMES_BEFORE_REFRESH: u32 = 10; // Force refresh every 10 frames

fn update_neighbor_cache(
    mut query: Query<(Entity, &SimPosition, &mut CachedNeighbors, &Collider)>,
    spatial_hash: Res<SpatialHash>,
    sim_config: Res<SimConfig>,
) {
    for (entity, pos, mut cache, collider) in query.iter_mut() {
        cache.frames_since_update += 1;
        
        let moved_distance = (pos.0 - cache.last_query_pos).length();
        let needs_update = moved_distance > CACHE_UPDATE_THRESHOLD 
                        || cache.frames_since_update > MAX_FRAMES_BEFORE_REFRESH;
        
        if needs_update {
            // Full spatial query (expensive)
            let search_radius = collider.radius * sim_config.collision_search_radius_multiplier;
            cache.neighbors = spatial_hash.get_potential_collisions(
                pos.0, 
                search_radius, 
                Some(entity)
            );
            cache.last_query_pos = pos.0;
            cache.frames_since_update = 0;
        }
        // Otherwise: Reuse cached neighbors from last frame
    }
}

fn detect_collisions_cached(
    mut commands: Commands,
    query: Query<(Entity, &SimPosition, &CachedNeighbors, &Collider)>,
    sim_config: Res<SimConfig>,
    mut events: MessageWriter<CollisionEvent>,
) {
    for (entity, pos, cache, collider) in query.iter() {
        // Use cached neighbor list instead of spatial query!
        for &(other_entity, other_cached_pos) in &cache.neighbors {
            if entity > other_entity { continue; }
            
            // NOTE: Use current position from query, not cached position
            if let Ok((_, other_pos, _, other_collider)) = query.get(other_entity) {
                // Layer check
                if (collider.mask & other_collider.layer) == 0 
                    && (other_collider.mask & collider.layer) == 0 {
                    continue;
                }
                
                // Narrow phase collision check (same as before)
                let min_dist = collider.radius + other_collider.radius;
                let min_dist_sq = min_dist * min_dist;
                let delta = pos.0 - other_pos.0;
                let dist_sq = delta.length_squared();
                
                if dist_sq < min_dist_sq {
                    // Collision detected - emit event...
                }
            }
        }
    }
}
```

**Expected Results:**
- **90% reduction in spatial hash queries**
  - Only query when moved > 0.5 units or every 10 frames
  - Most frames: Use cached neighbor list
- **Estimated speedup: 3-5× for collision detection**
  - Spatial hash queries iterate multiple cells (expensive)
  - Cached array lookup is ~100× faster
- From your logs: 1.8ms → 0.4-0.6ms per frame

**Performance Analysis (2000 units):**

Without cache:
```
Every frame: 2000 spatial queries × 1µs = 2ms
```

With cache (90% hit rate):
```
10% frames: 200 spatial queries × 1µs = 0.2ms  (cache miss - moved)
90% frames: 0 spatial queries = 0ms            (cache hit)
Average: 0.2ms (10× faster!)
```

**Determinism Considerations:**
✅ **Safe for determinism** if:
- Cache invalidation uses FixedNum (no floating-point)
- Updates happen in deterministic query iteration order
- Same movement → same cache invalidation across machines

**Trade-offs:**
- **Memory:** ~200 bytes per entity (2000 entities = 400KB - negligible)
- **Stale neighbors:** Cache may include entities that moved away
  - ✅ Mitigation: Distance check in narrow phase catches this
- **Missing neighbors:** New entities that entered radius won't be detected until cache refresh
  - ⚠️ Risk: Could miss fast-moving entities for up to 10 frames
  - ✅ Mitigation: Force refresh every 10 frames (0.33 seconds @ 30fps)
- **Complexity:** More state to manage

**Advanced Variant: Incremental Updates**

Track which neighbors moved out of range:

```rust
fn update_neighbor_cache_incremental(
    mut query: Query<(Entity, &SimPosition, &mut CachedNeighbors, &Collider)>,
    spatial_hash: Res<SpatialHash>,
    position_tracker: Res<PreviousPositions>,  // Track all entities' last positions
) {
    for (entity, pos, mut cache, collider) in query.iter_mut() {
        let mut needs_refresh = false;
        
        // Check if any cached neighbor moved significantly
        for &(neighbor_entity, cached_neighbor_pos) in &cache.neighbors {
            if let Some(prev_pos) = position_tracker.get(neighbor_entity) {
                let neighbor_moved = (*prev_pos - cached_neighbor_pos).length();
                if neighbor_moved > CACHE_UPDATE_THRESHOLD {
                    needs_refresh = true;
                    break;
                }
            }
        }
        
        if needs_refresh || /* self moved */ {
            // Re-query spatial hash
        }
    }
}
```

**Advanced Variant: Velocity-Aware Caching**

Track entity velocities and update fast movers more frequently:

```rust
#[derive(Component)]
struct CachedNeighbors {
    neighbors: Vec<(Entity, FixedVec2)>,
    last_query_pos: FixedVec2,
    frames_since_update: u32,
    is_fast_mover: bool,  // Track if this entity moves quickly
}

const FAST_MOVER_SPEED_THRESHOLD: FixedNum = FixedNum::from_num(8.0); // units/sec
const NORMAL_UPDATE_THRESHOLD: FixedNum = FixedNum::from_num(0.5);
const FAST_MOVER_UPDATE_THRESHOLD: FixedNum = FixedNum::from_num(0.2);
const MAX_FRAMES_NORMAL: u32 = 10;
const MAX_FRAMES_FAST: u32 = 2; // Fast movers refresh every 2 frames

fn update_neighbor_cache_velocity_aware(
    mut query: Query<(Entity, &SimPosition, &SimVelocity, &mut CachedNeighbors, &Collider)>,
    spatial_hash: Res<SpatialHash>,
    sim_config: Res<SimConfig>,
) {
    for (entity, pos, velocity, mut cache, collider) in query.iter_mut() {
        cache.frames_since_update += 1;
        
        // Classify entity by speed
        let speed = velocity.0.length();
        cache.is_fast_mover = speed > FAST_MOVER_SPEED_THRESHOLD;
        
        // Use different thresholds based on movement speed
        let (distance_threshold, max_frames) = if cache.is_fast_mover {
            (FAST_MOVER_UPDATE_THRESHOLD, MAX_FRAMES_FAST)
        } else {
            (NORMAL_UPDATE_THRESHOLD, MAX_FRAMES_NORMAL)
        };
        
        let moved_distance = (pos.0 - cache.last_query_pos).length();
        let needs_update = moved_distance > distance_threshold 
                        || cache.frames_since_update > max_frames;
        
        if needs_update {
            // Full spatial query
            let search_radius = collider.radius * sim_config.collision_search_radius_multiplier;
            cache.neighbors = spatial_hash.get_potential_collisions(
                pos.0, 
                search_radius, 
                Some(entity)
            );
            cache.last_query_pos = pos.0;
            cache.frames_since_update = 0;
        }
    }
}
```

**Performance Impact:**
- **Slow/stopped units** (most of the time): Update every ~10 frames (90% cache hit)
- **Fast-moving units** (projectiles, charging units): Update every 2 frames (50% cache hit)
- Adaptive: Units automatically switch categories as they accelerate/decelerate

**Example Scenario (2000 units, 200 fast movers):**
```
Slow movers: 1800 × 10% update rate = 180 queries/frame
Fast movers:  200 × 50% update rate = 100 queries/frame
Total: 280 queries/frame (vs 2000 without cache)

Speedup: 7× (vs 3-5× with fixed threshold)
```

**Additional Benefit:**
- No need for forced refresh every N frames
- Fast movers naturally get fresher cache data
- Stopped units can keep cache for very long time (great for idle formations)

**Enhanced Version: Track Neighbors' Velocities**

Go even further - track if cached neighbors are fast movers too:

```rust
#[derive(Component)]
struct CachedNeighbors {
    neighbors: Vec<(Entity, FixedVec2, bool)>,  // Added: is_fast_mover flag
    // ...
}

fn update_neighbor_cache_bidirectional(
    mut query: Query<(Entity, &SimPosition, &SimVelocity, &mut CachedNeighbors, &Collider)>,
    velocity_lookup: Query<&SimVelocity>,
    spatial_hash: Res<SpatialHash>,
) {
    for (entity, pos, velocity, mut cache, collider) in query.iter_mut() {
        let moved = (pos.0 - cache.last_query_pos).length();
        
        // Check if any cached NEIGHBOR is a fast mover
        let has_fast_neighbor = cache.neighbors.iter()
            .any(|(_, _, is_fast)| *is_fast);
        
        // Update if:
        // - Self moved significantly
        // - Self is fast mover
        // - Any neighbor is fast mover (they might leave our radius)
        let needs_update = moved > NORMAL_UPDATE_THRESHOLD
                        || velocity.0.length() > FAST_MOVER_SPEED_THRESHOLD
                        || has_fast_neighbor;
        
        if needs_update {
            // Query spatial hash
            let neighbors_raw = spatial_hash.get_potential_collisions(...);
            
            // Tag each neighbor with their speed classification
            cache.neighbors = neighbors_raw.into_iter()
                .map(|(e, pos)| {
                    let is_fast = velocity_lookup.get(e)
                        .map(|v| v.0.length() > FAST_MOVER_SPEED_THRESHOLD)
                        .unwrap_or(false);
                    (e, pos, is_fast)
                })
                .collect();
        }
    }
}
```

**When This Matters:**
- **Projectiles:** Moving 50+ units/sec, need fresh cache every frame
- **Charging units:** Brief acceleration bursts
- **Idle formations:** 1000 stopped units can share stale cache for seconds

**Determinism Note:**
✅ Safe - velocity classification is deterministic (same velocities across machines)

**Recommendation:**
Start with simple velocity-aware version. The bidirectional tracking is probably overkill unless you have thousands of projectiles.

---

#### Solution 2B: Eliminate Duplicate Pair Checks
**Addresses:** Issue #2 (Duplicate Entity Pair Checks)  
**Impact:** 🔥🔥 Medium - Cuts narrow-phase checks by ~50%  
**Effort:** ⚡⚡⚡ Medium - Refactor collision detection  
**Risk:** 🟢 Low - Well-understood pattern

**Current Inefficiency:**
```rust
// Current: Both directions checked, half discarded
Unit A queries → finds B → processes (A, B)
Unit B queries → finds A → SKIP (already did A-B)
                           ↑ Wasted spatial query + iteration
```

**Proposed: Spatial Sweep Algorithm:**
```rust
fn detect_collisions(
    mut commands: Commands,
    query: Query<(Entity, &SimPosition, &Collider)>,
    spatial_hash: Res<SpatialHash>,
    // ...
) {
    // Collect all entities and sort by X coordinate
    let mut entities: Vec<_> = query.iter()
        .map(|(e, pos, col)| (e, pos.0, col))
        .collect();
    
    entities.sort_by(|a, b| a.1.x.cmp(&b.1.x));
    
    // Sweep: Only check entities "to the right"
    for i in 0..entities.len() {
        let (entity, pos, collider) = entities[i];
        
        // Only query entities not yet processed (j > i)
        // Spatial hash returns all, but we only process those ahead in sweep
        let potential = spatial_hash.get_potential_collisions(pos, search_radius, Some(entity));
        
        for (other, other_pos) in potential {
            // Only process if other is ahead in sweep order (not yet checked)
            if other_pos.x <= pos.x { continue; }  // Already processed this pair
            
            // Narrow phase collision check...
        }
    }
}
```

**Expected Results:**
- Eliminates 26,000 duplicate skips (from your logs)
- Reduces cache pressure from iterating discarded neighbors
- **Estimated speedup: 1.3-1.5× for collision detection**

**Trade-offs:**
- Additional sort step (~O(N log N), but N is small and cache-friendly)
- More complex code (harder to understand)

---

#### Solution 2C: Integrate Free Obstacles into Spatial Hash
**Addresses:** Issue #4 (O(N×M) Free Obstacle Checking)  
**Impact:** 🔥 Low-Medium - Currently only ~340µs, but scales poorly  
**Effort:** ⚡⚡ Low-Medium - Modify spatial hash insertion  
**Risk:** 🟢 Low - Straightforward change

**Current Problem:**
```rust
// 2100 units × 50 obstacles = 105,000 checks
for each unit:
    for each free_obstacle:  // No spatial partitioning!
        check_collision()
```

**Proposed Change:**
Insert static obstacles into spatial hash at initialization:

```rust
// In update_spatial_hash system
fn update_spatial_hash(
    mut spatial_hash: ResMut<SpatialHash>,
    dynamic_query: Query<(Entity, &SimPosition), Without<StaticObstacle>>,
    static_query: Query<(Entity, &SimPosition), With<StaticObstacle>>,  // NEW
) {
    spatial_hash.clear();
    
    // Insert dynamic entities
    for (entity, pos) in dynamic_query.iter() {
        spatial_hash.insert(entity, pos.0);
    }
    
    // Insert static obstacles (these don't move, but we rebuild anyway for simplicity)
    for (entity, pos) in static_query.iter() {
        spatial_hash.insert(entity, pos.0);
    }
}
```

Then in collision detection, obstacles are automatically returned by spatial queries.

**Expected Results:**
- Free obstacle checks drop from O(N×M) to O(N)
- 105,000 checks → ~1,000 checks (only nearby obstacles queried)
- **Estimated speedup: ~10× for obstacle collision (340µs → 30µs)**

**Trade-offs:**
- Spatial hash size increases slightly (50 more entities)
- Need to distinguish dynamic vs static in collision response

---

### 10.3 Advanced Optimizations (High Effort, High Impact)

#### Solution 3A: Adaptive Collision - Skip Hotspots
**Addresses:** Issue #7 (Unit Pile-Up) - Alternative to arrival spacing  
**Impact:** 🔥🔥🔥 High - Prevents O(N²) worst case  
**Effort:** ⚡⚡⚡⚡ High - New system, tuning required  
**Risk:** 🔴 High - Could cause visible clipping in dense areas

**Concept:**
If a unit detects it's in an extreme hotspot (> 500 neighbors), **skip collision checks entirely** for that entity.

```rust
// In detect_collisions
for (entity, pos, collider) in query.iter() {
    let potential = spatial_hash.get_potential_collisions(pos, search_radius, Some(entity));
    
    // HOTSPOT DETECTION
    if potential.len() > 500 {
        warn!("Entity {:?} in hotspot ({} neighbors) - skipping collision checks", 
              entity, potential.len());
        continue;  // Skip collision checks for this entity
    }
    
    // Normal collision detection...
}
```

**Expected Results:**
- Prevents single entity from doing 1500+ collision checks
- Caps worst-case collision detection at O(500) per entity
- **Prevents frame time spikes > 50ms**

**Trade-offs:**
- Units in hotspot may overlap/clip through each other
- Visually acceptable if hotspot is a chaotic blob anyway
- Could cause gameplay issues (units stuck inside each other)

**Recommendation:** Use arrival spacing (Solution 1B) instead - prevents hotspots rather than working around them.

---

#### Solution 3B: Adaptive Cell Size for Spatial Hash
**Addresses:** Issue #3 (Cell Size Mismatch)  
**Impact:** 🔥🔥 Medium - Better cache locality  
**Effort:** ⚡⚡⚡⚡⚡ Very High - Major refactor  
**Risk:** 🔴 High - Complex implementation

**Current:** Fixed 2.0 unit cells across entire map  
**Proposed:** Quadtree/octree with adaptive subdivision

```rust
// Pseudocode - NOT actual implementation
struct AdaptiveSpatialHash {
    root: QuadTreeNode,
}

struct QuadTreeNode {
    bounds: Rect,
    entities: Vec<Entity>,
    children: Option<Box<[QuadTreeNode; 4]>>,
}

impl QuadTreeNode {
    fn insert(&mut self, entity: Entity, pos: FixedVec2) {
        if self.entities.len() > MAX_ENTITIES_PER_NODE {
            // Subdivide into 4 quadrants
            self.subdivide();
        }
        // Insert into appropriate child...
    }
}
```

**Expected Results:**
- Dense areas get smaller cells (better precision)
- Sparse areas get larger cells (less overhead)
- Query efficiency improves by 2-3×

**Recommendation:** Not worth the complexity for current scale. Consider at 10k+ units.

---

### 10.4 Recommended Implementation Order

Based on impact vs effort:

**Phase 1: Quick Wins (1-2 days)**
1. ✅ **Solution 1A:** Reduce `collision_search_radius_multiplier` to 2.5 (5 min)
2. ✅ **Solution 1B:** Implement arrival spacing for crowding (2-4 hours)
3. ✅ **Solution 2A:** Spatial query caching (3-5 hours)
4. ✅ Test with 2000-5000 units, measure improvement

**Expected Result:** 20-30× improvement in worst-case frame time (60ms → 2-3ms)
- Config + spacing: 60ms → 6-12ms (10× improvement)  
- Add caching: 6-12ms → 2-3ms (additional 3-5× improvement)

**Phase 2: Polish (2-3 days) - Only if targeting 10k+ units**
5. ✅ **Solution 2B:** Eliminate duplicate pair checks (spatial sweep)
6. ✅ **Solution 2C:** Insert free obstacles into spatial hash
7. ✅ Test with 10000+ units

**Expected Result:** Additional 1.5-2× improvement (2-3ms → 1-2ms)

**Phase 3: Future (when needed)**
8. ⏸️ **Solution 3A:** Adaptive collision (only if pile-ups still occur)
9. ⏸️ **Solution 3B:** Adaptive spatial hash (only at 50k+ units)

**Priority Ranking by Impact:**
1. 🔥🔥🔥🔥 **Solution 2A (Caching)** - Biggest single win, eliminates 90% of queries
2. 🔥🔥🔥 **Solution 1B (Arrival spacing)** - Eliminates O(N²) hotspots  
3. 🔥🔥🔥 **Solution 1A (Reduce radius)** - Simple but effective
4. 🔥🔥 **Solution 2B (Eliminate duplicates)** - Good cleanup
5. 🔥 **Solution 2C (Spatial obstacles)** - Minor (not current bottleneck)

---

## 11. Future Improvements
*   **Hard Collisions:** For gameplay reasons, we might want "hard" collisions that absolutely prevent overlap (e.g., for blocking units).
*   **Mass/Weight:** Currently all units have equal weight. We may add mass so tanks can push infantry.
*   **Push Priority:** Moving units should push idle units out of the way (bumping).
