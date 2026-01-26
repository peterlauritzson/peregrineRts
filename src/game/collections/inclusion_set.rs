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
    /// Item already existed (keep existing component state)
    AlreadyPresent,
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
    count: usize,              // Current size (may have None holes)
    live_count: usize,         // Number of non-None items
    capacity: usize,
    sorted: bool,
}

impl<T> HotStorage<T>
where
    T: Copy + Into<usize> + From<u32> + PartialOrd,
{
    fn new(capacity: usize, sorted: bool) -> Self {
        // Pre-allocate fixed-size array
        let items = (0..capacity).map(|_| None).collect::<Vec<_>>().into_boxed_slice();
        
        Self {
            items,
            count: 0,
            live_count: 0,
            capacity,
            sorted,
        }
    }

    fn insert(&mut self, item: T) -> Option<usize> {
        // Find insertion point
        if self.sorted {
            // Binary search for sorted insertion
            let mut insert_pos = self.count;
            for idx in 0..self.count {
                if let Some(stored) = self.items[idx] {
                    if stored.partial_cmp(&item).unwrap() == std::cmp::Ordering::Greater {
                        insert_pos = idx;
                        break;
                    }
                }
            }
            
            // Shift items to make room
            if self.count >= self.capacity {
                return None; // Full
            }
            
            for idx in (insert_pos..self.count).rev() {
                self.items[idx + 1] = self.items[idx];
            }
            
            self.items[insert_pos] = Some(item);
            self.count += 1;
            self.live_count += 1;
            Some(insert_pos)
        } else {
            // Unsorted - append at end
            if self.count >= self.capacity {
                return None; // Full
            }
            
            let index = self.count;
            self.items[index] = Some(item);
            self.count += 1;
            self.live_count += 1;
            Some(index)
        }
    }

    /// Mark item at specific index for removal (sets to None immediately).
    /// Returns true if verification passed and item was removed.
    fn mark_removed_at(&mut self, item: T, index: usize) -> bool {
        // Verify index is valid and matches item
        if index >= self.count {
            warn!("mark_removed_at: index {} out of bounds (count={})", index, self.count);
            return false;
        }
        
        if let Some(stored) = self.items[index] {
            let stored_key: usize = stored.into();
            let key: usize = item.into();
            
            if stored_key != key {
                warn!("mark_removed_at: index {} has item {} but expected {}", index, stored_key, key);
                return false;
            }
            
            // Set to None immediately (bitset managed by caller)
            self.items[index] = None;
            self.live_count -= 1;
            true
        } else {
            // Already None
            false
        }
    }

    /// Remove item immediately using swap-remove (O(1)).
    /// Swaps the removed item with the last item in the array, then decrements count.
    /// Calls update_fn(old_index, new_index) if the last item was moved.
    fn remove_immediate_at(&mut self, item: T, index: usize, mut update_fn: impl FnMut(usize, usize)) -> bool {
        // Verify index is valid and matches item
        if index >= self.count {
            warn!("remove_immediate_at: index {} out of bounds (count={})", index, self.count);
            return false;
        }
        
        if let Some(stored) = self.items[index] {
            let stored_key: usize = stored.into();
            let key: usize = item.into();
            
            if stored_key != key {
                warn!("remove_immediate_at: index {} has item {} but expected {}", index, stored_key, key);
                return false;
            }
            
            // Swap-remove: replace with last item
            let last_idx = self.count - 1;
            if index != last_idx {
                // Swap with last item
                self.items[index] = self.items[last_idx];
                // Notify that the last item moved to this index (only if non-None)
                if self.items[index].is_some() {
                    update_fn(last_idx, index);
                }
            }
            
            // Clear the last slot
            self.items[last_idx] = None;
            self.count -= 1;
            self.live_count -= 1;
            
            true
        } else {
            // Already None
            false
        }
    }

    /// Sweep None holes and call update_fn for each index that moved.
    /// Compacts the array so all live items are at the front.
    fn sweep(&mut self, mut update_fn: impl FnMut(usize, usize)) {
        if self.live_count == self.count {
            return; // No holes
        }

        // Compact items array: move all Some values to front
        let mut write_idx = 0;
        for read_idx in 0..self.count {
            if let Some(item) = self.items[read_idx] {
                if read_idx != write_idx {
                    self.items[write_idx] = Some(item);
                    self.items[read_idx] = None;
                    update_fn(read_idx, write_idx);
                }
                write_idx += 1;
            }
        }

        self.count = self.live_count;
    }

    fn count(&self) -> usize {
        self.live_count
    }

    fn iter(&self) -> impl Iterator<Item = T> + '_ {
        self.items[..self.count]
            .iter()
            .filter_map(|slot| *slot)
    }



    fn get_at(&self, index: usize) -> Option<T> {
        if index < self.count {
            self.items[index]
        } else {
            None
        }
    }
}

/// Storage mode: either use hot Vec or rely on bitset only.
enum StorageMode<T: Copy> {
    Hot(HotStorage<T>),
    BitsetOnly,  // When hot Vec disabled or too full, use bitset directly
}

/// Set of included entities with O(included) iteration.
///
/// **Type Requirements**: T must convert to dense, sequential indices (0, 1, 2, 3, ...)  
/// See module docs for detailed type constraints.
///
/// # Architecture
/// - **bitset**: Always maintained, tracks which entity IDs are present (for contains() and fallback storage)
/// - **mode**: Either Hot(Vec) for fast iteration OR BitsetOnly when Vec is disabled/full
pub struct InclusionSet<T>
where
    T: Copy + Into<usize> + From<u32> + PartialOrd,
{
    /// Always maintained, serves dual purpose: presence checks + fallback storage
    bitset: FixedBitSet,
    /// Track count manually (faster than bitset.count_ones())
    bitset_count: usize,
    /// Track highest index for efficient iteration
    highest_set: usize,
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
            StorageMode::Hot(HotStorage::new(hot_capacity, config.sorted))
        } else {
            StorageMode::BitsetOnly
        };

        Self {
            bitset: FixedBitSet::with_capacity(config.max_capacity),
            bitset_count: 0,
            highest_set: 0,
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
        let key = item.into();
        
        // Reject indices exceeding max_capacity
        if key >= self.config.max_capacity {
            warn!("InclusionSet: Index {} exceeds max_capacity {} - REJECTING", key, self.config.max_capacity);
            return IncludeResult::AlreadyPresent; // Treat as no-op
        }
        
        // Check bitset first (O(1) duplicate check)
        if key < self.bitset.len() && self.bitset[key] {
            return IncludeResult::AlreadyPresent;
        }
        
        // Grow bitset if needed (once to max_capacity)
        if key >= self.bitset.len() {
            self.bitset.grow(self.config.max_capacity);
        }
        
        // Set in bitset (always!)
        self.bitset.set(key, true);
        self.bitset_count += 1;
        if key > self.highest_set {
            self.highest_set = key;
        }
        
        // Try to add to hot storage if enabled
        match &mut self.mode {
            StorageMode::Hot(hot) => {
                match hot.insert(item) {
                    Some(index) => IncludeResult::Hot(InclusionIndex(index)),
                    None => {
                        // Hot storage full - migrate to bitset-only
                        self.migrate_to_bitset();
                        IncludeResult::Bitset
                    }
                }
            }
            StorageMode::BitsetOnly => IncludeResult::Bitset,
        }
    }

    /// Mark item for exclusion (lazy removal).
    /// 
    /// For hot storage, requires the InclusionIndex component for verification.
    /// For bitset storage, index is ignored.
    pub fn exclude(&mut self, item: T, index: Option<InclusionIndex>) -> bool {
        let key = item.into();
        
        // Clear from bitset immediately (always!)
        if key < self.bitset.len() && self.bitset[key] {
            self.bitset.set(key, false);
            self.bitset_count -= 1;
        }
        
        // Remove from hot storage if applicable
        match &mut self.mode {
            StorageMode::Hot(hot) => {
                if let Some(idx) = index {
                    hot.mark_removed_at(item, idx.0)
                } else {
                    warn!("exclude: Hot storage requires InclusionIndex component");
                    false
                }
            }
            StorageMode::BitsetOnly => true,
        }
    }

    /// Remove item immediately and compact the array (expensive: O(n) per call).
    /// Use for single/rare removals. For batch removals, prefer exclude() + sweep().
    /// 
    /// For hot storage: Removes and shifts all subsequent items, calling update_fn for each move.
    /// For bitset storage: Same as exclude() (no compaction needed).
    pub fn remove_immediate(&mut self, item: T, index: Option<InclusionIndex>, update_fn: impl FnMut(usize, usize)) -> bool {
        let key = item.into();
        
        // Clear from bitset immediately (always!)
        if key < self.bitset.len() && self.bitset[key] {
            self.bitset.set(key, false);
            self.bitset_count -= 1;
        }
        
        // Remove from hot storage if applicable
        match &mut self.mode {
            StorageMode::Hot(hot) => {
                if let Some(idx) = index {
                    hot.remove_immediate_at(item, idx.0, update_fn)
                } else {
                    warn!("remove_immediate: Hot storage requires InclusionIndex component");
                    false
                }
            }
            StorageMode::BitsetOnly => true,
        }
    }

    /// Remove all excluded items (sweep None holes in hot mode).
    /// Calls update_fn(old_index, new_index) for each item that moved during compaction.
    /// Also checks if we should migrate back to hot mode.
    pub fn sweep(&mut self, update_fn: impl FnMut(usize, usize)) {
        match &mut self.mode {
            StorageMode::Hot(hot) => {
                hot.sweep(update_fn);
            }
            StorageMode::BitsetOnly => {
                // Check if we should migrate back to hot mode
                if let Some(hot_capacity) = self.config.hot_capacity {
                    let threshold = hot_capacity - self.config.hysteresis_buffer.unwrap_or(hot_capacity / 10);
                    if self.bitset_count < threshold {
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
            StorageMode::BitsetOnly => {
                let range = if self.bitset_count == 0 { 0 } else { self.highest_set + 1 };
                Box::new(self.bitset.ones().take_while(move |&idx| idx < range).map(|idx| T::from(idx as u32)))
            }
        }
    }

    /// Get count of included items.
    pub fn count(&self) -> usize {
        match &self.mode {
            StorageMode::Hot(hot) => hot.count(),
            StorageMode::BitsetOnly => self.bitset_count,
        }
    }

    /// Check if item is included (O(1) via bitset, regardless of mode).
    pub fn contains(&self, item: T) -> bool {
        let key = item.into();
        key < self.bitset.len() && self.bitset[key]
    }

    /// Clear all items.
    pub fn clear(&mut self) {
        self.bitset.clear();
        self.bitset_count = 0;
        self.highest_set = 0;
        
        match &mut self.mode {
            StorageMode::Hot(hot) => {
                for i in 0..hot.count {
                    hot.items[i] = None;
                }
                hot.count = 0;
                hot.live_count = 0;
            }
            StorageMode::BitsetOnly => {}
        }
    }

    /// Get statistics about current mode and usage.
    pub fn stats(&self) -> SetStats {
        match &self.mode {
            StorageMode::Hot(hot) => SetStats {
                mode: "Hot",
                count: hot.count(),
                capacity: hot.capacity,
                tombstones: hot.count - hot.live_count,
            },
            StorageMode::BitsetOnly => SetStats {
                mode: "Bitset",
                count: self.bitset_count,
                capacity: self.config.max_capacity,
                tombstones: 0,
            },
        }
    }

    /// Migrate from hot to bitset-only mode (bitset already in sync, just drop hot Vec).
    fn migrate_to_bitset(&mut self) {
        if let StorageMode::Hot(hot) = &self.mode {
            info!("InclusionSet: Migrating to bitset-only mode ({} items)", hot.count());
            // Bitset already contains all items - just drop the hot Vec
            self.mode = StorageMode::BitsetOnly;
        }
    }

    /// Migrate from bitset-only to hot mode (rebuild hot Vec from bitset).
    fn migrate_to_hot(&mut self) {
        if let StorageMode::BitsetOnly = &self.mode {
            if let Some(hot_capacity) = self.config.hot_capacity {
                info!("InclusionSet: Migrating to hot mode ({} items)", self.bitset_count);

                let mut hot = HotStorage::new(hot_capacity, self.config.sorted);

                // Rebuild hot storage from bitset
                let range = if self.bitset_count == 0 { 0 } else { self.highest_set + 1 };
                for idx in self.bitset.ones().take_while(|&i| i < range) {
                    let item = T::from(idx as u32);
                    if hot.insert(item).is_none() {
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
