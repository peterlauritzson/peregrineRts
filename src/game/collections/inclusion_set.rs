//! Inclusion Set: Fast iteration over entity subsets with specific properties.
//!
//! # Purpose
//!
//! Track subsets of entities that have a transient property (e.g., "has active path", "is selected",
//! "is animating") without constantly adding/removing ECS components (which causes archetype churn).
//!
//! **Use when**: Property changes frequently on many entities simultaneously  
//! **Don't use when**: Property is rare, permanent, or changes affect few entities (use normal ECS components)
//!
//! # Architecture
//!
//! Dynamically switches between two storage modes with hysteresis to prevent thrashing:
//!
//! - **Hot Mode** (count < hot_capacity): Fast Vec with tombstone mark-and-sweep
//! - **Bitset Mode** (count >= hot_capacity): Space-efficient bitset iteration
//!
//! When crossing threshold, **migrates ALL items** to new mode (no dual-iteration).
//!
//! # Type Constraints (CRITICAL)
//!
//! **T must convert to DENSE, SEQUENTIAL indices (0, 1, 2, 3, ...)**
//!
//! ## ✅ Valid Types
//! - `bevy::Entity` - Perfect! Sequential IDs
//! - `u32`, `u16` - When used as dense entity IDs  
//! - Custom wrappers around sequential IDs
//!
//! ## ❌ Invalid Types
//! - UUIDs/GUIDs - Extremely sparse
//! - Hash-based IDs - Unpredictable  
//! - Pointers - Sparse, huge values
//!
//! ## Why: Bitset uses `T.into()` as direct index
//! ```text
//! Entity(5) → bitset[5] = true ✅
//! UUID      → bitset[huge_random_value] = ❌ Out of bounds
//! ```

use bevy::prelude::*;
use fixedbitset::FixedBitSet;
use std::marker::PhantomData;

use super::components::InclusionIndex;

/// Configuration for InclusionSet behavior.
#[derive(Debug, Clone)]
pub struct SetConfig {
    /// Maximum number of entities that can be tracked (for bitset sizing).
    /// Must be large enough to fit all possible entity indices.
    pub max_capacity: usize,

    /// Capacity for hot Vec mode. If None, uses bitset-only mode.
    /// When count exceeds this, migrates to bitset mode.
    pub hot_capacity: Option<usize>,

    /// Hysteresis buffer to prevent thrashing between modes.
    /// When in bitset mode, only migrate back to hot when count < (hot_capacity - hysteresis_buffer).
    /// Default: 10% of hot_capacity.
    pub hysteresis_buffer: Option<usize>,

    /// Keep hot tier sorted for cache locality.
    pub sorted: bool,
}

impl Default for SetConfig {
    fn default() -> Self {
        Self {
            max_capacity: 10_000_000,
            hot_capacity: Some(100_000),
            hysteresis_buffer: Some(10_000), // 10% of default hot_capacity
            sorted: false,
        }
    }
}

/// Result of inclusion operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncludeResult {
    /// Item was added to hot storage at this index (insert InclusionIndex component)
    Hot(InclusionIndex),
    /// Item was added to bitset storage (no component needed)
    Bitset,
    /// Item already existed (keep existing component if any)
    AlreadyPresent(Option<InclusionIndex>),
}

/// Index update information returned by sweep
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexUpdate {
    pub old_index: usize,
    pub new_index: usize,
}

/// Hot Vec storage mode.
struct HotStorage<T: Copy> {
    items: Box<[Option<T>]>,  // Pre-allocated, fixed-size array
    count: usize,              // Number of live items (not tombstoned)
    capacity: usize,
    tombstones: FixedBitSet,
    tombstone_count: usize,    // Cached count of tombstones
    /// Bitset tracking which entity IDs are present (for O(1) contains/duplicate checks)
    presence: FixedBitSet,
    sorted: bool,
    max_capacity: usize,
}

impl<T> HotStorage<T>
where
    T: Copy + Into<usize> + From<u32> + PartialOrd,
{
    fn new(capacity: usize, max_capacity: usize, sorted: bool) -> Self {
        // Pre-allocate fixed-size array
        let items = (0..capacity).map(|_| None).collect::<Vec<_>>().into_boxed_slice();
        
        Self {
            items,
            count: 0,
            capacity,
            tombstones: FixedBitSet::with_capacity(capacity),
            tombstone_count: 0,
            presence: FixedBitSet::with_capacity(max_capacity),
            sorted,
            max_capacity,
        }
    }

    fn insert(&mut self, item: T) -> Option<usize> {
        let key: usize = item.into();

        // Check presence bitset for duplicates
        if key < self.presence.len() && self.presence[key] {
            // Already present - find its index
            for (idx, slot) in self.items.iter().enumerate() {
                if let Some(stored) = slot {
                    if !self.tombstones[idx] && (*stored).into() == key {
                        return Some(idx);
                    }
                }
            }
        }

        // Ensure presence bitset is large enough
        if key >= self.presence.len() {
            if key >= self.max_capacity {
                warn!("HotStorage: Index {} exceeds max_capacity {}", key, self.max_capacity);      
                return None;
            }
            self.presence.grow(key + 1);
        }

        // Find insertion point
        if self.sorted {
            // Binary search for sorted insertion
            let mut insert_pos = self.count;
            for idx in 0..self.count {
                if let Some(stored) = self.items[idx] {
                    if !self.tombstones[idx] {
                        if stored.partial_cmp(&item).unwrap() == std::cmp::Ordering::Greater {
                            insert_pos = idx;
                            break;
                        }
                    }
                }
            }
            
            // Shift items to make room
            if self.count >= self.capacity {
                return None; // Full
            }
            
            for idx in (insert_pos..self.count).rev() {
                self.items[idx + 1] = self.items[idx];
                if self.tombstones[idx] {
                    self.tombstones.set(idx + 1, true);
                    self.tombstones.set(idx, false);
                }
            }
            
            self.items[insert_pos] = Some(item);
            self.presence.set(key, true);
            self.count += 1;
            Some(insert_pos)
        } else {
            // Unsorted - append at end
            if self.count >= self.capacity {
                return None; // Full
            }
            
            let index = self.count;
            self.items[index] = Some(item);
            self.presence.set(key, true);
            self.count += 1;
            Some(index)
        }
    }

    /// Mark item at specific index for removal.
    /// Returns true if verification passed and item was marked.
    fn mark_removed_at(&mut self, item: T, index: usize) -> bool {
        // Verify index is valid and matches item
        if index >= self.count {
            warn!("mark_removed_at: index {} out of bounds (count={})", index, self.count);
            return false;
        }
        
        if self.tombstones[index] {
            // Already tombstoned
            return false;
        }
        
        if let Some(stored) = self.items[index] {
            let stored_key: usize = stored.into();
            let key: usize = item.into();
            
            if stored_key != key {
                warn!("mark_removed_at: index {} has item {} but expected {}", index, stored_key, key);
                return false;
            }
            
            self.tombstones.set(index, true);
            self.tombstone_count += 1;
            true
        } else {
            warn!("mark_removed_at: index {} is empty", index);
            false
        }
    }

    /// Remove item immediately and compact the array (expensive: O(n) per call).
    /// Use for single/rare removals. For batch removals, prefer mark_removed_at() + sweep().
    /// Returns index updates via callback for all shifted items.
    fn remove_immediate_at(&mut self, item: T, index: usize, mut update_fn: impl FnMut(usize, usize)) -> bool {
        // Verify index is valid and matches item
        if index >= self.count {
            warn!("remove_immediate_at: index {} out of bounds (count={})", index, self.count);
            return false;
        }
        
        if self.tombstones[index] {
            warn!("remove_immediate_at: index {} is already tombstoned", index);
            return false;
        }
        
        if let Some(stored) = self.items[index] {
            let stored_key: usize = stored.into();
            let key: usize = item.into();
            
            if stored_key != key {
                warn!("remove_immediate_at: index {} has item {} but expected {}", index, stored_key, key);
                return false;
            }
            
            // Clear presence bit
            if key < self.presence.len() {
                self.presence.set(key, false);
            }
            
            // Shift all items after this down by 1
            for i in index..(self.count - 1) {
                self.items[i] = self.items[i + 1];
                // Also shift tombstone bits
                if self.tombstones[i + 1] {
                    self.tombstones.set(i, true);
                } else {
                    self.tombstones.set(i, false);
                }
                // Notify about index change
                if !self.tombstones[i] {
                    update_fn(i + 1, i);
                }
            }
            
            // Clear the last slot
            self.items[self.count - 1] = None;
            self.tombstones.set(self.count - 1, false);
            self.count -= 1;
            
            true
        } else {
            warn!("remove_immediate_at: index {} is empty", index);
            false
        }
    }

    /// Sweep tombstoned items and call update_fn for each index that moved.
    /// Takes a callback to avoid allocating a Vec.
    fn sweep(&mut self, mut update_fn: impl FnMut(usize, usize)) {
        if self.tombstone_count == 0 {
            return;
        }

        // Clear presence bits for tombstoned items
        for idx in 0..self.count {
            if self.tombstones[idx] {
                if let Some(item) = self.items[idx] {
                    let key = item.into();
                    if key < self.presence.len() {
                        self.presence.set(key, false);
                    }
                }
                self.items[idx] = None;
            }
        }

        // Compact items array and call update_fn for moves
        let mut write_idx = 0;
        for read_idx in 0..self.count {
            if !self.tombstones[read_idx] {
                if read_idx != write_idx {
                    self.items[write_idx] = self.items[read_idx];
                    self.items[read_idx] = None;
                    update_fn(read_idx, write_idx);
                }
                write_idx += 1;
            }
        }

        self.count = write_idx;
        self.tombstones.clear();
        self.tombstone_count = 0;
    }

    fn count(&self) -> usize {
        self.count - self.tombstone_count
    }

    fn iter(&self) -> impl Iterator<Item = T> + '_ {
        self.items[..self.count]
            .iter()
            .enumerate()
            .filter(move |(idx, _)| !self.tombstones[*idx])
            .filter_map(|(_, slot)| *slot)
    }

    fn contains(&self, item: T) -> bool {
        let key: usize = item.into();
        // O(1) check via presence bitset - much faster than HashMap or linear search!
        key < self.presence.len() && self.presence[key]
    }

    fn get_at(&self, index: usize) -> Option<T> {
        if index < self.count && !self.tombstones[index] {
            self.items[index]
        } else {
            None
        }
    }
}

/// Bitset storage mode.
struct BitsetStorage {
    bits: FixedBitSet,
    max_capacity: usize,
    count: usize,
}

impl BitsetStorage {
    fn new(max_capacity: usize) -> Self {
        Self {
            bits: FixedBitSet::with_capacity(max_capacity),
            max_capacity,
            count: 0,
        }
    }

    fn insert(&mut self, key: usize) -> bool {
        if key >= self.max_capacity {
            warn!(
                "InclusionSet: Index {} exceeds max_capacity {}",
                key, self.max_capacity
            );
            return false;
        }

        if key >= self.bits.len() {
            self.bits.grow(key + 1);
        }

        if !self.bits[key] {
            self.bits.set(key, true);
            self.count += 1;
        }
        true
    }

    fn remove(&mut self, key: usize) {
        if key < self.bits.len() && self.bits[key] {
            self.bits.set(key, false);
            self.count -= 1;
        }
    }

    fn contains(&self, key: usize) -> bool {
        key < self.bits.len() && self.bits[key]
    }

    fn count(&self) -> usize {
        self.count
    }

    fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.bits.ones()
    }

    fn clear(&mut self) {
        self.bits.clear();
        self.count = 0;
    }
}

/// Storage mode state machine.
enum StorageMode<T: Copy> {
    Hot(HotStorage<T>),
    Bitset(BitsetStorage),
}

/// Set of included entities with O(included) iteration.
///
/// **Type Requirements**: T must convert to dense, sequential indices (0, 1, 2, 3, ...)  
/// See module docs for detailed type constraints.
pub struct InclusionSet<T>
where
    T: Copy + Into<usize> + From<u32> + PartialOrd,
{
    mode: StorageMode<T>,
    config: SetConfig,
    _phantom: PhantomData<T>,
}

impl<T> InclusionSet<T>
where
    T: Copy + Into<usize> + From<u32> + PartialOrd,
{
    /// Create a new inclusion set with the given configuration.
    ///
    /// # Panics
    /// Panics if config is invalid (e.g., hysteresis_buffer >= hot_capacity).
    pub fn new(config: SetConfig) -> Self {
        // Validate config
        if let (Some(hot_cap), Some(hyst)) = (config.hot_capacity, config.hysteresis_buffer) {
            if hyst >= hot_cap {
                panic!(
                    "hysteresis_buffer ({}) must be < hot_capacity ({})",
                    hyst, hot_cap
                );
            }
        }

        let mode = if let Some(hot_capacity) = config.hot_capacity {
            StorageMode::Hot(HotStorage::new(
                hot_capacity,
                config.max_capacity,
                config.sorted,
            ))
        } else {
            // Bitset-only mode
            StorageMode::Bitset(BitsetStorage::new(config.max_capacity))
        };

        Self {
            mode,
            config,
            _phantom: PhantomData,
        }
    }

    /// Include an item in the set.
    /// 
    /// Returns:
    /// - `IncludeResult::Hot(index)` - Item added to hot storage, insert InclusionIndex component
    /// - `IncludeResult::Bitset` - Item added to bitset storage, no component needed
    /// - `IncludeResult::AlreadyPresent` - Item was already in the set
    pub fn include(&mut self, item: T) -> IncludeResult {
        match &mut self.mode {
            StorageMode::Hot(hot) => {
                if let Some(index) = hot.insert(item) {
                    IncludeResult::Hot(InclusionIndex(index))
                } else {
                    // Hot storage full - migrate to bitset
                    self.migrate_to_bitset();
                    // Try again in bitset mode
                    if let StorageMode::Bitset(bitset) = &mut self.mode {
                        bitset.insert(item.into());
                    }
                    IncludeResult::Bitset
                }
            }
            StorageMode::Bitset(bitset) => {
                bitset.insert(item.into());
                IncludeResult::Bitset
            }
        }
    }

    /// Mark item for exclusion (lazy removal).
    /// 
    /// For hot storage, requires the InclusionIndex component for verification.
    /// For bitset storage, index is ignored.
    pub fn exclude(&mut self, item: T, index: Option<InclusionIndex>) -> bool {
        match &mut self.mode {
            StorageMode::Hot(hot) => {
                if let Some(idx) = index {
                    hot.mark_removed_at(item, idx.0)
                } else {
                    warn!("exclude: Hot storage requires InclusionIndex component");
                    false
                }
            }
            StorageMode::Bitset(bitset) => {
                bitset.remove(item.into());
                true
            }
        }
    }

    /// Remove item immediately and compact the array (expensive: O(n) per call).
    /// Use for single/rare removals. For batch removals, prefer exclude() + sweep().
    /// 
    /// For hot storage: Removes and shifts all subsequent items, calling update_fn for each move.
    /// For bitset storage: Same as exclude() (no compaction needed).
    pub fn remove_immediate(&mut self, item: T, index: Option<InclusionIndex>, update_fn: impl FnMut(usize, usize)) -> bool {
        match &mut self.mode {
            StorageMode::Hot(hot) => {
                if let Some(idx) = index {
                    hot.remove_immediate_at(item, idx.0, update_fn)
                } else {
                    warn!("remove_immediate: Hot storage requires InclusionIndex component");
                    false
                }
            }
            StorageMode::Bitset(bitset) => {
                bitset.remove(item.into());
                true
            }
        }
    }

    /// Remove all excluded items (sweep tombstones in hot mode).
    /// Calls update_fn(old_index, new_index) for each item that moved during compaction.
    /// Also checks if we should migrate back to hot mode.
    pub fn sweep(&mut self, update_fn: impl FnMut(usize, usize)) {
        match &mut self.mode {
            StorageMode::Hot(hot) => {
                hot.sweep(update_fn);
            }
            StorageMode::Bitset(bitset) => {
                // Check if we should migrate back to hot mode
                if let Some(hot_capacity) = self.config.hot_capacity {
                    let threshold = hot_capacity
                        - self
                            .config
                            .hysteresis_buffer
                            .unwrap_or(hot_capacity / 10);
                    if bitset.count() < threshold {
                        self.migrate_to_hot();
                    }
                }
                // Bitset mode has no indices to update
            }
        }
    }

    /// Iterate over all included items.
    pub fn iter(&self) -> Box<dyn Iterator<Item = T> + '_> {
        match &self.mode {
            StorageMode::Hot(hot) => Box::new(hot.iter()),
            StorageMode::Bitset(bitset) => {
                Box::new(bitset.iter().map(|idx| T::from(idx as u32)))
            }
        }
    }

    /// Get count of included items.
    pub fn count(&self) -> usize {
        match &self.mode {
            StorageMode::Hot(hot) => hot.count(),
            StorageMode::Bitset(bitset) => bitset.count(),
        }
    }

    /// Check if item is included.
    pub fn contains(&self, item: T) -> bool {
        match &self.mode {
            StorageMode::Hot(hot) => hot.contains(item),
            StorageMode::Bitset(bitset) => bitset.contains(item.into()),
        }
    }

    /// Clear all items.
    pub fn clear(&mut self) {
        match &mut self.mode {
            StorageMode::Hot(hot) => {
                for i in 0..hot.count {
                    hot.items[i] = None;
                }
                hot.count = 0;
                hot.tombstones.clear();
                hot.tombstone_count = 0;
                hot.presence.clear();
            }
            StorageMode::Bitset(bitset) => bitset.clear(),
        }
    }

    /// Get statistics about current mode and usage.
    pub fn stats(&self) -> SetStats {
        match &self.mode {
            StorageMode::Hot(hot) => SetStats {
                mode: "Hot",
                count: hot.count(),
                capacity: hot.capacity,
                tombstones: hot.tombstone_count,
            },
            StorageMode::Bitset(bitset) => SetStats {
                mode: "Bitset",
                count: bitset.count(),
                capacity: self.config.max_capacity,
                tombstones: 0,
            },
        }
    }

    /// Migrate from hot to bitset mode.
    fn migrate_to_bitset(&mut self) {
        if let StorageMode::Hot(hot) = &self.mode {
            info!(
                "InclusionSet: Migrating to bitset mode ({} items)",
                hot.count()
            );
            let mut bitset = BitsetStorage::new(self.config.max_capacity);

            // Move ALL items to bitset
            for item in hot.iter() {
                bitset.insert(item.into());
            }

            self.mode = StorageMode::Bitset(bitset);
        }
    }

    /// Migrate from bitset to hot mode.
    fn migrate_to_hot(&mut self) {
        if let StorageMode::Bitset(bitset) = &self.mode {
            if let Some(hot_capacity) = self.config.hot_capacity {
                info!(
                    "InclusionSet: Migrating to hot mode ({} items)",
                    bitset.count()
                );

                let mut hot = HotStorage::new(
                    hot_capacity,
                    self.config.max_capacity,
                    self.config.sorted,
                );

                // Move ALL items to hot storage
                for idx in bitset.iter() {
                    let item = T::from(idx as u32);
                    if hot.insert(item).is_none() {
                        // Shouldn't happen since we checked count < threshold
                        warn!("InclusionSet: Failed to migrate item to hot mode");
                        return; // Abort migration
                    }
                }

                self.mode = StorageMode::Hot(hot);
            }
        }
    }
}

/// Statistics about set usage.
#[derive(Debug, Clone)]
pub struct SetStats {
    pub mode: &'static str,
    pub count: usize,
    pub capacity: usize,
    pub tombstones: usize,
}
