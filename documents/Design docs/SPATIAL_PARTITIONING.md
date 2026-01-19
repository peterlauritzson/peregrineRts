# Peregrine Spatial Partitioning & Proximity Systems

This document details the spatial partitioning infrastructure and its various use cases: proximity queries, collision detection/resolution, boids flocking, combat targeting, and AI systems.

> **Architectural Note:** The spatial hash is a **general-purpose proximity query engine** for all gameplay systems. Physical collision detection is just one of many use cases. This system will also power boids flocking, enemy detection, attack range queries, area-of-effect abilities, and more.

## 1. Core Philosophy
*   **Determinism:** All physics calculations use fixed-point arithmetic (`FixedNum`, `FixedVec2`) to ensure identical results across different machines (crucial for RTS lockstep networking).
*   **Performance:** We prioritize throughput (10M+ units) over perfect physical accuracy. Collisions are "soft" (separation forces) rather than rigid body solves.
*   **Zero-Allocation Hot Paths:** All data structures are pre-allocated at startup. No runtime allocation in critical systems to eliminate performance spikes.
*   **Simplicity:** Units are treated as circles with a fixed radius.
*   **Generality:** The proximity query system supports multiple use cases beyond just collision detection.
*   **Memory Efficiency:** Target 10M entities with <500MB spatial hash memory footprint (vs 30GB for naive approaches).

## 2. Spatial Partitioning: The Foundation

To avoid $O(N^2)$ proximity checks (which would require 100 billion comparisons for 10,000 units), we use a **Spatial Hash Grid** as the core spatial partitioning structure.

### 2.1 Structure
*   **Grid:** A 2D grid covering the map. Each cell contains a list of Entity IDs and positions.
*   **Cell Size:** Tuned based on typical query radius (usually 2-3x unit radius).
*   **Dynamic:** Rebuilt every physics tick as entities move.

### 2.2 Storage Strategy: Arena-Based Staggered Multi-Resolution Grids

**CRITICAL DESIGN DECISION (January 2026):**

The spatial hash uses a **zero-allocation arena-based architecture** with:
1. **Multiple cell sizes** handle entities of different radii (small units vs huge obstacles)
2. **Each cell size has TWO offset grids** (Grid A and Grid B) with centers staggered by half_cell
3. **Entities are always single-cell** - inserted into whichever grid they're closest to the center of
4. **Arena storage per grid** - all entities stored in one pre-allocated Vec, cells track ranges
5. **Deferred structural updates** - hot paths never allocate, cold paths run async

This eliminates both multi-cell complexity AND runtime allocation, enabling 10M+ entity scale.

#### 2.2.1 Arena Storage Architecture

**The Performance Problem:**

Traditional spatial hash implementations use `Vec<Vec<Entity>>` - one Vec per cell. This causes:
- **Runtime reallocation spikes:** When a cell Vec hits capacity, Rust allocates 2× buffer and copies all entities
- **Memory fragmentation:** Thousands of small allocations scattered across heap
- **Cache misses:** Entities in adjacent cells are not memory-adjacent
- **Observed in testing:** 66ms spikes (4× normal) at 500k entities due to Vec reallocation

**The Solution: Multi-Resolution Staggered Grids with Arena Storage**

**Three Key Architectural Decisions:**

1. **Arena Storage (Zero Allocation)**: Each grid uses ONE preallocated Vec for all entities
2. **Staggered Grids (Rare Updates)**: Two offset grids (A + B) per size class eliminate boundary issues
3. **Multi-Resolution (Cache Isolation)**: Separate arenas per entity size for perfect cache locality

```rust
struct SpatialHash {
    // Multiple size classes for different entity sizes
    size_classes: Vec<SizeClass>,  // Typically 3: small/medium/large
}

struct SizeClass {
    // TWO staggered grids per size class (Grid A + Grid B)
    grid_a: StaggeredGrid,  // Centers at (0, cell_size, 2×cell_size, ...)
    grid_b: StaggeredGrid,  // Centers at (cell_size/2, 3×cell_size/2, ...)
}

struct StaggeredGrid {
    // ARENA: One big preallocated Vec for all entities in this grid
    // CRITICAL: Vec::with_capacity() at startup, NEVER REALLOCATE during gameplay
    entity_storage: Vec<Entity>,      // 10M capacity = 80MB per grid
    
    // METADATA: Each cell tracks which range of entity_storage it owns
    cell_ranges: Vec<CellRange>,       // One per cell (8 bytes each)
    
    offset: FixedVec2,  // Grid A: (0,0), Grid B: (cell_size/2, cell_size/2)
}

#[derive(Copy, Clone)]
struct CellRange {
    start_index: usize,    // Index into entity_storage where this cell begins
    current_count: usize,  // Number of entities in this cell
}

// Note: For incremental updates (Section 2.8.2), we extend this with:
// max_index: usize  // Maximum index (exclusive) for overflow detection
// See the full version in Section 2.8.2 below.

**CRITICAL: Zero-Allocation Query Design**

**⚠️ ABSOLUTE REQUIREMENT: Queries MUST return slice views (`&[Entity]`), NEVER allocate!**

The entire spatial hash architecture is designed around **preallocated memory**. This includes:
- ✅ Arena storage: Preallocated at startup
- ✅ Query results: Return **slice views** (`&[Entity]`), NOT owned `Vec<Entity>`
- ✅ Scratch buffers: Preallocated, cleared and reused

**Rust Memory Types for Arena Storage:**

```rust
// Option 1: Vec<Entity> - Growable (what we use)
entity_storage: Vec<Entity>,  // Can push/pop, capacity can grow
// Initialized: Vec::with_capacity(10_000_000)
// We NEVER grow it during gameplay (strict capacity limits)

// Option 2: Box<[Entity]> - Fixed-size heap array
entity_storage: Box<[Entity]>,  // Fixed capacity, immutable size
// Initialized: vec![Entity::PLACEHOLDER; 10_000_000].into_boxed_slice()
// Guarantees no reallocation (size is immutable)
// Downside: Harder to manage partial fills

// Both allow zero-copy slicing: &entity_storage[start..end]
```

**We use `Vec<Entity>` but treat it like `Box<[Entity]>` with strict capacity enforcement.**

**How Zero-Allocation Queries Work:**

When a query requests entities in a cell, we return a **slice reference** `&[Entity]`:

```rust
impl StaggeredGrid {
    /// ZERO-ALLOCATION QUERY: Returns immutable view of cell's entities
    /// 
    /// CRITICAL: This returns &[Entity], which is just a fat pointer (16 bytes):
    ///   - 8 bytes: pointer to data in entity_storage
    ///   - 8 bytes: length (number of entities)
    /// 
    /// NO HEAP ALLOCATION. NO COPYING. Just a view into existing memory.
    pub fn query_cell(&self, col: usize, row: usize) -> &[Entity] {
        let cell_idx = row * self.cols + col;
        let range = &self.cell_ranges[cell_idx];
        
        // Slice operator creates a fat pointer view (zero-copy!)
        &self.entity_storage[range.start_index .. range.start_index + range.current_count]
        //                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        //                    This is a VIEW, not an allocation!
        //                    Returns &[Entity] = (ptr: *const Entity, len: usize)
    }
}
```

**What happens at the assembly level:**

```rust
let entities: &[Entity] = grid.query_cell(10, 20);
// Assembly (simplified):
//   1. Calculate offset: cell_idx = row * cols + col
//   2. Load cell metadata: range = cell_ranges[cell_idx]
//   3. Calculate pointer: ptr = entity_storage.as_ptr() + range.start_index
//   4. Return fat pointer: (ptr, range.current_count)
// 
// ZERO ALLOCATIONS. Just pointer arithmetic.
```

**Returning a "view" vs "owned data":**

```rust
// ❌ WRONG - ALLOCATES MEMORY (kills performance!)
fn query_cell_WRONG(&self, col: usize, row: usize) -> Vec<Entity> {
    let range = &self.cell_ranges[cell_idx];
    
    // This allocates a NEW Vec and COPIES all entities!
    self.entity_storage[range.start_index .. range.start_index + range.current_count]
        .to_vec()  // ❌ HEAP ALLOCATION + MEMCPY
}

// ✅ CORRECT - ZERO ALLOCATION
fn query_cell(&self, col: usize, row: usize) -> &[Entity] {
    let range = &self.cell_ranges[cell_idx];
    
    // Slice creates a view - just returns (pointer, length)
    &self.entity_storage[range.start_index .. range.start_index + range.current_count]
    // ✅ NO ALLOCATION, NO COPY - just a 16-byte fat pointer
}
```

**Why `&[Entity]` is perfect for queries:**

1. **Zero-copy:** Caller gets direct read access to arena memory
2. **Safe:** Rust borrow checker ensures no one modifies arena while slice exists
3. **Efficient:** Just two machine words (pointer + length), passed in registers
4. **Iterator-ready:** `for entity in entities.iter()` works directly

**Performance Impact:**

| Approach | Allocation | Copy Cost | Typical Latency |
|----------|------------|-----------|-----------------|
| **Return `&[Entity]`** (correct) | 0 bytes | 0 bytes | **~5ns** (pointer math) |
| Return `Vec<Entity>` (wrong) | 8-32 bytes (Vec header) | 8 × count bytes | ~500ns + 1ns/entity |

For a cell with 100 entities:
- `&[Entity]`: **~5ns**
- `Vec<Entity>`: ~600ns (120× slower) + heap fragmentation
```

**Architecture Benefits:**

| Benefit | Traditional Vec-Per-Cell | Arena + Staggered + Multi-Res |
|---------|-------------------------|-------------------------------|
| **Memory** | 9.6 GB (naive) | 790 MB (12× better) |
| **Reallocation Spikes** | 66ms at 500k entities | Zero (preallocated) |
| **Update Frequency** | Every ~10 units | Every ~20-40 units (staggered) |
| **Cache Locality** | Poor (scattered Vecs) | Excellent (contiguous + isolated) |
| **Query Cost** | 1× (single grid) | 2× (A + B), but cached |
| **Code Complexity** | Multi-cell logic | Single-cell everywhere |

**Memory Layout Example (4 cells, 10 entities):**

```
entity_storage: [E1, E2, E3, E4, E5, E6, E7, E8, E9, E10, _, _, _, ...]
                 └──────┘  └────────┘  └────┘  └──┘
                 Cell 0    Cell 1      Cell 2  Cell 3

cell_ranges[0]: CellRange { start_index: 0, current_count: 3 }
cell_ranges[1]: CellRange { start_index: 3, current_count: 4 }
cell_ranges[2]: CellRange { start_index: 7, current_count: 2 }
cell_ranges[3]: CellRange { start_index: 9, current_count: 1 }
```

**Benefits:**
- ✅ **Zero runtime allocation** - preallocated once at startup
- ✅ **Cache-friendly** - entities in spatial proximity are memory-adjacent
- ✅ **Predictable performance** - no reallocation spikes
- ✅ **Memory efficient** - no per-cell Vec overhead (24 bytes each)
- ✅ **Scales to 10M entities** - 80MB for entity storage vs 3GB for fixed-capacity-per-cell

#### 2.2.2 Three-Phase Update Architecture

To maintain zero-allocation hot paths, updates are split into three phases:

**PHASE 1: Hot Path (Every Tick, <1ms target)** - Query Spatial Hash
```rust
// Called by collision detection, boids, AI - MUST be fast
// 
// ⚠️⚠️⚠️ CRITICAL: Returns &[Entity] slice view - ZERO ALLOCATION! ⚠️⚠️⚠️
fn query_radius(&self, pos: FixedVec2, radius: FixedNum) -> &[Entity] {
    let cell_idx = self.pos_to_cell(pos);
    let range = &self.cell_ranges[cell_idx];
    
    // ZERO allocation - just slice the storage array
    // This returns a fat pointer (ptr, len) - NO heap allocation!
    &self.entity_storage[range.start_index .. range.start_index + range.current_count]
}
```
- **ZERO ALLOCATION** - `&[Entity]` is just a view (pointer + length)
- No iteration (direct array slice)
- Target: <0.5ms for 500k entities
- **Performance: ~5ns per query** (just pointer arithmetic)

**Why This Is Fast:**

```rust
// What the CPU actually does:
// 1. Load cell_idx from function parameter
// 2. Load cell_ranges[cell_idx] from memory (2 usizes = 16 bytes)
// 3. Calculate: ptr = entity_storage_base + (start_index * 8)
// 4. Return: (ptr, current_count)
// 
// Total: ~5 CPU instructions, ~5ns on modern hardware
// NO MALLOC, NO MEMCPY, NO HEAP TOUCHING
```

**PHASE 2: Warm Path (Every Tick, <5ms target)** - Detect Movement
```rust
// Runs BEFORE collision detection in same tick
fn detect_moved_entities(
    positions: Query<(Entity, &SimPosition, &SimPositionPrev, &OccupiedCell)>,
    mut scratch: ResMut<SpatialHashScratch>,
) {
    scratch.moved_entities.clear();  // O(1) - doesn't deallocate
    
    for (entity, pos, prev, occupied) in positions.iter() {
        if pos.0 != prev.0 {  // Position changed?
            // CRITICAL: Check capacity BEFORE push
            if scratch.moved_entities.len() < scratch.moved_entities.capacity() {
                scratch.moved_entities.push(MovedEntity {
                    entity,
                    old_cell: *occupied,
                });
            } else {
                // OVERFLOW DETECTED
                #[cfg(debug_assertions)]
                panic!("Too many entities moved! {} > capacity {}", 
                       scratch.moved_entities.len(), 
                       scratch.moved_entities.capacity());
                
                #[cfg(not(debug_assertions))]
                warn_once!("Moved entities overflow - updates dropped");
            }
        }
    }
}
```
- Uses pre-allocated `moved_entities` Vec (cleared each tick)
- Only processes entities that actually moved (~5-10% per tick)
- Target: <5ms for 500k entities (only ~25k moved)

**PHASE 3: Cold Path (Async/Deferred)** - Apply Updates & Compact
```rust
// Runs in parallel with non-spatial systems, or every N ticks
fn apply_spatial_updates(
    moved: Res<SpatialHashScratch>,
    mut arena: ResMut<SpatialHashArena>,
) {
    // Apply all deferred moves
    for moved_entity in &moved.moved_entities {
        arena.remove_from_cell(moved_entity.old_cell);
        arena.insert_to_cell(moved_entity.new_cell, moved_entity.entity);
    }
    
    // Incremental compaction if fragmented
    if arena.fragmentation_ratio() > 0.2 {
        arena.compact_incremental(10_000);  // Process 10k entities max
    }
}
```
- Can run in parallel system chain
- Expensive work (compaction) is incremental
- Target: <20ms worst case (but async, doesn't block simulation)

#### 2.2.3 Zero-Allocation Guarantee

**⚠️⚠️⚠️ ABSOLUTE REQUIREMENTS FOR 10M ENTITY SCALE ⚠️⚠️⚠️**

**NO RUNTIME ALLOCATION. PERIOD.**

Every single allocation happens at startup. During gameplay (insert/remove/query), we ONLY:
- ✅ Write to preallocated buffers
- ✅ Return slice views (`&[Entity]`) 
- ✅ Clear and reuse Vecs (`.clear()` keeps capacity)

**NEVER during gameplay:**
- ❌ Call `Vec::new()` or `Vec::with_capacity()`
- ❌ Call `.to_vec()`, `.clone()`, or `.collect()` on iterators
- ❌ Return `Vec<Entity>` from queries (return `&[Entity]` instead!)
- ❌ Push to Vec without capacity check
- ❌ Use `SmallVec`, `String`, `HashMap` (unless preallocated)

**Why This Is Non-Negotiable:**

```
Runtime allocation at 10M entities:
- Allocation: ~500ns (syscall + metadata)
- Deallocation: ~200ns (free list update)
- Memory fragmentation: Unbounded
- Frame spike: 1000 allocations = 0.7ms (14% of 60 FPS budget!)

With zero-allocation:
- Write to arena: ~5ns (cache hit)
- Return slice: ~5ns (pointer math)
- Clear Vec: ~1ns (set len=0)
- Frame spike: ZERO
```

**Critical Rules:**

1. **All Vecs preallocated at startup** based on `max_entities` config
   ```rust
   entity_storage: Vec::with_capacity(max_entities),
   moved_entities: Vec::with_capacity(max_entities * 30 / 100),  // 30% worst case
   query_results: Vec::with_capacity(max_query_results),
   ```

2. **Use `.clear()` not new Vecs** - clear sets len=0, keeps capacity
   ```rust
   // WRONG - allocates every tick! ❌
   let mut moved = Vec::new();
   
   // RIGHT - reuses preallocated buffer ✅
   scratch.moved_entities.clear();
   ```

3. **Queries return slice views `&[Entity]`, NEVER `Vec<Entity>`**
   ```rust
   // WRONG - allocates + copies! ❌
   fn query(&self) -> Vec<Entity> {
       self.entities[start..end].to_vec()  // ALLOCATION!
   }
   
   // RIGHT - returns view, zero-copy ✅
   fn query(&self) -> &[Entity] {
       &self.entities[start..end]  // Just a fat pointer!
   }
   ```

4. **Check capacity before `.push()`** to detect overflow
   ```rust
   if vec.len() < vec.capacity() {
       vec.push(item);
   } else {
       handle_overflow();  // Panic debug, warn release
   }
   ```

5. **Use `Box<[T]>` for truly fixed arrays** that can never grow
   ```rust
   cell_ranges: Box<[CellRange]>,  // Allocated once, immutable capacity
   ```

6. **Iterator chains that `.collect()` must use preallocated buffers**
   ```rust
   // WRONG - allocates! ❌
   let results: Vec<Entity> = query.iter().filter(...).collect();
   
   // RIGHT - write to preallocated buffer ✅
   scratch.results.clear();
   for item in query.iter().filter(...) {
       scratch.results.push(item);
   }
   ```

#### 2.2.4 Memory Budget (10M Entities)

**Architecture: Duplicated Arenas Per Size Class**

Each size class gets TWO full-capacity arenas (Grid A + Grid B):

**Per Size Class (One of Three):**

| Component | Grid A | Grid B | Total |
|-----------|--------|--------|-------|
| Entity Storage | 80 MB | 80 MB | 160 MB |
| Cell Ranges (10k cells) | 80 KB | 80 KB | 160 KB |
| **Subtotal per Size Class** | | | **~160 MB** |

**Total System (3 Size Classes):**

| Size Class | Cell Size | Entity Radii | Arena A | Arena B | Total |
|------------|-----------|--------------|---------|---------|-------|
| Small | 40 | 0.1 - 10.0 | 80 MB | 80 MB | 160 MB |
| Medium | 100 | 10.0 - 25.0 | 80 MB | 80 MB | 160 MB |
| Large | 200 | 25.0+ | 80 MB | 80 MB | 160 MB |
| Cell Ranges (all grids) | - | - | - | - | 0.5 MB |
| Scratch Buffers | - | - | - | - | 72 MB |
| Compaction Buffers | - | - | - | - | 80 MB |
| **GRAND TOTAL** | | | | | **~790 MB** |

**Memory Trade-off Analysis:**

| Approach | Entity Storage | Cell Metadata | Total |
|----------|----------------|---------------|-------|
| **Unified Arena** (1 shared) | 80 MB | 0.5 MB | 80.5 MB |
| **Duplicated Arenas** (6 separate) | 480 MB | 0.5 MB | 480.5 MB |
| **Overhead** | **+400 MB** | - | **+400 MB** |

**Why Duplicated Arenas Despite Overhead:**

1. **Cache Isolation**: Querying small units NEVER touches large obstacle data
   - Huge performance win in hot path (queries)
   - Worth 400MB (4% of 10GB budget)

2. **Independent Compaction**: Each grid can compact in parallel
   ```rust
   size_classes.par_iter_mut().for_each(|sc| {
       sc.grid_a.compact();  // No data races!
       sc.grid_b.compact();
   });
   ```

3. **Simple Growth**: If one size class fills up, only grow that arena
   ```rust
   if grid_small.entity_storage.len() == grid_small.capacity() {
       grid_small.entity_storage.reserve(1_000_000);  // Just this one
   }
   ```

4. **Actual Usage**: Even if 99% of entities are small (9.9M) and 1% large (100k):
   - Large arena "wastes" 9.9M slots × 8 bytes = 79MB
   - Still negligible compared to performance benefits

**Compare to Naive Vec-Per-Cell:**
- 3M cells × 6 grids × 64-capacity Vecs × 8 bytes = **~9.2 GB**
- Plus Vec overhead (24 bytes each) = **+430 MB**
- **Total: ~9.6 GB** for naive approach

**Duplicated arena is 12× more memory efficient than naive approach!**

#### 2.2.5 Configuration in initial_config.ron

```ron
SpatialHashConfig(
    // Maximum entities across ALL size classes
    // Each size class gets a full-capacity arena (duplicated storage)
    max_entities: 10_000_000,
    
    // Worst-case: assume 30% of entities move per tick
    max_moved_per_tick_ratio: 0.30,
    
    // Size class configuration (cell sizes and entity radius ranges)
    // Each size class has Grid A + Grid B (staggered by cell_size/2)
    size_classes: [
        SizeClass(
            cell_size: 40.0, 
            min_radius: 0.1, 
            max_radius: 10.0,
            // Grid A centers: (0, 40, 80, ...)
            // Grid B centers: (20, 60, 100, ...)
        ),
        SizeClass(
            cell_size: 100.0, 
            min_radius: 10.0, 
            max_radius: 25.0,
            // Grid A centers: (0, 100, 200, ...)
            // Grid B centers: (50, 150, 250, ...)
        ),
        SizeClass(
            cell_size: 200.0, 
            min_radius: 25.0, 
            max_radius: 100.0,
            // Grid A centers: (0, 200, 400, ...)
            // Grid B centers: (100, 300, 500, ...)
        ),
    ],
    
    // CAVEAT: Super-rare huge entities (radius > 100)
    // If count is small (<1000), use multi-cell in largest size class
    // instead of creating dedicated size class (saves 160MB arena)
    rare_huge_entity_threshold: 1000,
    
    // Defragmentation threshold
    fragmentation_threshold: 0.2,  // Compact when >20% wasted space
    
    // Overflow behavior
    overflow_strategy: WarnOnce,  // Panic | WarnOnce | Silent
)
```

**Caveat: Handling Rare Super-Huge Entities**

If you have < 1000 entities with radius > 100 (e.g., mega-structures, map decorations):

**Option A (Preferred if rare):** Use multi-cell insertion in largest size class
```rust
// Entity with radius 150 in cell_size=200 grid:
// Spans ~2×2 = 4 cells, insert into all 4
// Acceptable overhead: 1000 entities × 4 cells = 4000 insertions
// vs dedicating 160MB arena for 1000 entities (99.4% wasted)
```

**Option B (If common):** Add dedicated size class
```rust
size_classes: [
    // ... existing ...
    SizeClass(cell_size: 500.0, min_radius: 100.0, max_radius: 500.0),
    // Costs 160MB but worth it if >1000 entities
],
```

**Rule of thumb:** 
- < 1000 entities: Multi-cell is fine (small overhead)
- 1000-10,000 entities: Profile to decide
- \> 10,000 entities: Dedicated size class justified

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
    range_idx: usize,  // Index into cell's range in arena (for O(1) removal)
}
```

**Memory Savings:**
- Old multi-cell: `SmallVec<[(usize, usize, usize); 4]>` = 96 bytes
- New single-cell: 5 fields = ~25 bytes
- **71 bytes saved per entity** (10M entities = 710 MB saved!)

### 2.3 Lifecycle (Arena-Based Three-Phase Architecture)

The arena-based design uses **deferred structural updates** to maintain zero-allocation hot paths:

#### Phase 1: Query (Hot Path - Every Tick)

**⚠️ ZERO-ALLOCATION REQUIREMENT: All queries use preallocated scratch buffers!**

Single-cell queries can return `&[Entity]` slice views directly. Multi-cell queries (radius search) 
must aggregate results, which requires a **preallocated scratch buffer** that is cleared and reused.

**Single-Cell Query (Returns Slice View):**
```rust
// ✅ PERFECT - Zero allocation, returns view
fn query_single_cell(&self, col: usize, row: usize) -> &[Entity] {
    let cell_idx = row * self.cols + col;
    let range = &self.cell_ranges[cell_idx];
    
    // Just returns a fat pointer (16 bytes) - NO ALLOCATION
    &self.entity_storage[range.start_index .. range.start_index + range.current_count]
}
```

**Multi-Cell Query (Uses Preallocated Scratch Buffer):**
```rust
// ✅ CORRECT - Uses preallocated scratch buffer, no runtime allocation
// 
// CRITICAL: scratch buffer is preallocated at startup with capacity for worst-case
// We NEVER allocate during query - just clear() and reuse
fn query_entities_in_radius(
    spatial_hash: Res<SpatialHash>,
    pos: FixedVec2,
    radius: FixedNum,
    size_class_idx: u8,
    scratch: &mut Vec<Entity>,  // Preallocated buffer passed in
) -> &[Entity] {
    // Clear sets len=0 but keeps capacity - NO DEALLOCATION
    scratch.clear();  // O(1), reuse buffer
    
    let size_class = &spatial_hash.size_classes[size_class_idx as usize];
    
    // Query Grid A - each cell returns a &[Entity] view
    let cells_a = size_class.grid_a.get_cells_in_radius(pos, radius);
    for cell_idx in cells_a {
        let range = &size_class.grid_a.cell_ranges[cell_idx];
        
        // Get slice view from arena (ZERO ALLOCATION)
        let entities = &size_class.grid_a.entity_storage[range.start_index .. range.start_index + range.current_count];
        
        // Copy entity IDs into scratch buffer (ALREADY ALLOCATED)
        // CRITICAL: Check capacity to detect overflow
        if scratch.len() + entities.len() <= scratch.capacity() {
            scratch.extend_from_slice(entities);  // Memcpy, no allocation
        } else {
            #[cfg(debug_assertions)]
            panic!("Scratch buffer overflow! Need {} but capacity is {}", 
                   scratch.len() + entities.len(), scratch.capacity());
            
            #[cfg(not(debug_assertions))]
            warn_once!("Query scratch overflow - results truncated");
            break;  // Truncate results rather than allocate
        }
    }
    
    // Query Grid B (same pattern)
    let cells_b = size_class.grid_b.get_cells_in_radius(pos, radius);
    for cell_idx in cells_b {
        let range = &size_class.grid_b.cell_ranges[cell_idx];
        let entities = &size_class.grid_b.entity_storage[range.start_index .. range.start_index + range.current_count];
        
        if scratch.len() + entities.len() <= scratch.capacity() {
            scratch.extend_from_slice(entities);
        } else {
            warn_once!("Query scratch overflow - results truncated");
            break;
        }
    }
    
    // Return slice view of scratch buffer (still no allocation!)
    &scratch[..]  // Returns &[Entity] view into scratch buffer
}
```

**Scratch Buffer Management:**

```rust
// Preallocated at startup
#[derive(Resource)]
struct SpatialHashScratch {
    query_results: Vec<Entity>,  // Capacity: worst-case query size
    moved_entities: Vec<MovedEntity>,  // Capacity: max_entities × 30%
}

impl SpatialHashScratch {
    fn new(max_entities: usize, max_query_radius: f32, cell_size: f32) -> Self {
        // Worst-case query: circle overlaps 9 cells (3×3), each at max capacity
        let cells_per_query = ((max_query_radius * 2.0 / cell_size).ceil() as usize + 1).pow(2);
        let max_entities_per_cell = (max_entities / (MAP_SIZE / cell_size).pow(2) as usize) * 2;
        let query_capacity = cells_per_query * max_entities_per_cell;
        
        Self {
            query_results: Vec::with_capacity(query_capacity),
            moved_entities: Vec::with_capacity(max_entities * 30 / 100),
        }
    }
}

// Usage in system
fn collision_detection_system(
    spatial_hash: Res<SpatialHash>,
    mut scratch: ResMut<SpatialHashScratch>,
    units: Query<(&SimPosition, &Collider)>,
) {
    for (pos, collider) in units.iter() {
        // Pass scratch buffer to query - will be cleared and reused
        let nearby = query_entities_in_radius(
            spatial_hash.as_ref(),
            pos.0,
            collider.radius * 2.0,
            0,  // size_class_idx
            &mut scratch.query_results,
        );
        
        // Process nearby entities
        for &entity in nearby {
            // ... collision logic ...
        }
    }
    // scratch.query_results cleared at start of each query
    // NO allocations happened during this entire system!
}
```

**Why We Can't Return `&[Entity]` for Multi-Cell Queries:**

```rust
// ❌ IMPOSSIBLE - Can't return slice view across multiple non-contiguous cells
fn query_radius_IMPOSSIBLE(&self, pos: FixedVec2, radius: FixedNum) -> &[Entity] {
    // Cell A entities: &entity_storage[100..150]  (50 entities)
    // Cell B entities: &entity_storage[500..580]  (80 entities)
    // Cell C entities: &entity_storage[900..950]  (50 entities)
    // 
    // Can't return a SINGLE &[Entity] that spans all three!
    // They're not contiguous in memory.
    // 
    // MUST aggregate into scratch buffer to return contiguous slice.
}
```

**Performance Summary:**

| Query Type | Allocation | Copy Cost | Typical Latency |
|------------|------------|-----------|-----------------|
| Single-cell `&[Entity]` | 0 bytes | 0 bytes | **~5ns** |
| Multi-cell (3×3 cells) | 0 bytes | ~800 bytes memcpy | **~50ns** |
| Multi-cell (WRONG, new Vec) | 8-32 bytes | ~800 bytes | ~500ns + fragmentation |

**The scratch buffer approach:**
- ✅ Zero heap allocations
- ✅ Predictable memory usage
- ✅ Cache-friendly (linear memcpy)
- ✅ Overflow protection (capacity checks)

#### Phase 2: Detect Movement (Warm Path - Every Tick)
```rust
fn detect_moved_entities(
    query: Query<(Entity, &SimPosition, &SimPositionPrev, &OccupiedCell)>,
    mut scratch: ResMut<SpatialHashScratch>,
    spatial_hash: Res<SpatialHash>,
) {
    // CRITICAL: Clear, don't allocate new Vec
    scratch.moved_entities.clear();
    
    for (entity, pos, prev, occupied) in query.iter() {
        if pos.0 == prev.0 { continue; }  // Position unchanged
        
        let size_class = &spatial_hash.size_classes[occupied.size_class as usize];
        
        // Get current grid (A or B)
        let current_grid = if occupied.grid_offset == 0 {
            &size_class.grid_a
        } else {
            &size_class.grid_b
        };
        
        // Calculate center of current cell
        let current_center = current_grid.cell_center(occupied.col, occupied.row);
        
        // Calculate center in opposite grid
        let opposite_grid = if occupied.grid_offset == 0 {
            &size_class.grid_b
        } else {
            &size_class.grid_a
        };
        
        let (opp_col, opp_row) = opposite_grid.pos_to_cell(pos.0);
        let opposite_center = opposite_grid.cell_center(opp_col, opp_row);
        
        // Check if entity switched grids (now closer to opposite grid)
        let dist_current_sq = (pos.0 - current_center).length_squared();
        let dist_opposite_sq = (pos.0 - opposite_center).length_squared();
        
        if dist_opposite_sq < dist_current_sq {
            // Entity switched grids - record for deferred update
            if scratch.moved_entities.len() < scratch.moved_entities.capacity() {
                scratch.moved_entities.push(MovedEntity {
                    entity,
                    old_cell: *occupied,
                    new_grid_offset: 1 - occupied.grid_offset,  // Flip 0<->1
                    new_col: opp_col,
                    new_row: opp_row,
                });
            } else {
                #[cfg(debug_assertions)]
                panic!("Moved entities buffer overflow!");
                
                #[cfg(not(debug_assertions))]
                warn_once!("Moved entities overflow - some updates dropped");
            }
        }
        // else: Still in same grid, no update needed
    }
}
```

#### Phase 3: Apply Updates (Cold Path - Async/Deferred)
```rust
fn apply_deferred_spatial_updates(
    scratch: Res<SpatialHashScratch>,
    mut spatial_hash: ResMut<SpatialHash>,
    mut occupied_cells: Query<&mut OccupiedCell>,
) {
    for moved in &scratch.moved_entities {
        let size_class = &mut spatial_hash.size_classes[moved.old_cell.size_class as usize];
        
        // Remove from old grid (A or B)
        let old_grid = if moved.old_cell.grid_offset == 0 {
            &mut size_class.grid_a
        } else {
            &mut size_class.grid_b
        };
        
        old_grid.remove_entity_from_cell(
            moved.old_cell.col,
            moved.old_cell.row,
            moved.old_cell.range_idx,
            moved.entity,
        );
        
        // Insert into new grid (opposite of old)
        let new_grid = if moved.new_grid_offset == 0 {
            &mut size_class.grid_a
        } else {
            &mut size_class.grid_b
        };
        
        let new_range_idx = new_grid.insert_entity_into_cell(
            moved.new_col,
            moved.new_row,
            moved.entity,
        );
        
        // Update component
        if let Ok(mut occupied) = occupied_cells.get_mut(moved.entity) {
            occupied.grid_offset = moved.new_grid_offset;
            occupied.col = moved.new_col;
            occupied.row = moved.new_row;
            occupied.range_idx = new_range_idx;
        }
    }
    
    // Check fragmentation and compact if needed (per grid)
    for size_class in &mut spatial_hash.size_classes {
        if size_class.grid_a.fragmentation_ratio() > 0.2 {
            size_class.grid_a.compact_incremental(10_000);
        }
        if size_class.grid_b.fragmentation_ratio() > 0.2 {
            size_class.grid_b.compact_incremental(10_000);
        }
    }
}
```
) {
    for moved in &scratch.moved_entities {
        // Remove from old cell
        spatial_hash.remove_from_cell(moved.old_cell);
        
        // Insert into new cell (may trigger compaction)
        let new_range_idx = spatial_hash.insert_into_cell(moved.new_cell, moved.entity);
        
        // Update component
        if let Ok(mut occupied) = occupied_cells.get_mut(moved.entity) {
            *occupied = moved.new_cell;
            occupied.range_idx = new_range_idx;
        }
    }
    
    // Incremental compaction if fragmented
    if spatial_hash.fragmentation_ratio() > 0.2 {
        spatial_hash.compact_incremental(10_000);  // Max 10k entities per tick
    }
}
```

**Lifecycle Summary:**
1. **Insert New Entities:** Classify by size, insert into nearest grid center (single cell in arena)
2. **Detect Movement (Warm):** Build list of moved entities (preallocated Vec, just cleared)
3. **Apply Updates (Cold):** Remove from old cells, insert into new cells (async/deferred)
4. **Compact (Cold):** Incremental defragmentation when fragmentation > threshold
5. **Query (Hot):** Read-only queries against stable arena storage

**Static Entities Optimization:**
- Calculate cell once on spawn
- **Never** appear in moved_entities list
- Zero ongoing cost (perfect for obstacles, buildings)

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

### 2.5 Query API

**CRITICAL: All query methods are READ-ONLY and NEVER allocate!**

```rust
impl SpatialHash {
    /// General proximity query: Find all entities within radius
    /// Returns slice of entity_storage - zero allocation!
    /// 
    /// Used by: Boids, AI, general gameplay
    pub fn query_radius<'a>(
        &'a self, 
        pos: FixedVec2, 
        radius: FixedNum,
        scratch: &'a mut SpatialHashScratch,
    ) -> &'a [Entity] {
        // CRITICAL: Use scratch buffer, don't allocate
        scratch.query_results.clear();
        
        let cells = self.get_cells_in_radius(pos, radius);
        for cell_idx in cells {
            let range = &self.cell_ranges[cell_idx];
            let entities = &self.entity_storage[range.start_index .. range.start_index + range.current_count];
            
            // CRITICAL: Check capacity before extend
            let needed = scratch.query_results.len() + entities.len();
            if needed <= scratch.query_results.capacity() {
                scratch.query_results.extend_from_slice(entities);
            } else {
                warn_once!("Query result buffer overflow - results truncated");
                break;
            }
        }
        
        &scratch.query_results[..]
    }
    
    /// Layer-filtered query: Find entities matching layer mask within radius
    /// 
    /// Used by: Combat systems, AI target selection
    pub fn query_radius_filtered<'a>(
        &'a self, 
        pos: FixedVec2, 
        radius: FixedNum, 
        layer_mask: u32,
        colliders: &Query<&Collider>,
        scratch: &'a mut SpatialHashScratch,
    ) -> &'a [Entity] {
        // Similar to query_radius but filters by layer
        // Still uses scratch buffer - no allocation
    }
}

// Scratch buffer for query results
#[derive(Resource)]
struct SpatialHashScratch {
    moved_entities: Vec<MovedEntity>,
    query_results: Vec<Entity>,         // Preallocated for query results
    compaction_buffer: Vec<Entity>,
}
```

### 2.6 Design Principles (Arena-Based Architecture)

1. **Zero Runtime Allocation:** All data structures preallocated at startup based on `max_entities` config
2. **Single Source of Truth:** One spatial structure (arena per grid) for all proximity queries
3. **Deferred Structural Updates:** Hot paths (queries) are read-only; expensive updates deferred to cold path
4. **Self-Exclusion:** Queries never return the querying entity itself
5. **Correctness over Speed:** Spatial hash must return identical results to brute-force O(N²) search
6. **Layer Awareness:** Support collision layers for filtering
7. **Predictable Performance:** No reallocation spikes, no GC pauses, deterministic frame times
8. **Memory Efficiency:** Arena storage is 5-10× more efficient than per-cell Vecs
9. **Scalability:** Designed to handle 10M+ entities with <2GB spatial hash footprint

### 2.7 Performance Characteristics (Arena vs Naive)

#### Memory Comparison (500k Entities)

| Approach | Entity Storage | Cell Metadata | Overhead | Total |
|----------|----------------|---------------|----------|-------|
| **Naive (Vec per cell)** | N/A (in cells) | 500k cells × 6 grids × 24 bytes | Vec growth buffer | **~500 MB** |
| **Fixed-cap per cell** | 500k cells × 6 grids × 64 × 8 bytes | Included | None | **~9.6 GB** |
| **Arena (This Design)** | 500k × 8 bytes × 6 grids | 500k cells × 6 grids × 16 bytes | Scratch buffers | **~90 MB** |

**Arena is 5.4× more efficient than naive, 106× more efficient than fixed-cap!**

#### Update Cost

| Phase | Naive Vec-per-cell | Arena-Based | Notes |
|-------|-------------------|-------------|-------|
| **Hot (Query)** | 0.1ms | **0.1ms** | Same - both are reads |
| **Warm (Detect)** | 5ms | **1ms** | Arena only builds list, no updates |
| **Cold (Apply)** | **66ms SPIKE** | 20ms (async) | Naive spikes on Vec realloc, Arena is deferred |

**Arena eliminates 66ms spikes, enables 10M+ scale!**

#### Query Cost
- Must query both Grid A and Grid B (2× grid queries)
- But queries can be parallelized or memory-interleaved
- With neighbor caching (already implemented): Query cost mostly irrelevant (95% cache hit)
- Each grid query is simpler (no duplicate detection within grid)

### 2.8 Arena Fragmentation & Compaction Strategy

**The Fragmentation Problem:**

When entities move between cells, the arena storage can become fragmented:

```
Before moves (compact):
Cell 0: [E1, E2, E3]     start=0, count=3
Cell 1: [E4, E5, E6, E7] start=3, count=4
Cell 2: [E8, E9]         start=7, count=2

After E2 moves from Cell 0 → Cell 1:
Cell 0: [E1, _, E3]      start=0, count=2 (gap at index 1!)
Cell 1: [E4, E5, E6, E7, E2] start=3, count=5
Cell 2: [E8, E9]         start=7, count=2

Fragmentation ratio = 1 gap / 9 entities = 11%
```

**When Compaction is Needed:**
- **Trigger:** Fragmentation ratio > 20% (configurable)
- **Frequency:** Check every N ticks (e.g., every 10 ticks)
- **Strategy:** Incremental compaction (10k entities max per frame)

**Incremental Compaction Algorithm:**

```rust
impl StaggeredGrid {
    /// Compact a portion of the arena to remove fragmentation
    /// Processes up to max_entities_per_tick to maintain frame budget
    pub fn compact_incremental(&mut self, max_entities_per_tick: usize) {
        let mut entities_processed = 0;
        let mut write_pos = 0;
        
        // For each cell range
        for cell_idx in 0..self.cell_ranges.len() {
            let range = &mut self.cell_ranges[cell_idx];
            
            if range.current_count == 0 {
                continue;
            }
            
            // Check if we've hit our frame budget
            if entities_processed + range.current_count > max_entities_per_tick {
                break;  // Continue next frame
            }
            
            // Copy this cell's entities to write_pos
            if range.start_index != write_pos {
                // Only copy if not already at correct position
                for i in 0..range.current_count {
                    self.entity_storage[write_pos + i] = 
                        self.entity_storage[range.start_index + i];
                }
            }
            
            // Update range to new compacted position
            range.start_index = write_pos;
            write_pos += range.current_count;
            entities_processed += range.current_count;
        }
        
        // Update fragmentation ratio
        self.fragmentation_ratio = 
            1.0 - (write_pos as f32 / self.entity_storage.capacity() as f32);
    }
    
    /// Calculate current fragmentation ratio
    pub fn fragmentation_ratio(&self) -> f32 {
        let used: usize = self.cell_ranges.iter()
            .map(|r| r.current_count)
            .sum();
        let wasted = self.entity_storage.capacity() - used;
        wasted as f32 / self.entity_storage.capacity() as f32
    }
}
```

**Compaction Characteristics:**

| Metric | Value | Notes |
|--------|-------|-------|
| **Trigger Threshold** | 20% fragmentation | Configurable in initial_config.ron |
| **Max Entities/Frame** | 10,000 | Maintains <5ms frame budget |
| **Frequency Check** | Every 10 ticks | Only check, not always compact |
| **Worst Case Time** | ~5ms | 10k entities @ 0.5μs each |
| **Parallelizable** | Yes | Can run async with queries |

**Double-Buffering Strategy (Advanced):**

For zero-latency compaction, use ping-pong buffers:

```rust
struct StaggeredGrid {
    // Active buffer (used by queries)
    entity_storage: Vec<Entity>,
    cell_ranges: Vec<CellRange>,
    
    // Background buffer (used during compaction)
    next_storage: Vec<Entity>,
    next_ranges: Vec<CellRange>,
    
    // Swap when compaction completes
    buffer_idx: bool,  // false = current, true = next
}

impl StaggeredGrid {
    /// Background compaction - builds next_storage while queries use entity_storage
    pub fn compact_background(&mut self) {
        // Build fully compacted version in next_storage
        let mut write_pos = 0;
        for (cell_idx, range) in self.cell_ranges.iter().enumerate() {
            if range.current_count > 0 {
                for i in 0..range.current_count {
                    self.next_storage[write_pos + i] = 
                        self.entity_storage[range.start_index + i];
                }
                self.next_ranges[cell_idx] = CellRange {
                    start_index: write_pos,
                    current_count: range.current_count,
                };
                write_pos += range.current_count;
            }
        }
    }
    
    /// Atomic swap - instant cutover to compacted buffer
    pub fn swap_buffers(&mut self) {
        std::mem::swap(&mut self.entity_storage, &mut self.next_storage);
        std::mem::swap(&mut self.cell_ranges, &mut self.next_ranges);
        self.fragmentation_ratio = 0.0;  // Fully compacted
    }
}
```

**When to Use Each Strategy:**

| Strategy | Use Case | Latency | Memory |
|----------|----------|---------|--------|
| **Incremental** | Default | ~1ms per 10k | 1× storage |
| **Double-Buffer** | >5M entities | Instant (atomic swap) | 2× storage |
| **No Compaction** | <100k entities | N/A | Acceptable fragmentation |

### 2.8 Update Strategies: Full Rebuild vs Incremental Updates

**ARCHITECTURAL DECISION (January 2026):**

The spatial hash supports **two distinct update strategies** optimized for different entity scales:

1. **Full Rebuild Every Frame** - Simple, fast for <1M entities
2. **Incremental Updates with Swap-Based Removal** - Complex, essential for 5-10M entities

#### 2.8.1 Strategy A: Full Rebuild Every Frame (Current Implementation)

**When to Use:**
- Entity count: <1M entities
- Update frequency: Every physics tick
- Acceptable rebuild time: <5ms per frame

**Architecture:**

```rust
fn rebuild_spatial_hash_system(
    entities: Query<(Entity, &SimPosition, &Collider)>,
    mut spatial_hash: ResMut<SpatialHash>,
) {
    // Step 1: Clear all cells (O(num_cells), very fast)
    spatial_hash.clear();  // Resets current_count to 0 for all cells
    
    // Step 2: Rebuild from scratch (O(num_entities))
    for (entity, pos, collider) in entities.iter() {
        spatial_hash.insert(entity, pos.0, collider.radius);
    }
}
```

**Performance Characteristics:**

| Entity Count | Rebuild Time | Frame Budget | Acceptable? |
|--------------|--------------|--------------|-------------|
| 100k | ~0.5ms | 16.67ms @ 60fps | ✅ Yes |
| 500k | ~2.5ms | 16.67ms @ 60fps | ✅ Yes |
| 1M | ~5ms | 16.67ms @ 60fps | ⚠️ Marginal |
| 5M | ~25ms | 16.67ms @ 60fps | ❌ No |
| 10M | ~50ms | 16.67ms @ 60fps | ❌ No |

**Advantages:**
- ✅ **Simple:** No fragmentation tracking, no compaction needed
- ✅ **Predictable:** Same cost every frame (no spikes)
- ✅ **Cache-friendly:** Sequential writes to arena
- ✅ **Zero fragmentation:** Always perfectly packed after rebuild

**Disadvantages:**
- ❌ **O(N) every frame:** Processes ALL entities even if most didn't move
- ❌ **Doesn't scale:** >1M entities exceeds frame budget
- ❌ **Redundant work:** 90%+ of entities typically haven't changed cells

**When Full Rebuild Breaks Down:**

At 10M entities with 5% movement per tick:
- **Entities that moved cells:** 500k (need updates)
- **Entities that didn't move cells:** 9.5M (redundant processing)
- **Wasted work ratio:** 95%

#### 2.8.2 Strategy B: Incremental Updates with Arena Over-Provisioning

**When to Use:**
- Entity count: >1M entities
- Movement rate: 5-15% entities change cells per tick
- Target update time: <5ms even at 10M entities

**Core Idea:**

Instead of rebuilding, use **swap-based removal** to incrementally update cells:

```rust
// Per-cell storage structure with headroom
struct CellRange {
    start_index: usize,    // Where this cell's data begins in arena
    current_count: usize,  // Current number of entities in this cell
    max_index: usize,      // Maximum index (exclusive) before overflow
}
```

**Arena Over-Provisioning:**

Pre-allocate arena with extra capacity to avoid frequent rebuilds:

```ron
// In initial_config.ron
spatial_hash_arena_overcapacity_ratio: 1.5,  // 1.5× = 50% extra space
// Example: 10M entities × 1.5 = 15M arena capacity
//          Extra 5M slots distributed across cells as headroom
```

**CRITICAL: Arena Cell Structure & Rebuild Strategy**

The key invariant is that after every REBUILD, all cells have **equal headroom**:
```
headroom = max_index - current_count
```

This headroom should be **(arena_length - total_entity_count) / num_cells** for all cells (±1 due to rounding).

**Arena Size Calculation:**

```rust
let arena_length = MAX_ENTITY_COUNT * OVERCAPACITY_RATIO;
// Round up to be evenly divisible by num_cells for cleaner math
let arena_length = ((arena_length + num_cells - 1) / num_cells) * num_cells;
```

**Initial Build (First Rebuild):**

When the spatial hash is first created, all cells are empty:
```rust
fn initial_build(arena_length: usize, num_cells: usize) -> Vec<CellRange> {
    let headroom_per_cell = arena_length / num_cells;
    
    (0..num_cells).map(|cell_idx| {
        CellRange {
            start_index: cell_idx * headroom_per_cell,
            current_count: 0,  // No entities yet
            max_index: (cell_idx + 1) * headroom_per_cell,
        }
    }).collect()
}
```

Example with `arena_length = 1000`, `num_cells = 10`:
```
Cell 0: start_index = 0,   current_count = 0, max_index = 100   (headroom = 100)
Cell 1: start_index = 100, current_count = 0, max_index = 200   (headroom = 100)
Cell 2: start_index = 200, current_count = 0, max_index = 300   (headroom = 100)
...
Cell 9: start_index = 900, current_count = 0, max_index = 1000  (headroom = 100)
```

**Subsequent Rebuilds:**

After entities have been inserted and some cells are occupied, we rebuild while maintaining equal headroom:

```rust
fn rebuild_with_equal_headroom(
    arena_length: usize,
    num_cells: usize,
    cell_entity_counts: &[usize],  // current_count for each cell
) -> Vec<CellRange> {
    let total_entity_count: usize = cell_entity_counts.iter().sum();
    let total_free_space = arena_length - total_entity_count;
    let headroom_per_cell = total_free_space / num_cells;
    let extra_slots = total_free_space % num_cells;  // Distribute remainder
    
    let mut ranges = Vec::with_capacity(num_cells);
    let mut write_pos = 0;
    
    for (cell_idx, &entity_count) in cell_entity_counts.iter().enumerate() {
        // Give first `extra_slots` cells one additional slot to handle remainder
        let this_cell_headroom = headroom_per_cell + if cell_idx < extra_slots { 1 } else { 0 };
        
        ranges.push(CellRange {
            start_index: write_pos,
            current_count: entity_count,
            max_index: write_pos + entity_count + this_cell_headroom,
        });
        
        write_pos += entity_count + this_cell_headroom;
    }
    
    ranges
}
```

Example with `arena_length = 1000`, `num_cells = 10`, and cells have `[50, 30, 80, 20, 40, 60, 10, 90, 70, 50]` entities:
```
total_entity_count = 500
total_free_space = 500
headroom_per_cell = 50
extra_slots = 0

Cell 0: start_index = 0,   current_count = 50, max_index = 100  (headroom = 50)
Cell 1: start_index = 100, current_count = 30, max_index = 180  (headroom = 50)
Cell 2: start_index = 180, current_count = 80, max_index = 310  (headroom = 50)
Cell 3: start_index = 310, current_count = 20, max_index = 380  (headroom = 50)
Cell 4: start_index = 380, current_count = 40, max_index = 470  (headroom = 50)
Cell 5: start_index = 470, current_count = 60, max_index = 580  (headroom = 50)
Cell 6: start_index = 580, current_count = 10, max_index = 640  (headroom = 50)
Cell 7: start_index = 640, current_count = 90, max_index = 780  (headroom = 50)
Cell 8: start_index = 780, current_count = 70, max_index = 900  (headroom = 50)
Cell 9: start_index = 900, current_count = 50, max_index = 1000 (headroom = 50)
```

**Key Properties:**

1. **Equal Opportunity:** All cells have equal headroom, regardless of current occupancy
2. **Compaction:** Entities are tightly packed at the start of each cell's range
3. **First Build is Special Case:** Initial build is just `rebuild_with_equal_headroom` where all `current_count = 0`
4. **No Fragmentation:** After rebuild, `start_index + current_count` points to first free slot

**Incremental Update Algorithm:**

```rust
impl StaggeredGrid {
    /// Remove entity from old cell using swap-with-last-element trick
    /// O(1) operation, NO FRAGMENTATION
    fn remove_from_cell(&mut self, cell_id: usize, entity: Entity) -> bool {
        let range = &mut self.cell_ranges[cell_id];
        
        // Find entity in cell's range [start_index, start_index + current_count)
        let cell_start = range.start_index;
        let cell_end = range.start_index + range.current_count;
        
        for i in cell_start..cell_end {
            if self.entity_storage[i] == entity {
                // Swap with last element in this cell
                let last_idx = cell_end - 1;
                self.entity_storage[i] = self.entity_storage[last_idx];
                
                // Shrink count by 1
                range.current_count -= 1;
                return true;
            }
        }
        false
    }
    
    /// Add entity to new cell if capacity available
    /// O(1) operation if headroom available
    fn insert_to_cell(&mut self, cell_id: usize, entity: Entity) -> Result<(), OverflowError> {
        let range = &mut self.cell_ranges[cell_id];
        
        // Check if we have headroom (next write position must be < max_index)
        let next_write_pos = range.start_index + range.current_count;
        if next_write_pos < range.max_index {
            // Write to next available slot
            self.entity_storage[next_write_pos] = entity;
            range.current_count += 1;
            Ok(())
        } else {
            // Cell overflow - trigger rebuild
            Err(OverflowError::CellFull(cell_id))
        }
    }
    
    /// Update entity that moved from old_cell to new_cell
    /// Total cost: O(1) amortized
    pub fn update_entity_cell(
        &mut self, 
        entity: Entity, 
        old_cell: CellId, 
        new_cell: CellId
    ) -> Result<(), OverflowError> {
        // Remove from old (swap-based, no fragmentation!)
        self.remove_from_cell(old_cell.to_index(), entity);
        
        // Insert to new (uses headroom)
        self.insert_to_cell(new_cell.to_index(), entity)?;
        
        Ok(())
    }
}
```

**How Swap-Based Removal Avoids Fragmentation:**

Traditional removal creates gaps:
```
Before: [E1, E2, E3, E4, E5]
Remove E2 (naive): [E1, _, E3, E4, E5]  ❌ GAP!
```

Swap-based removal maintains contiguity:
```
Before: [E1, E2, E3, E4, E5]  (current_count = 5)
Remove E2 (swap): [E1, E5, E3, E4, _]  (current_count = 4)  ✅ NO GAP!
```

**Arena Headroom Distribution Strategy (Deprecated - Use Equal Headroom Instead):**

**IMPORTANT:** The strategy below distributes headroom proportionally, which creates unfairness.
The **preferred approach** is equal headroom distribution (see above), where all cells get
`(arena_length - total_entity_count) / num_cells` headroom after every rebuild.

```rust
// DEPRECATED: Proportional headroom (unfair, some cells starve)
fn rebuild_with_proportional_headroom(&mut self) {
    // Calculate total free space
    let total_capacity = self.entity_storage.capacity();
    let total_used = self.count_all_entities();
    let free_space = total_capacity - total_used;
    
    // Option A: Uniform distribution (RECOMMENDED - use equal headroom instead)
    let per_cell_buffer = free_space / self.cell_ranges.len();
    
    // Option B: Proportional to recent usage (NOT RECOMMENDED - creates unfairness)
    let per_cell_buffer = self.calculate_proportional_buffers(free_space);
    
    // Rebuild with new max_index values
    let mut write_pos = 0;
    for cell_id in 0..self.cell_ranges.len() {
        let count = self.cell_usage[cell_id];  // Entities currently in cell
        let headroom = per_cell_buffer[cell_id];  // Additional buffer
        
        self.cell_ranges[cell_id] = CellRange {
            start_index: write_pos,
            current_count: count,
            max_index: write_pos + count + headroom,
        };
        
        write_pos += count + headroom;
    }
}

fn calculate_proportional_buffers(&self, free_space: usize) -> Vec<usize> {
    // Allocate buffer proportional to recent cell occupancy
    // High-traffic cells get more headroom
    let total_usage: usize = self.cell_usage.iter().sum();
    
    self.cell_usage.iter().map(|&usage| {
        let min_buffer = 10;  // Every cell gets at least 10 slots
        let proportional = (usage as f32 / total_usage as f32 * free_space as f32) as usize;
        min_buffer.max(proportional)
    }).collect()
}
```

**Performance Comparison:**

| Scenario | Full Rebuild | Incremental | Speedup |
|----------|--------------|-------------|---------|
| 1M entities, 5% moved | ~5ms (1M processed) | ~0.5ms (50k processed) | **10×** |
| 5M entities, 5% moved | ~25ms (5M processed) | ~2.5ms (250k processed) | **10×** |
| 10M entities, 10% moved | ~50ms (10M processed) | ~5ms (1M processed) | **10×** |
| 10M entities, 50% moved | ~50ms (10M processed) | ~25ms (5M processed) | **2×** |

**Rebuild Trigger Conditions:**

Incremental updates eventually require full rebuild when:

1. **Cell Overflow:** Any cell exceeds `max_index`
2. **Global Fill:** Total arena usage > 85%
3. **Fragmentation:** (Very rare with swap-based removal)

```rust
fn should_trigger_rebuild(&self) -> bool {
    let global_usage = self.count_all_entities() as f32 / self.entity_storage.capacity() as f32;
    
    // Trigger if ANY cell overflowed (current_count reached headroom limit)
    let any_cell_full = self.cell_ranges.iter()
        .any(|r| r.start_index + r.current_count >= r.max_index);
    
    // Or if global capacity critically low
    let critically_full = global_usage > 0.85;
    
    any_cell_full || critically_full
}
```

**Double-Buffering for Zero-Latency Rebuilds:**

For >5M entities, rebuild in background to avoid frame spikes:

```rust
struct SpatialHash {
    active_grid: Arc<StaggeredGrid>,   // Used by queries (read-only)
    rebuild_grid: Arc<StaggeredGrid>,  // Being rebuilt (write-only)
    is_rebuilding: AtomicBool,
}

impl SpatialHash {
    fn start_rebuild(&mut self) {
        if !self.is_rebuilding.load(Ordering::Relaxed) {
            self.is_rebuilding.store(true, Ordering::Relaxed);
            
            // Spawn async rebuild task
            let entities = self.snapshot_entities();  // Copy entity list
            let rebuild_grid = Arc::clone(&self.rebuild_grid);
            
            spawn_task(move || {
                rebuild_grid.rebuild_from_snapshot(entities);
            });
        }
    }
    
    fn complete_rebuild(&mut self) {
        if self.rebuild_complete() {
            // Atomic pointer swap - instant cutover
            std::mem::swap(&mut self.active_grid, &mut self.rebuild_grid);
            self.is_rebuilding.store(false, Ordering::Relaxed);
        }
    }
}
```

**Configuration in initial_config.ron:**

```ron
// Spatial Hash Update Strategy
spatial_hash_update_strategy: Incremental,  // or FullRebuild

// Arena over-provisioning for incremental updates
// 1.5 = 50% extra capacity, 2.0 = 100% extra
// Higher ratios = fewer rebuilds but more memory
spatial_hash_arena_overcapacity_ratio: 1.5,

// Rebuild trigger thresholds
spatial_hash_rebuild_on_cell_overflow: true,
spatial_hash_rebuild_threshold: 0.85,  // Rebuild when 85% full

// Double-buffering for large-scale (>5M entities)
spatial_hash_use_double_buffering: false,  // Enable for >5M entities
spatial_hash_async_rebuild_threshold: 5_000_000,
```

**Strategy Selection Guidelines:**

| Entity Scale | Recommended Strategy | Config |
|--------------|---------------------|--------|
| <500k | Full Rebuild | `update_strategy: FullRebuild` |
| 500k-2M | Full Rebuild (monitor) | Same, watch rebuild time |
| 2M-5M | Incremental | `update_strategy: Incremental`<br>`overcapacity_ratio: 1.5` |
| 5M-10M | Incremental + Double-Buffer | `update_strategy: Incremental`<br>`use_double_buffering: true`<br>`overcapacity_ratio: 1.5` |
| >10M | Incremental + Double-Buffer + Parallel | Above + parallel region updates |

### 2.9 Spatial Hash Update Optimizations (Arena-Based)

**CRITICAL PERFORMANCE INSIGHT:**

With arena-based staggered grids, spatial hash updates use a **three-phase deferred architecture**:

1. **Hot Path (Queries):** READ-ONLY, zero allocation, <0.5ms
2. **Warm Path (Movement Detection):** Build moved_entities list, <5ms
3. **Cold Path (Apply Updates):** Modify arena storage, runs async/deferred

**Update Check Logic (Warm Path):**

```rust
fn detect_moved_entities(
    query: Query<(Entity, &SimPosition, &SimPositionPrev, &OccupiedCell)>,
    mut scratch: ResMut<SpatialHashScratch>,
    spatial_hash: Res<SpatialHash>,
) {
    scratch.moved_entities.clear();  // O(1), no dealloc
    
    for (entity, pos, prev, occupied) in query.iter() {
        // Quick check: Did position change at all?
        if pos.0 == prev.0 {
            continue;
        }
        
        // Calculate which grid/cell this entity should be in
        let size_class = &spatial_hash.size_classes[occupied.size_class as usize];
        let (new_grid_offset, new_col, new_row) = 
            size_class.calculate_best_cell(pos.0);
        
        // Only record if cell actually changed
        if new_grid_offset != occupied.grid_offset ||
           new_col != occupied.col ||
           new_row != occupied.row 
        {
            // CRITICAL: Check capacity before push
            if scratch.moved_entities.len() < scratch.moved_entities.capacity() {
                scratch.moved_entities.push(MovedEntity {
                    entity,
                    old_cell: *occupied,
                    new_grid_offset,
                    new_col,
                    new_row,
                });
            } else {
                #[cfg(debug_assertions)]
                panic!("Moved entities overflow: {} > {}", 
                       scratch.moved_entities.len(),
                       scratch.moved_entities.capacity());
                       
                #[cfg(not(debug_assertions))]
                warn_once!("Moved entities buffer full - updates dropped");
                break;
            }
        }
    }
}
```

**Apply Updates Logic (Cold Path):**

```rust
fn apply_deferred_spatial_updates(
    scratch: Res<SpatialHashScratch>,
    mut spatial_hash: ResMut<SpatialHash>,
    mut occupied_query: Query<&mut OccupiedCell>,
) {
    for moved in &scratch.moved_entities {
        // Get the appropriate grid for old and new cells
        let old_grid = spatial_hash.get_grid_mut(
            moved.old_cell.size_class,
            moved.old_cell.grid_offset,
        );
        
        // Remove from old cell in arena
        old_grid.remove_entity_from_cell(
            moved.old_cell.col,
            moved.old_cell.row,
            moved.old_cell.range_idx,
            moved.entity,
        );
        
        let new_grid = spatial_hash.get_grid_mut(
            moved.old_cell.size_class,  // Size class doesn't change
            moved.new_grid_offset,
        );
        
        // Insert into new cell in arena
        let new_range_idx = new_grid.insert_entity_into_cell(
            moved.new_col,
            moved.new_row,
            moved.entity,
        );
        
        // Update component
        if let Ok(mut occupied) = occupied_query.get_mut(moved.entity) {
            occupied.grid_offset = moved.new_grid_offset;
            occupied.col = moved.new_col;
            occupied.row = moved.new_row;
            occupied.range_idx = new_range_idx;
        }
    }
    
    // Check fragmentation and compact if needed
    for grid in spatial_hash.all_grids_mut() {
        if grid.fragmentation_ratio() > 0.2 {
            grid.compact_incremental(10_000);
        }
    }
}
```

**Performance Characteristics:**

| Metric | Value | Notes |
|--------|-------|-------|
| **Entities Checked** | All dynamic (~500k) | But 90%+ skip quickly |
| **Entities Moved** | ~5-10% (~25k-50k) | Typical movement patterns |
| **Update Threshold** | ~half_cell (~20 units) | Large hysteresis |
| **Warm Path Time** | <5ms | Just builds list |
| **Cold Path Time** | <20ms | Can run async |
| **Skip Rate** | 90-95% | Most entities don't change cells |

**No Complex Optimizations Needed:**
- No velocity-based prediction
- No distance thresholds  
- No multi-cell symmetric difference
- Just: "Am I closer to the other grid now?"

### 2.10 Parallel Spatial Hash Updates (Arena Architecture)

**Zero-Allocation Parallel Pattern:**

With arena-based storage, parallel updates use **per-thread scratch buffers** + single-threaded compaction:

**Phase 1: Parallel Movement Detection (Warm Path)**

```rust
fn detect_moved_entities_parallel(
    query: Query<(Entity, &SimPosition, &SimPositionPrev, &OccupiedCell)>,
    mut scratch_set: ResMut<ParallelScratchSet>,
    spatial_hash: Res<SpatialHash>,
) {
    let thread_count = std::thread::available_parallelism()
        .unwrap_or(NonZeroUsize::new(8).unwrap())
        .get();
    
    // Partition query across threads
    query.par_iter().for_each_init(
        || scratch_set.get_thread_local_buffer(),
        |thread_buffer, (entity, pos, prev, occupied)| {
            if pos.0 == prev.0 { return; }
            
            let size_class = &spatial_hash.size_classes[occupied.size_class as usize];
            let (new_grid_offset, new_col, new_row) = 
                size_class.calculate_best_cell(pos.0);
            
            if new_grid_offset != occupied.grid_offset ||
               new_col != occupied.col ||
               new_row != occupied.row 
            {
                // CRITICAL: Each thread has its own pre-allocated buffer
                if thread_buffer.len() < thread_buffer.capacity() {
                    thread_buffer.push(MovedEntity {
                        entity,
                        old_cell: *occupied,
                        new_grid_offset,
                        new_col,
                        new_row,
                    });
                }
            }
        },
    );
    
    // Merge thread buffers into main scratch (single-threaded, fast)
    scratch_set.merge_into_main();
}
```

**Phase 2: Single-Threaded Update Application (Cold Path)**

```rust
fn apply_deferred_updates_sequential(
    scratch: Res<SpatialHashScratch>,
    mut spatial_hash: ResMut<SpatialHash>,
    mut occupied_query: Query<&mut OccupiedCell>,
) {
    // CRITICAL: Arena updates MUST be single-threaded
    // Parallel writes to same arena cause data races
    
    for moved in &scratch.moved_entities {
        // Remove from old cell
        let old_grid = spatial_hash.get_grid_mut(
            moved.old_cell.size_class,
            moved.old_cell.grid_offset,
        );
        old_grid.remove_entity_from_cell(
            moved.old_cell.col,
            moved.old_cell.row,
            moved.old_cell.range_idx,
            moved.entity,
        );
        
        // Insert into new cell
        let new_grid = spatial_hash.get_grid_mut(
            moved.old_cell.size_class,
            moved.new_grid_offset,
        );
        let new_range_idx = new_grid.insert_entity_into_cell(
            moved.new_col,
            moved.new_row,
            moved.entity,
        );
        
        // Update component
        if let Ok(mut occupied) = occupied_query.get_mut(moved.entity) {
            occupied.grid_offset = moved.new_grid_offset;
            occupied.col = moved.new_col;
            occupied.row = moved.new_row;
            occupied.range_idx = new_range_idx;
        }
    }
}
```

**Per-Thread Scratch Buffer Structure:**

```rust
pub struct ParallelScratchSet {
    // One pre-allocated buffer per hardware thread
    thread_buffers: Vec<Vec<MovedEntity>>,
    
    // Main merged buffer (cleared and reused)
    main_buffer: Vec<MovedEntity>,
}

impl ParallelScratchSet {
    pub fn new(thread_count: usize, max_moved_per_thread: usize) -> Self {
        let mut thread_buffers = Vec::with_capacity(thread_count);
        for _ in 0..thread_count {
            thread_buffers.push(Vec::with_capacity(max_moved_per_thread));
        }
        
        Self {
            thread_buffers,
            main_buffer: Vec::with_capacity(thread_count * max_moved_per_thread),
        }
    }
    
    pub fn get_thread_local_buffer(&mut self) -> &mut Vec<MovedEntity> {
        let thread_id = get_thread_id() % self.thread_buffers.len();
        &mut self.thread_buffers[thread_id]
    }
    
    pub fn merge_into_main(&mut self) {
        self.main_buffer.clear();  // O(1), no dealloc
        for buffer in &self.thread_buffers {
            self.main_buffer.extend_from_slice(buffer);  // Pre-allocated
            buffer.clear();  // Reuse next frame
        }
    }
}
```

**Why This Architecture:**

1. **Movement Detection** = Embarrassingly parallel (read-only queries)
2. **Arena Updates** = MUST be single-threaded (data races otherwise)
3. **Per-Thread Buffers** = Zero allocation, perfect cache locality
4. **Merge Step** = Fast (~1ms for 8 threads × 10k entities/thread)

**Performance Characteristics:**

| Operation | Time (500k entities) | Parallelism | Allocation |
|-----------|---------------------|-------------|------------|
| Movement Detection | ~2ms (8 threads) | Parallel | Zero |
| Buffer Merge | ~1ms | Single | Zero |
| Arena Updates | ~15ms | Single | Zero |
| **Total** | **~18ms** | Hybrid | **Zero** |

**Configuration (initial_config.ron):**

```ron
spatial_hash: SpatialHashConfig(
    parallel_scratch: ParallelScratchConfig(
        thread_count: 8,
        max_moved_per_thread: 15000,  // ~10% of 500k / 8 threads
    ),
),
```

### 2.11 Query Optimization and Radius Searches

**Zero-Allocation Query Pattern:**

With arena-based storage, radius queries use **scratch buffers** to avoid allocating result vectors:

```rust
pub fn query_radius(
    &self,
    center: FixedVec2,
    radius: FixedNum,
    size_class_idx: u8,
    scratch: &mut Vec<Entity>,
) -> &[Entity] {
    scratch.clear();  // O(1), no dealloc
    
    let size_class = &self.size_classes[size_class_idx as usize];
    let (grid_offset, col, row) = size_class.calculate_best_cell(center);
    let grid = size_class.get_grid(grid_offset);
    
    // Query 3×3 cell neighborhood
    let radius_cells = (radius.to_num::<f32>() / grid.cell_size as f32).ceil() as usize;
    
    for dy in -(radius_cells as i32)..=(radius_cells as i32) {
        for dx in -(radius_cells as i32)..=(radius_cells as i32) {
            let query_col = (col as i32 + dx).clamp(0, grid.cols as i32 - 1) as usize;
            let query_row = (row as i32 + dy).clamp(0, grid.rows as i32 - 1) as usize;
            
            // READ-ONLY access to arena storage via cell range
            let cell_idx = query_row * grid.cols + query_col;
            let range = &grid.cell_ranges[cell_idx];
            
            if range.current_count > 0 {
                // CRITICAL: Check capacity before extend
                let entities = &grid.entity_storage[range.start_index..range.start_index + range.current_count];
                
                if scratch.len() + entities.len() <= scratch.capacity() {
                    scratch.extend_from_slice(entities);
                } else {
                    #[cfg(debug_assertions)]
                    panic!("Query scratch overflow: {} + {} > {}",
                           scratch.len(), entities.len(), scratch.capacity());
                    
                    #[cfg(not(debug_assertions))]
                    warn_once!("Query scratch buffer full - results truncated");
                    break;
                }
            }
        }
    }
    
    &scratch[..]  // Return slice (no allocation)
}
```

**Typical Usage Pattern:**

```rust
fn boids_system(
    mut query: Query<(&SimPosition, &Collider, &mut Velocity)>,
    spatial_hash: Res<SpatialHash>,
    mut scratch: ResMut<QueryScratch>,
) {
    for (pos, collider, mut velocity) in query.iter_mut() {
        // Reuse scratch buffer across all queries
        let neighbors = spatial_hash.query_radius(
            pos.0,
            collider.radius * 3.0,  // Search 3× unit radius
            collider.size_class,
            &mut scratch.buffer,
        );
        
        // Process neighbors (read-only, no copy)
        for &neighbor_entity in neighbors {
            // ... boids logic ...
        }
        
        // scratch.buffer.clear() called by next query_radius()
    }
}
```

**Query Scratch Resource:**

```rust
pub struct QueryScratch {
    // Pre-allocated buffer reused by all queries
    pub buffer: Vec<Entity>,
}

impl QueryScratch {
    pub fn new(max_results: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(max_results),
        }
    }
}
```

**Performance Characteristics:**

| Metric | Value | Notes |
|--------|-------|-------|
| **Query Time** | <0.1ms | 3×3 cell neighborhood, ~50 entities |
| **Cache Locality** | Excellent | Arena storage is contiguous |
| **Allocation** | Zero | Scratch buffer reused |
| **Typical Results** | 10-100 entities | Depends on density |
| **Max Results** | 500 entities | Configured in initial_config.ron |

**Neighbor Caching Pattern:**

For expensive queries (boids, pathfinding), cache results in components:

```rust
#[derive(Component)]
pub struct CachedNeighbors {
    pub entities: Vec<Entity>,
    pub last_update_tick: u64,
    pub update_interval: u64,  // e.g., every 5 ticks
}

fn cache_neighbors_system(
    mut query: Query<(&SimPosition, &Collider, &mut CachedNeighbors)>,
    spatial_hash: Res<SpatialHash>,
    mut scratch: ResMut<QueryScratch>,
    tick: Res<GameTick>,
) {
    for (pos, collider, mut cached) in query.iter_mut() {
        // Only update cache every N ticks
        if tick.0 - cached.last_update_tick >= cached.update_interval {
            cached.entities.clear();
            
            let neighbors = spatial_hash.query_radius(
                pos.0,
                collider.radius * 3.0,
                collider.size_class,
                &mut scratch.buffer,
            );
            
            // Copy to component cache (infrequent, acceptable allocation)
            cached.entities.extend_from_slice(neighbors);
            cached.last_update_tick = tick.0;
        }
    }
}
```

**Configuration (initial_config.ron):**

```ron
spatial_hash: SpatialHashConfig(
    query_scratch: QueryScratchConfig(
        max_results: 500,  // Per-query result limit
    ),
    
    neighbor_cache: NeighborCacheConfig(
        update_interval: 5,  // Cache lifetime in ticks
        max_cached: 100,     // Max neighbors per entity
    ),
),
```

**Query Optimization Rules:**

1. **Reuse Scratch Buffers**: Never allocate Vec per query
2. **Cache Expensive Queries**: Boids, pathfinding neighbor searches
3. **Stale Caches Are OK**: 5-tick-old neighbors still 95% accurate
4. **Check Capacity**: Always validate before `extend_from_slice()`
5. **Return Slices**: `&[Entity]` not `Vec<Entity>`

### 2.12 Parallel Reads (Already Optimal)
### 2.12 Parallel Reads (Already Optimal)

Unlike writes, **spatial hash reads are fully parallelizable** with arena-based architecture:

```rust
// Multiple systems can query simultaneously (read-only)
spatial_hash: Res<SpatialHash>  // Shared read access

// Example: Collision system and boids system query in parallel
par_iter(collision_entities).for_each(|(entity, pos, collider, scratch)| {
    let neighbors = spatial_hash.query_radius(
        pos.0,
        collider.radius * 2.0,
        collider.size_class,
        scratch,  // Each thread has own scratch buffer
    );
    // ✅ Thread-safe - reads from arena storage
});

par_iter(boids_entities).for_each(|(entity, pos, collider, scratch)| {
    let neighbors = spatial_hash.query_radius(
        pos.0,
        collider.radius * 3.0,
        collider.size_class,
        scratch,  // Each thread has own scratch buffer
    );
    // ✅ Thread-safe - reads from arena storage
});
```

**Why Arena Reads Are Fast:**

1. **Shared Reference**: `Res<SpatialHash>` provides `&SpatialHash` (no mutation)
2. **Contiguous Memory**: Arena storage (`Vec<Entity>`) has perfect cache locality
3. **Lock-Free**: No mutexes, no atomic operations
4. **Per-Thread Scratch**: Each thread has its own result buffer
5. **SIMD-Friendly**: Contiguous arrays enable vectorization

**Performance Characteristics:**

| Operation | Time (per query) | Parallel Speedup | Notes |
|-----------|------------------|------------------|-------|
| Cell Lookup | ~5ns | N/A | Array index, instant |
| Range Read | ~20ns | 8× | Memcpy from arena |
| Distance Filter | ~50ns | 8× | SIMD-friendly loop |
| **Total** | **<0.1ms** | **8×** | Scales linearly with cores |

**Per-Thread Scratch Pattern:**

```rust
fn parallel_query_system(
    entities: Query<(&SimPosition, &Collider)>,
    spatial_hash: Res<SpatialHash>,
    mut thread_scratches: ResMut<ThreadScratchSet>,
) {
    entities.par_iter().for_each_init(
        || thread_scratches.get_thread_scratch(),
        |scratch, (pos, collider)| {
            let neighbors = spatial_hash.query_radius(
                pos.0,
                collider.radius * 3.0,
                collider.size_class,
                scratch,  // Owned by this thread
            );
            
            // Process neighbors...
        },
    );
}

pub struct ThreadScratchSet {
    scratches: Vec<Vec<Entity>>,  // One per hardware thread
}

impl ThreadScratchSet {
    pub fn new(thread_count: usize, max_results: usize) -> Self {
        let mut scratches = Vec::with_capacity(thread_count);
        for _ in 0..thread_count {
            scratches.push(Vec::with_capacity(max_results));
        }
        Self { scratches }
    }
    
    pub fn get_thread_scratch(&mut self) -> &mut Vec<Entity> {
        let thread_id = get_thread_id() % self.scratches.len();
        &mut self.scratches[thread_id]
    }
}
```

**Configuration (initial_config.ron):**

```ron
spatial_hash: SpatialHashConfig(
    thread_scratch: ThreadScratchConfig(
        thread_count: 8,
        max_results_per_thread: 500,
    ),
),
```

**Neighbor Caching with Arena Reads:**

For expensive multi-query systems (boids, pathfinding), cache results in components to reduce query frequency:

```rust
#[derive(Component)]
pub struct CachedNeighbors {
    pub entities: Vec<Entity>,
    pub last_update_tick: u64,
    pub update_interval: u64,  // e.g., 5 ticks
}

fn cache_neighbors_system(
    mut query: Query<(&SimPosition, &Collider, &mut CachedNeighbors)>,
    spatial_hash: Res<SpatialHash>,
    mut scratch: ResMut<QueryScratch>,
    tick: Res<GameTick>,
) {
    for (pos, collider, mut cached) in query.iter_mut() {
        if tick.0 - cached.last_update_tick >= cached.update_interval {
            cached.entities.clear();
            
            // Query arena (fast, zero-allocation)
            let neighbors = spatial_hash.query_radius(
                pos.0,
                collider.radius * 3.0,
                collider.size_class,
                &mut scratch.buffer,
            );
            
            // Copy to cache (infrequent, acceptable allocation)
            cached.entities.extend_from_slice(neighbors);
            cached.last_update_tick = tick.0;
        }
    }
}
```

**Cache Hit Rate Analysis:**

| System | Update Interval | Cache Hit Rate | Queries Saved |
|--------|-----------------|----------------|---------------|
| Boids (flocking) | 5 ticks | 80% | 400k/tick → 100k/tick |
| Pathfinding | 10 ticks | 90% | 50k/tick → 5k/tick |
| Collision | 1 tick (no cache) | N/A | Always fresh |

**Key Insight:**

With arena-based storage, **reads are embarrassingly parallel** - no synchronization overhead, perfect cache locality, and linear scaling with CPU cores. The bottleneck shifts entirely to the **warm/cold update paths** (which are already deferred and optimized).

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

/// One grid in a staggered pair (Arena-Based)
struct StaggeredGrid {
    // ARENA STORAGE: Single contiguous Vec for all entities
    entity_storage: Vec<Entity>,
    
    // CELL RANGES: Track which slice of entity_storage belongs to each cell
    cell_ranges: Vec<CellRange>,  // Length = cols × rows
    
    cols: usize,
    rows: usize,
    cell_size: FixedNum,
    offset: FixedVec2,        // Grid A: (0, 0), Grid B: (cell_size/2, cell_size/2)
}

/// Tracks a cell's slice in the arena
struct CellRange {
    start_index: usize,    // Index into entity_storage where this cell begins
    current_count: usize,  // Number of entities in this cell
}

/// Component tracking entity's location
#[derive(Component)]
struct OccupiedCell {
    size_class: u8,    // Index into size_classes array
    grid_offset: u8,   // 0 = Grid A, 1 = Grid B
    col: usize,        // Cell column
    row: usize,        // Cell row
    range_idx: usize,  // Index within cell's range (for O(1) removal)
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

#### Memory Layout Optimization (Arena Architecture)

**Arena Per Grid for Cache Efficiency:**

```rust
struct StaggeredGrid {
    // CRITICAL: Single contiguous arena for perfect cache locality
    entity_storage: Vec<Entity>,
    cell_ranges: Vec<CellRange>,
    
    cols: usize,
    rows: usize,
    cell_size: FixedNum,
    offset: FixedVec2,
}

impl StaggeredGrid {
    /// Query entities in a cell (READ-ONLY, zero-allocation)
    pub fn query_cell(&self, col: usize, row: usize) -> &[Entity] {
        let cell_idx = row * self.cols + col;
        let range = &self.cell_ranges[cell_idx];
        
        if range.current_count > 0 {
            &self.entity_storage[range.start_index..range.start_index + range.current_count]
        } else {
            &[]  // Empty slice
        }
    }
    
    /// Query 3×3 neighborhood (typical collision radius)
    pub fn query_neighborhood(
        &self,
        center_col: usize,
        center_row: usize,
        scratch: &mut Vec<Entity>,
    ) -> &[Entity] {
        scratch.clear();
        
        for dy in -1..=1 {
            for dx in -1..=1 {
                let col = (center_col as i32 + dx).clamp(0, self.cols as i32 - 1) as usize;
                let row = (center_row as i32 + dy).clamp(0, self.rows as i32 - 1) as usize;
                
                let cell_idx = row * self.cols + col;
                let range = &self.cell_ranges[cell_idx];
                
                if range.current_count > 0 {
                    let entities = &self.entity_storage[range.start_index..range.start_index + range.current_count];
                    
                    // CRITICAL: Check capacity before extend
                    if scratch.len() + entities.len() <= scratch.capacity() {
                        scratch.extend_from_slice(entities);
                    } else {
                        warn_once!("Query scratch overflow");
                        break;
                    }
                }
            }
        }
        
        &scratch[..]
    }
}
```

**Benefits:**

1. **Cache Locality**: Contiguous arena means querying adjacent cells hits same cache lines
2. **Zero Allocation**: Queries return slices, never allocate
3. **SIMD-Friendly**: Contiguous Entity arrays enable auto-vectorization
4. **Defragmentation**: Periodic compaction maintains contiguity

**Memory Comparison (500k entities, 3 size classes, 2 grids each):**

| Approach | Entity Storage | Cell Metadata | Total | Notes |
|----------|----------------|---------------|-------|-------|
| **Naive (Vec<Vec>)** | 9.6 GB | 480 MB | **10.1 GB** | 64-capacity per cell, massive waste |
| **Arena (Current)** | 1.6 GB | 96 MB | **1.7 GB** | 5.6× more efficient |
| **Single Grid (Broken)** | 800 MB | 48 MB | 848 MB | Misses large entities |

**Why Arena Beats Vec<Vec>:**

- **Vec<Vec>** preallocates capacity per cell → most cells underutilized
- **Arena** shares capacity across all cells → no per-cell overhead
- Example: 10k cells × 64 capacity = 640k slots (500k used, 140k wasted)
- Arena: 500k slots exactly (0 wasted after compaction)

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
