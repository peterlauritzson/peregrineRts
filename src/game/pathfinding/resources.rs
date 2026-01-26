/// Pathfinding resources for active path tracking.

use bevy::prelude::*;
use crate::game::collections::{InclusionSet, SetConfig, InclusionIndex};

/// Wrapper around Entity for use with InclusionSet.
/// Stores the full entity bits (index + generation) as a u64 internally,
/// but InclusionSet uses the lower 32 bits (index) for bitset indexing.
/// This is safe because:
/// 1. We validate entities via query.get() before using them
/// 2. Invalid generations are caught naturally by Bevy's query system
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct EntityIndex(u64);  // Store full entity bits

impl From<Entity> for EntityIndex {
    fn from(entity: Entity) -> Self {
        EntityIndex(entity.to_bits())  // Store full bits (index + generation)
    }
}

impl From<EntityIndex> for Entity {
    fn from(idx: EntityIndex) -> Self {
        Entity::from_bits(idx.0)  // Restore full entity
    }
}

impl From<u32> for EntityIndex {
    fn from(idx: u32) -> Self {
        EntityIndex(idx as u64)  // For InclusionSet reconstruction
    }
}

impl From<EntityIndex> for usize {
    fn from(idx: EntityIndex) -> Self {
        // Extract just the index portion (Bevy's Entity::index())
        // Don't use bit masking - reconstruct Entity and call index()
        Entity::from_bits(idx.0).index() as usize
    }
}

/// Tracks entities with active paths for O(active_paths) iteration.
/// 
/// This is a critical performance optimization - instead of iterating over ALL entities
/// with Path components (which could be millions), we only iterate over entities that
/// currently have active paths (typically 1-10% of total entities).
/// 
/// Uses InclusionSet to dynamically switch between hot Vec storage and bitset storage
/// depending on how many active paths exist.
#[derive(Resource)]
pub struct ActivePathSet {
    inner: InclusionSet<EntityIndex>,
}

impl ActivePathSet {
    /// Include an entity in the active path set
    /// Returns IncludeResult indicating if InclusionIndex component should be added
    pub fn include(&mut self, entity: Entity) -> crate::game::collections::IncludeResult {
        self.inner.include(EntityIndex::from(entity))
    }
    
    /// Exclude an entity from the active path set
    pub fn exclude(&mut self, entity: Entity, index: Option<InclusionIndex>) {
        self.inner.exclude(EntityIndex::from(entity), index);
    }
    
    /// Iterate over all entities in the active path set
    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.inner.iter().map(Entity::from)
    }
    
    /// Sweep tombstoned entities and compact storage
    pub fn sweep(&mut self, update_fn: impl FnMut(usize, usize)) {
        self.inner.sweep(update_fn);
    }
    
    /// Get count of active paths
    pub fn count(&self) -> usize {
        self.inner.count()
    }
    
    /// Get statistics about set usage
    pub fn stats(&self) -> crate::game::collections::SetStats {
        self.inner.stats()
    }
}

impl Default for ActivePathSet {
    fn default() -> Self {
        Self {
            inner: InclusionSet::new(SetConfig {
                max_capacity: 10_000_000,      // 10M total entities
                hot_capacity: Some(1_000_000),  // 1M hot capacity (expect 1-10% active)
                hysteresis_buffer: Some(100_000), // 10% buffer to prevent mode thrashing
                sorted: false,                   // Fast append, no sorting needed
            })
        }
    }
}
