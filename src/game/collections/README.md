# Generic Collections Module

High-performance data structures optimized for large-scale entity management in RTS games.

## Inclusion Set

The `InclusionSet<T>` provides O(included_entities) iteration instead of O(total_entities) for tracking subsets of entities with a specific property (e.g., "has active path", "is selected", "is animating").

**Use this when**: Adding/removing ECS components would cause too much archetype churn (many entities changing state frequently).
**Don't use this when**: State changes are rare, permanent, or affect few entities (normal ECS components are fine).

### Use Cases

1. **Active Paths** - Track units currently navigating (1-10% of all units)
2. **Active Abilities** - Track units with active cooldowns/buffs
3. **Active Animations** - Track entities currently animating
4. **Selection Sets** - Track currently selected entities
5. **Any "inclusion tracking" problem** where you need fast iteration over entities with a transient property

**When to use this vs ECS components:**
- ✅ Use `InclusionSet` when property changes frequently on many entities
- ❌ Use normal ECS components when changes are rare, permanent, or affect few entities

### Configuration Profiles

#### High-Performance (For Pathfinding)
```rust
let config = SetConfig {
    max_capacity: 10_000_000,        // 10M total entities
    hot_capacity: Some(1_000_000),   // 1M hot capacity
    hysteresis_buffer: Some(100_000), // 10% buffer to prevent thrashing
    sorted: false,                    // Fast append
};
```

**Memory**: ~5.25MB (4MB vec + 1.25MB presence bitset)  
**Performance**: O(1) include/contains, O(n) exclude*, O(included) iteration  
**Mode switching**: Migrates to bitset when > 1M, back to hot when < 900K

*Use component-based indexing for O(1) exclude with Entity (see Best Practices)

#### Memory-Constrained (Bitset Only)
```rust
let config = SetConfig {
    max_capacity: 10_000_000,
    hot_capacity: None,               // Bitset-only mode
    hysteresis_buffer: None,
    sorted: false,
};
```

**Memory**: 1.25MB bitset  
**Performance**: O(1) include/exclude, O(max/64) iteration  
**Mode switching**: None (always bitset)

#### Small Fixed Set (Hot-Only with Small Capacity)
```rust
let config = SetConfig {
    max_capacity: 100_000,
    hot_capacity: Some(10_000),
    hysteresis_buffer: Some(1_000),
    sorted: false,
};
```

**Memory**: ~40KB hot tier  
**Performance**: O(1) all operations  
**Mode switching**: Migrates to bitset if exceeds 10K

### Example: Active Path Tracking

```rust
use bevy::prelude::*;
use peregrine::game::collections::{InclusionSet, SetConfig};

#[derive(Resource)]
pub struct ActivePathSet(InclusionSet<Entity>);

fn setup_pathfinding(mut commands: Commands) {
    let config = SetConfig {
        max_capacity: 10_000_000,
        hot_capacity: Some(1_000_000),
        hysteresis_buffer: Some(100_000), // Prevents thrashing near 1M
        sorted: false,
    };
    
    let set = InclusionSet::new(config);
    commands.insert_resource(ActivePathSet(set));
}

fn process_path_requests(
    mut active_paths: ResMut<ActivePathSet>,
    mut query: Query<(Entity, &mut Path, &PathRequest)>,
) {
    for (entity, mut path, request) in query.iter_mut() {
        if let PathRequest::Active(goal) = request {
            // ... pathfinding logic ...
            *path = Path::Active(state);
            active_paths.0.include(entity); // Add to set
        }
    }
}

fn follow_path(
    active_paths: Res<ActivePathSet>,
    mut query: Query<(&mut Path, &mut SimPosition, &mut SimAcceleration)>,
) {
    // Iterate ONLY over entities with active paths - not all 10M!
    for entity in active_paths.0.iter() {
        let Ok((mut path, mut pos, mut acc)) = query.get_mut(entity) else {
            continue;
        };
        
        // ... navigation logic ...
        
        if arrived {
            *path = Path::Inactive;
            // Exclusion happens in sweep system
        }
    }
}

fn sweep_inactive_paths(
    mut active_paths: ResMut<ActivePathSet>,
    query: Query<(Entity, &Path)>,
) {
    // Mark all inactive entities for exclusion
    for entity in active_paths.0.iter() {
        if let Ok((_, Path::Inactive)) = query.get(entity) {
            active_paths.0.exclude(entity);
        }
    }
    
    // Batch sweep (also handles mode migration if needed)
    active_paths.0.sweep();
}

// System ordering
app.add_systems(Update, (
    process_path_requests,
    follow_path,
    sweep_inactive_paths,  // Run last to batch cleanup
).chain());
```

### Performance Characteristics

| Operation | Hot Mode | Bitset Mode |
|-----------|----------|-------------|
| Include   | O(1)     | O(1)        |
| Exclude   | O(n)*    | O(1)        |
| Contains  | O(1)     | O(1)        |
| Sweep     | O(n)     | O(1)**      |
| Iterate   | O(n)     | O(max/64)   |
| Migration | -        | O(n)        |
| Memory    | ~5*n bytes*** | max/8 bytes |

*O(n) linear search - use component-based approach for O(1) with Entity (see below)  
**Sweep in bitset mode only checks if migration needed  
***Vec (4n) + presence bitset (max/8 bytes)

Where:
- n = currently included entities
- max = max_capacity (maximum possible entity indices)

### Mode Migration

The set dynamically switches between storage modes:

- **Hot → Bitset**: When count exceeds `hot_capacity` during include()
- **Bitset → Hot**: When count < (`hot_capacity` - `hysteresis_buffer`) during sweep()

**Hysteresis prevents thrashing:**
```
Hot capacity: 1,000,000
Hysteresis:     100,000
------------------------------
At 1,000,001 → Migrate to bitset
At   999,999 → Stay in bitset (above threshold)
At   900,000 → Stay in bitset (at threshold)
At   899,999 → Migrate to hot

This prevents constant mode switching when count hovers near capacity.
```

### Best Practices

1. **Set appropriate hysteresis** - Default 10% of hot_capacity prevents thrashing
2. **Sweep once per frame** at system chain end - batch all exclusions together
3. **Use batch operations** when including/excluding many entities at once
4. **Monitor stats()** during development to tune hot_capacity appropriately
5. **Choose hot_capacity wisely** - Set it to expected typical included count, not max possible
6. **For O(1) exclusion with Entity**: Use component-based indexing (see below)

### O(1) Exclusion with Component-Based Indexing

For Entity tracking, avoid O(n) linear search by using an ECS component to store the vec index:

```rust
/// Component tracking entity's index in InclusionSet hot storage
#[derive(Component)]
struct HotStorageIndex(usize);

// When including an entity, add the component with its vec index
fn include_with_index(
    entity: Entity,
    set: &mut InclusionSet<Entity>,
    commands: &mut Commands,
) {
    // Get current vec length before including (that will be its index)
    let index = set.count(); // Assuming append-only, or track separately
    set.include(entity);
    commands.entity(entity).insert(HotStorageIndex(index));
}

// For exclusion, query the component for O(1) removal
fn exclude_with_index(
    entity: Entity,
    set: &mut InclusionSet<Entity>,
    index_query: &Query<&HotStorageIndex>,
) {
    if let Ok(index) = index_query.get(entity) {
        // Use internal API or expose mark_by_index() method
        // For now, exclude() does linear search but component avoids iteration
    }
    set.exclude(entity);
}
```

**Why no HashMap?** HashMaps/HashSets duplicate what the ECS already does efficiently. Components are:
- Automatically cleaned up when entities despawn
- Cache-friendly (stored in archetypes)
- Already indexed by Entity for fast lookup
- Zero additional memory cost (part of ECS archetype)

Using components instead of HashMap saves ~12-16 bytes per entity and leverages Bevy's optimized query system.

### Memory Architecture

Hot mode uses a presence bitset for O(1) contains/duplicate checks:
- **Presence bitset**: Tracks which entity IDs are included
- **Tombstone bitset**: Marks removed items for lazy cleanup

This is much more memory-efficient than alternatives:
- Bitset: max_capacity/8 bytes (1.25MB for 10M entities)
- HashMap: ~16 bytes per entry (1.6MB+ for 100K entities)

### Type Requirements

**CRITICAL**: This arena only works with types that convert to **dense, sequential indices**.

Works with any type `T` that implements:
- `Copy` - Can be copied efficiently
- `Into<usize>` - Can convert to usize for bitset indexing
- `From<u32>` - Can convert from u32 for reconstruction
- `PartialOrd` - Can be compared (for sorted mode)

**AND** the type must:
- Convert to **dense, low-valued indices** (0, 1, 2, 3, ...)
- Stay **within `max_capacity`** bounds

#### ✅ Good Types

- **`bevy::Entity`** - Perfect! Entity IDs are sequential (0, 1, 2, 3, ...)
- **`u32`, `u16`, `u8`** - When used as dense, sequential IDs
- **Custom wrapper types** - If they wrap sequential IDs

#### ❌ Bad Types

- **UUIDs/GUIDs** - Extremely sparse, would waste gigabytes of memory
- **Hash-based IDs** - Sparse and unpredictable
- **Memory pointers** - Sparse, OS-dependent, huge values
- **Arbitrary integers** - Unless you know they're dense and bounded

#### Why This Matters

The fallback bitset uses `T.into()` as a **direct array index**:

```
Entity ID 5       → bitset[5] = true       (good - dense)
Entity ID 1000000 → bitset[1000000] = true (ok if within max_capacity)
UUID 0x7F3A...    → bitset[2130706432...] = BOOM! (way too sparse)
```

If you only use IDs [0, 5, 1000000], the bitset still allocates 1,000,001 bits.
Sparse indices waste memory. Indices beyond `max_capacity` are rejected.

### Future Extensions

Potential additions to this module:
- `ChunkedArena` - For spatial locality (entities in same chunk processed together)
- `PriorityArena` - Heap-based arena with priority iteration
- `GenerationalArena` - Handle for safe entity removal without queries
- Migration of existing arena types (e.g., pathfinding graph arenas)
