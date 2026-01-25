# Pathfinding Arena Optimization Design Document

**Date:** January 25, 2026  
**Status:** Design Phase  
**Goal:** Eliminate Path component insertion/removal and optimize iteration over active paths using a two-layer arena system

---

## Problem Statement

### Current Implementation Issues

1. **Component Churn**: Path components are constantly added and removed as units navigate, causing:
   - Memory allocations/deallocations
   - ECS archetype changes (expensive structural changes)
   - Cache invalidation
   - Fragmentation

2. **Inefficient Iteration**: 
   - `follow_path()` iterates over all entities with Path components
   - `cleanup_completed_paths()` scans all Path entities to find completed/blocked ones
   - With 10M entities, even a small percentage with paths = millions of iterations
   - No way to efficiently query only "active" paths without scanning everything

3. **Path Request Processing**:
   - Currently uses `get_mut()` per entity in `process_path_requests()`
   - Works but could be optimized with arena-based batch processing

### Performance Targets

- Support up to 10M total entities
- Expect ~1-10% entities with active paths at any time (100K-1M active)
- Path requests per frame: ~1K-10K (comparatively small)
- Goal: O(active_paths) iteration, not O(total_entities)

---

## Proposed Solution: Two-Layer Active Path Arena

### Architecture Overview

**Strategic Design Decision:** While `Box<[Entity]>` is the fastest single-tier solution, we implement a **configurable two-tier architecture** for:
1. **Memory Flexibility**: Different arenas can use different configurations (some with hot tier, some bitset-only)
2. **Reusability**: Generic design applicable to many "active entity set" problems beyond pathfinding
3. **Future-Proofing**: Easy to switch between space-saving (bitset-only) and performance (hot-tier) without API changes
4. **Clean Abstraction**: Clients never know which tier they're using - implementation detail hidden

```
┌─────────────────────────────────────────────────────────┐
│  ActivePathArena (Public API)                           │
│  ─────────────────────────────────────────────────────  │
│  Unified interface regardless of internal tiers         │
├─────────────────────────────────────────────────────────┤
│  - register_active(Entity)                              │
│  - mark_inactive(Entity)                                │
│  - sweep_inactive()                                     │
│  - iter() -> Iterator<Entity>                          │
│  - count() -> usize                                     │
└─────────────────────────────────────────────────────────┘
                         │
         ┌───────────────┴───────────────┐
         │                               │
    ┌────▼────────┐              ┌───────▼──────────┐
    │  HotArena   │              │  FallbackBitset  │
    │  (Primary)  │              │   (Overflow)     │
    ├─────────────┤              ├──────────────────┤
    │ Box<[u32]>  │              │ FixedBitSet      │
    │ capacity    │              │ (MAX_ENTITIES)   │
    │ len         │              │                  │
    │ tombstones  │              │                  │
    └─────────────┘              └──────────────────┘
         │                               │
         └───────────────┬───────────────┘
                         │
          (Graceful overflow, transparent to client)
```

### Layer 1: Hot Arena (Primary Storage)

**Purpose:** Fast iteration over the most common case (< 10% entities with paths)

**Data Structure:**
```rust
struct HotArena {
    /// Packed array of entity IDs (raw u32 indices)
    entities: Box<[u32]>,
    
    /// Current number of active entities
    len: usize,
    
    /// Capacity (e.g., MAX_ENTITIES / 10)
    capacity: usize,
    
    /// Tombstone tracking for two-phase removal
    tombstones: FixedBitSet,
    
    /// Optional: reverse lookup Entity -> index in array
    /// (Only if we need O(1) mark_inactive)
    entity_to_index: Option<HashMap<u32, usize>>,
}
```

**Operations:**
- `register_active(entity)`: Add to end if space, else overflow to bitset
- `mark_inactive(entity)`: Set tombstone bit (two-phase removal)
- `sweep()`: Compact array by moving live entities forward, removing tombstones
- `iter()`: Iterate 0..len, skip tombstones
- `is_full()`: len >= capacity

**Complexity:**
- Register: O(1) append (O(1) with reverse lookup, O(n) without)
- Mark inactive: O(1) with reverse lookup, O(n) linear search without
- Sweep: O(len) - amortized cost
- Iteration: O(len)

**Tombstone System (Mark-and-Sweep):**
- Marks entities for removal without immediate compaction
- Allows batch processing: mark many, then sweep once
- Sweep moves live entities to front, updates len

**Two-Pass Compaction Algorithm:**
```rust
fn sweep_tombstones(arena: &mut HotArena) {
    let mut write_idx = 0;
    
    // Pass 1: Linear read, skip tombstones
    for read_idx in 0..arena.len {
        if !arena.tombstones.get(read_idx) {
            // Pass 2: Write live entities to front
            if read_idx != write_idx {
                arena.entities[write_idx] = arena.entities[read_idx];
            }
            write_idx += 1;
        }
    }
    
    arena.len = write_idx;
    arena.tombstones.clear(); // Reset for next cycle
}
```

**Why This Is Fast:**
- **Cache Friendly**: Linear read + linear write = perfect prefetching
- **Minimal Writes**: Each live entity moved exactly once
- **Batch Efficiency**: 1,000 removals = 1 sweep operation, not 1,000 swaps
- **SIMD Potential**: Tombstone scanning can use bitset SIMD instructions (POPCNT, TZCNT)

### Layer 2: Fallback Bitset (Overflow)

**Purpose:** Handle cases where > capacity entities have active paths

**Data Structure:**
```rust
struct FallbackBitset {
    /// One bit per possible entity (Entity ID as index)
    bits: FixedBitSet,
    
    /// Count of set bits (active entities)
    count: usize,
}
```

**Operations:**
- `register_active(entity_id)`: Set bit at entity_id
- `mark_inactive(entity_id)`: Clear bit at entity_id
- `iter()`: Iterate set bits, convert to Entity
- `count()`: Return count

**Complexity:**
- Register/Mark: O(1)
- Iteration: O(MAX_ENTITIES / 64) - bitset iteration
- Count: O(1) cached

**When Used:**
- Only when HotArena is full
- Transparent to caller
- Entities can migrate back to HotArena during sweep if space freed

### Public API (Unified Interface)

```rust
pub struct ActivePathArena {
    hot: HotArena,
    fallback: FallbackBitset,
    config: ArenaConfig,
}

pub struct ArenaConfig {
    hot_capacity: Option<usize>,      // None = bitset-only mode (saves 80MB)
    enable_fallback: bool,             // false = hot-only mode (faster but fixed cap)
    enable_reverse_lookup: bool,       // hot arena reverse map for O(1) removal
    sorted: bool,                      // keep hot arena sorted (cache locality)
}

// Example configurations:
// High-performance RTS (Active Path Arena): hot_capacity: Some(1M), enable_fallback: true
// Memory-constrained (UI state tracking): hot_capacity: None, enable_fallback: true (bitset-only)
// Fixed small set (ability cooldowns): hot_capacity: Some(10K), enable_fallback: false

impl ActivePathArena {
    /// Create arena with configuration
    pub fn new(max_entities: usize, config: ArenaConfig) -> Self;
    
    /// Register entity as having active path
    pub fn register_active(&mut self, entity: Entity);
    
    /// Mark entity as no longer having active path (lazy removal)
    pub fn mark_inactive(&mut self, entity: Entity);
    
    /// Remove all marked entities (batch sweep)
    pub fn sweep_inactive(&mut self);
    
    /// Iterate over all active entities
    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_;
    
    /// Count of active entities
    pub fn count(&self) -> usize;
    
    /// Check if entity is registered (for debugging)
    pub fn contains(&self, entity: Entity) -> bool;
}
```

**Helper Functions:**
```rust
// Hide Entity::from_raw() implementation details
fn entity_id_to_entity(id: u32) -> Entity;
fn entity_to_entity_id(entity: Entity) -> u32;
```

### Entity Recycling and Generations

**Critical Detail:** Bevy uses generational indices for Entity IDs:
- **Entity = Index (32-bit) + Generation (32-bit)**
- When an entity is destroyed, its index goes into a free list
- When a new entity is spawned, it reuses the index but increments generation
- Example: Old unit at Index 5 Gen 1 → New unit at Index 5 Gen 2

**Impact on Arena Design:**
- Using `Box<[Entity]>` automatically handles this - `query.get(entity)` checks generation
- Using `FixedBitSet` (index-only) requires manual generation validation
- This is why `Box<[Entity]>` is safer and simpler despite 80MB cost

**Cleanup Integration:**
```rust
fn cleanup_on_entity_removal(
    mut removals: RemovedComponents<Path>,
    mut active_paths: ResMut<ActivePathArena>,
) {
    for entity in removals.read() {
        active_paths.mark_inactive(entity);
    }
    active_paths.sweep_inactive();
}
```

### Why Not Marker Components?

**Alternative Approach:** Use a marker component like `ActivePath` and query `With<ActivePath>`

**Why This Doesn't Work at 10M Scale:**
1. **Archetype Fragmentation**: Adding/removing components moves entities between memory tables
2. **Structural Changes**: Each insert/remove triggers expensive ECS reorganization
3. **Frame Stutters**: 1,000 units starting paths = 1,000 archetype moves = massive spike
4. **Cache Invalidation**: Moving entities ruins spatial locality

**Bevy Archetype System:**
- Entities with same components = same archetype (same memory table)
- Adding component = move entity to new table
- At 10M entities with constant path start/stop, this becomes a "silent killer"

**Verdict:** Marker components are idiomatic Bevy, but NOT suitable for high-frequency changes at RTS scale

### Design Decisions

#### 1. Should HotArena be sorted?

**Pros:**
- Better cache locality if entities are processed in order
- Enables binary search for contains()
- Potential for better prefetching

**Cons:**
- Insertion becomes O(n) instead of O(1)
- Sweep becomes more complex
- Unlikely to matter if we use reverse lookup

**Decision:** Make it configurable via `ArenaConfig::sorted`
- Default: unsorted (O(1) append)
- Can enable if profiling shows cache benefits

#### 2. Should we maintain reverse lookup (Entity -> index)?

**Pros:**
- O(1) mark_inactive instead of O(n)
- Critical if we have frequent path cancellations

**Cons:**
- Extra memory overhead (HashMap<u32, usize>)
- Must maintain on sweep/register

**Decision:** Make it configurable via `ArenaConfig::enable_reverse_lookup`
- Default: enabled (worth the memory for O(1) removal)
- Can disable for memory-constrained scenarios

#### 3. When to sweep tombstones?

**Options:**
- After every N marks
- When tombstone ratio > threshold (e.g., 25%)
- Manually called once per frame

**Decision:** Manual sweep once per frame
- Caller controls when to pay compaction cost
- Can be done at frame boundary (not in hot loop)
- Predictable performance

#### 4. Migration between layers?

**Scenario:** HotArena fills up, entities overflow to bitset. Later, many paths complete, HotArena has space.

**Decision:** One-way overflow for simplicity (v1)
- Entities stay in fallback once they overflow
- Sweep does NOT migrate back to hot arena
- Keeps logic simple, still performs well
- **Key Advantage**: Fallback tier gracefully handles pathological cases without client code changes

**Future:** Two-way migration (v2)
- During sweep, if hot arena has space and fallback has entities, migrate some back
- Maintains hot arena utilization
- More complex, but better for scenarios with bursty path usage

**Why Two-Tier Despite Box<[Entity]> Being Fastest:**
- **Not all arenas need 80MB**: Some systems (e.g., active abilities, animations) might only need bitset
- **Configuration flexibility**: Switch between performance/memory profiles via `ArenaConfig` at startup
- **Reusability**: Same generic arena code serves multiple systems with different needs
- **Client isolation**: Systems using the arena don't need to know or care about internal structure

---

## Implementation Plan

### Phase 1: Core Arena Implementation

**Files to Create:**
- `src/game/pathfinding/active_path_arena.rs` - Main arena logic
- Add module to `src/game/pathfinding/mod.rs`

**Steps:**
1. Implement `HotArena` struct and methods
2. Implement `FallbackBitset` struct and methods
3. Implement `ActivePathArena` unified interface
4. Add batch insertion/removal methods
5. Write unit tests for all operations
6. Write benchmark comparing iteration performance

**Batch Operations to Implement:**
```rust
impl ActivePathArena {
    /// Add multiple entities at once (memcpy optimization)
    pub fn register_active_batch(&mut self, entities: &[Entity]);
    
    /// Mark multiple entities for removal (integrates with RemovedComponents)
    pub fn mark_inactive_batch(&mut self, entities: &[Entity]);
    
    /// Efficient sweep using mark-and-sweep compaction
    pub fn sweep_inactive(&mut self);
}
```

**Integration with Bevy Removal System:**
```rust
fn handle_entity_removals(
    mut removals: RemovedComponents<Path>,
    mut active_paths: ResMut<ActivePathArena>,
) {
    if removals.is_empty() { return; }
    
    // Collect all removed entities
    let removed: Vec<Entity> = removals.read().collect();
    
    // Batch mark them
    active_paths.mark_inactive_batch(&removed);
    
    // Single sweep operation
    active_paths.sweep_inactive();
}
```

**Estimated Complexity:** Medium (250-35icity (v1)
- Entities stay in fallback once they overflow
- Sweep does NOT migrate back to hot arena
- Keeps logic simple, still performs well

**Future:** Two-way migration (v2)
- During sweep, if hot arena has space and fallback has entities, migrate some back
- Maintains hot arena utilization
- More complex, but better for scenarios with bursty path usage

---

## Implementation Plan

### Phase 1: Core Arena Implementation

**Files to Create:**
- `src/game/pathfinding/active_path_arena.rs` - Main arena logic
- Add module to `src/game/pathfinding/mod.rs`

**Steps:**
1. Implement `HotArena` struct and methods
2. Implement `FallbackBitset` struct and methods
3. Implement `ActivePathArena` unified interface
4. Write unit tests for all operations
5. Write benchmark comparing iteration performance

**Estimated Complexity:** Medium (200-300 LOC + tests)

### Phase 2: Path Component Changes

**Current State:**
```rust
// Path is added/removed dynamically
enum Path {
    Active(PathState),
    Completed,
    Blocked,
}
```

**New State:**
```rust
// Path exists on all potential path-following entities from spawn
enum Path {
    Active(PathState),
    Inactive,  // Replaces removal
}
```

**Changes:**
1. Add `Path::Inactive` variant
2. Remove all `commands.entity(entity).insert(Path::Active(...))` 
3. Replace with mutation: `path = Path::Active(...)`
4. Remove all `commands.entity(entity).remove::<Path>()`
5. Replace with `path = Path::Inactive`
6. Update queries to filter `With<Path>` where needed

**Files to Update:**
- `src/game/pathfinding/navigation.rs` - Remove cleanup system
- `src/game/pathfinding/systems.rs` - Mutate instead of insert
- `src/game/simulation/components.rs` - Add Path to unit bundles
- Any other files that insert/remove Path

### Phase 3: Integrate Arena with Pathfinding Systems

**3.1: Add Arena as Resource**
```rust
// In mod.rs or resources.rs
#[derive(Resource)]
pub struct ActivePathArena { ... }

// In plugin setup
fn setup_pathfinding(mut commands: Commands) {
    let config = ArenaConfig {
        hot_capacity: Some(MAX_ENTITIES / 10),
        enable_fallback: true,
        enable_reverse_lookup: true,
        sorted: false,
    };
    commands.insert_resource(ActivePathArena::new(MAX_ENTITIES, config));
}
```

**3.2: Update process_path_requests()**

**Current:**
```rust
pub fn process_path_requests(
    mut commands: Commands,
    mut requests: Query<(Entity, &SimPosition, &mut PathRequest)>,
    // ...
) {
    for (entity, pos, mut req) in requests.iter_mut() {
        // ... pathfinding logic ...
        commands.entity(entity).insert(Path::Active(state));
        commands.entity(entity).remove::<PathRequest>();
    }
}
```

**New:**
```rust
pub fn process_path_requests(
    mut requests: Query<(Entity, &SimPosition, &mut PathRequest, &mut Path)>,
    mut active_paths: ResMut<ActivePathArena>,
    // ...
) {
    for (entity, pos, mut req, mut path) in requests.iter_mut() {
        // ... pathfinding logic ...
        *path = Path::Active(state);
        active_paths.register_active(entity);
        *req = PathRequest::None; // Or use Inactive variant
    }
}
```

**3.3: Update follow_path()**

**Current:**
```rust
pub fn follow_path(
    mut query: Query<(&SimPosition, &SimVelocity, &mut SimAcceleration, &mut Path, &GoalNavCell)>,
    // ...
) {
    for (pos, vel, mut acc, mut path, goal_nav_cell) in query.iter_mut() {
        let Path::Active(ref mut state) = *path else {
            continue; // Skip inactive
        };
        // ... navigation logic ...
        
        // Arrival or blocked:
        *path = Path::Completed; // or Path::Blocked
    }
}
```

**New:**
```rust
pub fn follow_path(
    active_paths: Res<ActivePathArena>,
    mut query: Query<(&SimPosition, &SimVelocity, &mut SimAcceleration, &mut Path, &GoalNavCell)>,
    // ...
) {
    for entity in active_paths.iter() {
        let Ok((pos, vel, mut acc, mut path, goal_nav_cell)) = query.get_mut(entity) else {
            warn!("Active path entity {} missing components!", entity);
            continue;
        };
        
        let Path::Active(ref mut state) = *path else {
            // Shouldn't happen, but handle gracefully
            warn!("Active path entity {} has inactive path!", entity);
            continue;
        };
        
        // ... navigation logic ...
        
        // Arrival or blocked:
        *path = Path::Inactive;
        // Don't mark inactive yet - defer to sweep
    }
}
```

**3.4: Add sweep system**

```rust
/// Batch remove inactive paths from arena (runs once per frame after follow_path)
pub fn sweep_inactive_paths(
    mut active_paths: ResMut<ActivePathArena>,
    paths: Query<(Entity, &Path)>,
) {
    // Mark all inactive entities
    for entity in active_paths.iter() {
        if let Ok((_, path)) = paths.get(entity) {
            if matches!(path, Path::Inactive) {
                active_paths.mark_inactive(entity);
            }
        }
    }
    
    // Batch sweep
    active_paths.sweep_inactive();
}
```

**System Ordering:**
```rust
app.add_systems(Update, (
    process_path_requests,
    follow_path,
    sweep_inactive_paths, // Run last
).chain());
```

### Phase 4: Entity Spawning Updates

**Current:** Entities spawn without Path component, it's added on first request

**New:** All potentially-pathfinding entities spawn with Path::Inactive

**Files to Update:**
- `src/game/simulation/mod.rs` or wherever units are spawned
- Add Path::Inactive to unit bundles
- Document that Path is a "capability component" (always present)

**Example:**
```rust
#[derive(Bundle)]
pub struct UnitBundle {
    pub position: SimPosition,
    pub velocity: SimVelocity,
    pub acceleration: SimAcceleration,
    pub path: Path, // NEW: Always present
    // ... other components
}

impl Default for UnitBundle {
    fn default() -> Self {
        Self {
            // ...
            path: Path::Inactive, // Start inactive
            // ...
        }
    }
}
```

### Phase 5: Testing & Validation

**Unit Tests:**
- Arena operations (register, mark, sweep, iter)
- Overflow to fallback bitset
- Edge cases (empty, full, all tombstones)

**Integration Tests:**
- Spawn 1M entities, request paths for 100K, verify only 100K in arena
- Complete all paths, verify arena empties
- Overflow test: Request paths for > hot_capacity entities

**Performance Tests:**
- Benchmark: `follow_path` with 10K active paths vs querying 1M entities
- Benchmark: Sweep cost with varying tombstone ratios
- Compare old vs new implementation throughput

**Metrics to Track:**
- Frame time for follow_path (should decrease dramatically)
- Memory usage (should be predictable)
- Path completion throughput

---

## Migration Checklist

### Code Changes
- [ ] Implement ActivePathArena (hot + fallback)
- [ ] Add Path::Inactive variant
- [ ] Remove all Path component insertions
- [ ] Remove all Path component removals
- **Archetype fragmentation**: Constant add/remove causes structural changes

### After (Optimized)
- `follow_path`: O(active_paths) via arena iteration
  - With 10M entities, 100K active paths: iterates 100K from arena
  - With 10M entities, 5M active paths: iterates 100K from hot + 4.9M from bitset
- `sweep_inactive_paths`: O(active_paths) marking + O(hot_len) sweep
- No component insertion/removal: mutation only
- **Zero archetype changes**: All units always have Path component

### Expected Gains
- **10M entities, 100K active (1%)**: ~100x reduction in iteration overhead
  - Current: O(10M) to find 100K active
  - New: O(100K) direct iteration
- **10M entities, 1M active (10%)**: ~10x reduction if using hot arena optimally
- **Reduced GC pressure**: No allocations from component add/remove
- **Better cache locality**: Arena is packed, predictable memory access
- **Eliminated frame spikes**: No archetype fragmentation from structural changes

### Architecture Comparison

| Approach | Memory | CPU Iteration | Structural Changes | Best For |
|----------|--------|--------------|-------------------|----------|
| **Marker Components** (`With<ActivePath>`) | Lowest | Fast (archetype filter) | **High (killer at 10M)** | Small-scale (<100K entities) |
| **Bitset Arena** | 1.25MB | Very Fast (SIMD skip) | None | Memory-constrained, sparse |
| **Box<[Entity]> Arena** | ~80MB | **Fastest** (linear) | None | **10M RTS (WINNER)** |
| **Full Query Scan** | N/A | Terrible O(N) | None | Never (reference only) |

### Edge Cases
- **Pathological case**: If > hot_capacity entities overflow to bitset, iteration becomes O(MAX_ENTITIES/64)
  - Still better than component query if components are scattered
  - Bitset iteration is cache-friendly (sequential u64 reads)
  - Uses CPU instructions (POPCNT, TZCNT) to skip 64-bit chunks of zeros in single cycle
### Documentation
- [ ] Update pathfinding README
- [ ] Document arena API
- [ ] Add performance notes to ARCHITECTURE.md
- [ ] Update component lifecycle docs

---

## Performance Expectations

### Before (Current)
- `follow_path`: O(entities_with_Path_component)
  - With 10M entities, 100K active paths: iterates 100K
  - With 10M entities, 5M active paths: iterates 5M
- `cleanup_completed_paths`: O(entities_with_Path_component)
- Component insertion/removal: archetype changes, allocations

### After (Optimized)
- `follow_path`: O(active_paths) via arena iteration
  - With 10M entities, 100K active paths: iterates 100K from arena
  - With 10M entities, 5M active paths: iterates 100K from hot + 4.9M from bitset
- `sweep_inactive_paths`: O(active_paths) marking + O(hot_len) sweep
- No component insertion/removal: mutation only

### Expected Gains
- **10M entities, 100K active (1%)**: ~100x reduction in iteration overhead (1M -> 100K)
- **10M entities, 1M active (10%)**: ~10x reduction if using hot arena optimally
- **Reduced GC pressure**: No allocations from component add/remove
- **Better cache locality**: Arena is packed, predictable memory access

### Edge Cases
- **Pathological case**: If > hot_capacity entities overflow to bitset, iteration becomes O(MAX_ENTITIES/64)
  - Still better than component query if components are scattered
  - Bitset iteration is cache-friendly (sequential u64 reads)
  
---

## Future Optimizations (Post-MVP)

### 1. Two-Way Migration
- Migrate entities from fallback back to hot arena when space available
- Maintains hot arena utilization during bursty workloads

### 2. Arena-Based Batch Query
- Instead of `query.get_mut(entity)` per entity, batch query:
  ```rust
  let batch = query.get_many_mut(active_paths.iter().collect());
  ```
- Potential for better cache performance

### 3. SIMD Sweep
- Use SIMD for tombstone scanning and compaction
- Especially useful for large hot arenas

### 4. Generalize Arena (Already Built-In!)
- ActivePathArena is already generic via `ArenaConfig`
- **Reuse pattern** for other "sparse active set" problems:
  - `ActiveAbilityArena`: hot_capacity: Some(10K), enable_fallback: true (few units casting)
  - `ActiveAnimationArena`: hot_capacity: None, enable_fallback: true (bitset-only, memory-light)
  - `SpatialHashDirtyFlags`: hot_capacity: Some(100K), enable_fallback: false (bounded set)
  - `ActivePathArena`: hot_capacity: Some(1M), enable_fallback: true (our case)
- **Same code, different configs** - this is why we didn't just use a single Box<[Entity]>

### 5. Sorted Hot Arena
- Keep entities sorted by ID
- Better cache locality if ECS components are stored in entity order
- Enables binary search for contains()

### 6. Multi-Tier Arena
- Add a "warm" tier between hot and fallback
- 3 tiers: Hot (100K), Warm (1M), Cold (bitset)

### 7. Hierarchical Bitset
- For ultra-large sparse sets, use a two-level bitset
- **Summary Bitset**: 1 bit represents 64 entities in detail bitset
- If summary bit is 0, skip entire 64-entity chunk
- Used in engines like Frostbite and Factorio for massive sparse lookups
- Reduces 10M-bit scan to ~156K summary bits + selective detail scans

---

## Lessons from Architecture Discussion

### The "Sparse vs Dense" Problem

**Sparse Operations** (Path Requests):
- Few entities affected per frame (~1K-10K out of 10M)
- Solution: Loop over requests, use `query.get_mut(entity)` per entity
- Cost: O(requests) with random access - acceptable for small request counts
- No need to iterate all entities

**Dense Operations** (Path Following):
- Many entities affected per frame (~100K-1M out of 10M)
- Solution: **Maintain explicit active list**, iterate only active entities
- Cost: O(active) with linear access - critical for performance
- Avoids O(total_entities) scan

### Why Manual Arena Beats Marker Components

**The**Solution**: Use `RemovedComponents<Path>` system
     - Automatically triggers when entity despawns or Path removed
     - Batch mark entities, then sweep once
     - Integrates cleanly with Bevy's lifecyclet(ActivePath) -> 5,000 archetype moves
Frame 2: 3,000 units arrive -> 3,000 remove(ActivePath) -> 3,000 archetype moves
Frame 3: 10,000 units click -> 10,000 insert(ActivePath) -> 10,000 archetype moves
...
Result: Constant memory reorganization, cache thrashing, frame spikes
```

**The Manual Arena Solution:**
```
Startup: All units have Path::Inactive component (fixed archetypes)
Frame 1: 5,000 units click -> 5,000 mutations + arena.register() -> 0 archetype moves
Frame 2: 3,000 arrive -> 3,000 mutations + arena.mark() -> 0 archetype moves
Frame 3: sweep_inactive() -> compact arena once -> 0 archetype moves
...
Result: Zero structural changes, predictable performance, no spikes
```

### The Two-Phase System Pattern

**Phase 1: Sparse Input (Path Requests)**
```rust
// Few requests, scattered entities - use get_mut
for (entity, request) in requests.iter() {
    if let Ok(mut path) = path_query.get_mut(entity) {
        *path = Path::Active(compute_path(request));
        active_arena.register_active(entity);
    }
}
```

**Phase 2: Dense Processing (Path Following)**
```rust
// Many active paths, packed in arena - linear iteration
for entity in active_arena.iter() {
    let Ok((mut pos, mut vel, path)) = query.get_mut(entity) else { continue };
    // Navigation logic...
    if arrived {
        active_arena.mark_inactive(entity);
    }
}
active_arena.sweep_inactive(); // Batch cleanup
```

### Batch Removal: Mark-and-Sweep vs Swap-Remove

**Naive Approach (Swap-Remove Loop):**
```rust
// 1,000 removals = 1,000 operations, each potentially touching last element
for entity_to_remove in &dead_entities {
    arena.swap_remove(entity_to_remove); // O(n) search + O(1) swap
}
// Total: O(n * m) where n = arena size, m = removals
```

**Optimized Approach (Mark-and-Sweep):**
```rust
// Pass 1: Mark dead entities (can use HashSet for O(1) lookup)
let dead_set: HashSet<Entity> = dead_entities.iter().copied().collect();
for i in 0..arena.len {
    if dead_set.contains(&arena.entities[i]) {
        arena.tombstones.set(i, true);
    }
}

// Pass 2: Single linear compaction
arena.sweep_inactive(); // Moves live entities forward once
// Total: O(n + m) where n = arena size, m = removals
```

**Performance Difference:**
- 1,000 removals from 100,000 arena:
  - Swap-remove loop: ~100M operations (worst case)
  - Mark-and-sweep: ~101K operations (linear pass)
  - **~1000x faster for batch operations**

- Make ActivePathArena generic: `EntitySetArena<T>`
- Reuse for other "sparse active set" problems:
  - Units with active abilities
  - Entities with active animations
  - Dirty flags for spatial hash updates

### 5. Sorted Hot Arena
- Keep entities sorted by ID
- Better cache locality if ECS components are stored in entity order
- Enables binary search for contains()

### 6. Multi-Tier Arena
- Add a "warm" tier between hot and fallback
- 3 tiers: Hot (100K), Warm (1M), Cold (bitset)

---

## Open Questions

1. **Should PathRequest also be handled as mutation?**
   - Current: Component added/removed
   - Could be: PathRequest::Active(goal) / PathRequest::None
   - Would enable PathRequest arena as well

2. **How to handle entity despawn?**
   - Currently: ECS removes all components automatically
   - With arena: Need to detect despawn and remove from arena
   - **Solution**: Use `RemovedComponents<Path>` system
     - Automatically triggers when entity despawns or Path removed
     - Batch mark entities, then sweep once
     - Integrates cleanly with Bevy's lifecycle

3. **Should we track "dirty" entities for incremental updates?**
   - E.g., only recompute navigation for entities that changed regions
   - Could use another arena for "needs_recompute" flags

4. **Telemetry for arena usage?**
   - Track hot_count, fallback_count, overflow_events
   - Useful for tuning capacity

---

## Success Criteria

1. **Correctness**: All pathfinding behavior unchanged (deterministic)
2. **Performance**: `follow_path` scales O(active_paths), not O(total_entities)
3. **Memory**: Predictable memory usage, no leaks
4. **Maintainability**: Clean API, well-documented, testable
5. **Scalability**: Works efficiently from 100 to 10M entities

---

## Conclusion

This two-layer arena design provides:
- **Optimal common case**: Fast iteration via packed hot array (~80MB for 1M active entities)
- **Graceful degradation**: Fallback bitset handles overflow (only ~1.25MB if using bitset-only)
- **Flexibility**: Configurable for different workload profiles via `ArenaConfig`
- **Reusability**: Generic design applicable to other systems without code duplication
- **Client isolation**: Systems using arena never know/care about internal tier structure

### Why Two-Tier Instead of Single Box<[Entity]>?

While our performance analysis shows `Box<[Entity]>` arena is fastest for pathfinding:

1. **We need multiple arenas**: Paths, abilities, animations, dirty flags, etc.
2. **Different memory budgets**: Not all systems warrant 80MB; some need only 1.25MB bitset
3. **Configuration over duplication**: One codebase, many use cases via `ArenaConfig`
4. **API stability**: Can switch tiers (performance ↔ memory) without touching client systems
5. **Future-proofing**: Easy to add third tier, change strategies, or optimize per-arena

**Example Multi-Arena Setup:**
```rust
// Pathfinding: High performance, 1M hot + bitset fallback = ~88MB
ActivePathArena::new(MAX_ENTITIES, ArenaConfig { 
    hot_capacity: Some(1_000_000), enable_fallback: true, .. 
});

// Abilities: Medium performance, 10K hot + bitset = ~8MB
ActiveAbilityArena::new(MAX_ENTITIES, ArenaConfig { 
    hot_capacity: Some(10_000), enable_fallback: true, .. 
});

// UI Hover States: Memory-light, bitset-only = ~1.25MB
ActiveHoverArena::new(MAX_ENTITIES, ArenaConfig { 
    hot_capacity: None, enable_fallback: true, .. 
});
```

Total memory: ~97MB for all three arenas instead of 240MB if all used hot-only.

The migration is straightforward:
1. Implement generic arena (isolated, testable)
2. Switch Path to mutation-based (local change)
3. Integrate arena with systems (incremental)
4. Validate performance (measure!)
5. **Reuse arena for other systems as needed**

Total implementation time: ~1-2 days for core system + integration + testing.
