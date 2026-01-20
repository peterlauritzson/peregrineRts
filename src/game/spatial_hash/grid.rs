use bevy::prelude::*;
use crate::game::fixed_math::{FixedNum, FixedVec2};

/// Tracks which entities belong to a cell in the arena storage
#[derive(Debug, Clone, Copy)]
pub struct CellRange {
    pub start_index: usize,   // Index into entity_storage where this cell begins
    pub current_count: usize, // Current number of entities in this cell
    pub max_index: usize,     // Maximum index (exclusive) before overflow
}

impl CellRange {
    pub fn new() -> Self {
        Self { 
            start_index: 0, 
            current_count: 0,
            max_index: 0,
        }
    }
    
    pub fn with_capacity(start_index: usize, max_index: usize) -> Self {
        Self {
            start_index,
            current_count: 0,
            max_index,
        }
    }
    
    pub fn is_empty(&self) -> bool {
        self.current_count == 0
    }
    
    /// Get current count of entities in this cell
    pub fn count(&self) -> usize {
        self.current_count
    }
    
    /// Get remaining headroom before overflow
    pub fn headroom(&self) -> usize {
        self.max_index.saturating_sub(self.start_index + self.current_count)
    }
}

/// One grid in a staggered pair using arena-based storage
#[derive(Debug, Clone)]
#[allow(dead_code)]  // map_width/height stored for API consistency, may be used in future features
pub struct StaggeredGrid {
    /// ARENA: One big pre-allocated Vec for all entities in this grid
    entity_storage: Vec<Entity>,
    
    /// METADATA: Each cell tracks which range of entity_storage it owns
    pub cell_ranges: Vec<CellRange>,
    
    pub cols: usize,
    pub rows: usize,
    cell_size: FixedNum,
    offset: FixedVec2,  // Grid A: (0, 0), Grid B: (cell_size/2, cell_size/2)
    map_width: FixedNum,
    map_height: FixedNum,
    // Pre-computed constants to avoid repeated divisions in hot paths
    half_map_width: FixedNum,
    half_map_height: FixedNum,
    half_cell: FixedNum,  // 0.5 for cell center calculations
    
    /// Current number of entities stored in entity_storage
    entity_count: usize,
    
    /// Arena overcapacity ratio (e.g., 1.5 = 50% extra space for incremental updates)
    overcapacity_ratio: f32,
    
    /// Update strategy: true = incremental, false = full rebuild
    pub use_incremental_updates: bool,
}

impl StaggeredGrid {
    pub fn new(map_width: FixedNum, map_height: FixedNum, cell_size: FixedNum, offset: FixedVec2) -> Self {
        Self::with_capacity(map_width, map_height, cell_size, offset, 10_000_000, 1.0)
    }
    
    pub fn with_capacity(
        map_width: FixedNum, 
        map_height: FixedNum, 
        cell_size: FixedNum, 
        offset: FixedVec2,
        max_entities: usize,
        overcapacity_ratio: f32,
    ) -> Self {
        let cols = (map_width / cell_size).ceil().to_num::<usize>() + 2;  // Extra padding
        let rows = (map_height / cell_size).ceil().to_num::<usize>() + 2;
        let num_cells = cols * rows;
        
        // Calculate actual capacity with overcapacity ratio
        let actual_capacity = (max_entities as f32 * overcapacity_ratio) as usize;
        
        // For full rebuild mode (overcapacity_ratio ~= 1.0), use simple allocation
        // For incremental mode (overcapacity_ratio > 1.0), pre-distribute headroom
        let use_incremental = overcapacity_ratio > 1.1;
        
        Self {
            // Pre-allocate entity storage to actual capacity (zero allocation guarantee)
            entity_storage: Vec::with_capacity(actual_capacity),
            
            // One range per cell - will be initialized during first rebuild
            cell_ranges: vec![CellRange::new(); num_cells],
            
            cols,
            rows,
            cell_size,
            offset,
            map_width,
            map_height,
            half_map_width: map_width / FixedNum::from_num(2.0),
            half_map_height: map_height / FixedNum::from_num(2.0),
            half_cell: FixedNum::from_num(0.5),
            entity_count: 0,
            overcapacity_ratio,
            use_incremental_updates: use_incremental,
        }
    }
    
    /// Convert world position to cell coordinates
    pub fn pos_to_cell(&self, pos: FixedVec2) -> (usize, usize) {
        // Shift to [0, w] and apply grid offset
        let x = pos.x + self.half_map_width - self.offset.x;
        let y = pos.y + self.half_map_height - self.offset.y;
        
        let col = (x / self.cell_size).floor().to_num::<isize>().max(0).min((self.cols - 1) as isize) as usize;
        let row = (y / self.cell_size).floor().to_num::<isize>().max(0).min((self.rows - 1) as isize) as usize;
        
        (col, row)
    }
    
    /// Calculate the center of a cell in world coordinates
    pub fn cell_center(&self, col: usize, row: usize) -> FixedVec2 {
        let center_x = (FixedNum::from_num(col) + self.half_cell) * self.cell_size + self.offset.x;
        let center_y = (FixedNum::from_num(row) + self.half_cell) * self.cell_size + self.offset.y;
        
        FixedVec2::new(center_x - self.half_map_width, center_y - self.half_map_height)
    }
    
    /// Insert entity into cell (appends to entity_storage)
    /// Returns the index in entity_storage where entity was placed
    /// 
    /// Behavior depends on update strategy:
    /// - Full rebuild: ranges are contiguous, just extend count
    /// - Incremental: check headroom, use direct assignment to preallocated slots
    pub fn insert_entity(&mut self, col: usize, row: usize, entity: Entity) -> usize {
        let cell_idx = row * self.cols + col;
        
        // Bounds check to prevent crashes
        if cell_idx >= self.cell_ranges.len() {
            error!(
                "insert_entity: cell_idx {} out of bounds (cell_ranges.len={}), col={}, row={}, cols={}", 
                cell_idx, self.cell_ranges.len(), col, row, self.cols
            );
            return usize::MAX;
        }
        
        let range = &mut self.cell_ranges[cell_idx];
        
        if self.use_incremental_updates && range.max_index > 0 {
            // INCREMENTAL MODE (only if headroom has been allocated)
            // Check per-cell headroom
            let next_write_pos = range.start_index + range.current_count;
            if next_write_pos >= range.max_index {
                #[cfg(debug_assertions)]
                panic!(
                    "Cell overflow in incremental mode! Cell ({},{}) next_write_pos {} >= max_index {}", 
                    col, row, next_write_pos, range.max_index
                );
                
                #[cfg(not(debug_assertions))]
                {
                    // Clone cell info BEFORE logging to avoid borrow conflicts
                    let this_cell_start = range.start_index;
                    let this_cell_count = range.current_count;
                    let this_cell_max = range.max_index;
                    
                    // DETAILED DIAGNOSTICS - compute everything inline for easy cleanup
                    let total_entities_in_arena = self.entity_count;
                    let arena_storage_capacity = self.entity_storage.len();
                    let total_cells = self.cell_ranges.len();
                    let grid_cols = self.cols;
                    let grid_rows = self.rows;
                    
                    // Find top 10 cells by count
                    let mut cell_counts: Vec<(usize, usize, usize, usize, usize)> = Vec::new();
                    for (idx, cell_range) in self.cell_ranges.iter().enumerate() {
                        if cell_range.current_count > 0 {
                            let cell_col = idx % grid_cols;
                            let cell_row = idx / grid_cols;
                            cell_counts.push((cell_col, cell_row, cell_range.current_count, cell_range.start_index, cell_range.max_index));
                        }
                    }
                    cell_counts.sort_by(|a, b| b.2.cmp(&a.2));
                    
                    warn!("==================== CELL OVERFLOW DIAGNOSTICS ====================");
                    warn!("Cell overflow at ({},{})", col, row);
                    warn!("  This cell: start_index={}, current_count={}, max_index={}, next_write_pos={}", 
                        this_cell_start, this_cell_count, this_cell_max, next_write_pos);
                    warn!("Arena stats:");
                    warn!("  Total entities tracked: {}", total_entities_in_arena);
                    warn!("  Arena storage capacity: {}", arena_storage_capacity);
                    warn!("  Total cells: {}", total_cells);
                    warn!("  Grid dimensions: {}x{}", grid_cols, grid_rows);
                    warn!("Top 10 cells by entity count:");
                    for (i, (c, r, count, start, max)) in cell_counts.iter().take(10).enumerate() {
                        let headroom = max.saturating_sub(start + count);
                        warn!("  {}. Cell ({},{}) has {} entities (start={}, max={}, headroom={})", 
                            i+1, c, r, count, start, max, headroom);
                    }
                    warn!("===================================================================");
                    
                    return usize::MAX;
                }
            }
            
            // Direct assignment to preallocated slot
            self.entity_storage[next_write_pos] = entity;
            let entity_idx = range.current_count;  // Index within this cell's range
            range.current_count += 1;
            self.entity_count += 1;
            
            entity_idx  // Return index within cell (not absolute storage index)
        } else {
            // FULL REBUILD MODE: Append to global storage, ranges are contiguous
            // Also used for first rebuild in incremental mode before headroom is allocated
            
            // Check global capacity before push (only relevant in full rebuild mode)
            if self.entity_storage.len() >= self.entity_storage.capacity() {
                #[cfg(debug_assertions)]
                panic!(
                    "StaggeredGrid entity_storage overflow! {} >= capacity {}", 
                    self.entity_storage.len(), 
                    self.entity_storage.capacity()
                );
                
                #[cfg(not(debug_assertions))]
                {
                    warn!("StaggeredGrid entity_storage overflow - insertion dropped");
                    return usize::MAX;
                }
            }
            
            let storage_idx = self.entity_storage.len();
            self.entity_storage.push(entity);
            self.entity_count += 1;
            
            // Update cell range
            if range.current_count == 0 {
                // First entity in this cell
                range.start_index = storage_idx;
                range.current_count = 1;
                // DON'T set max_index - keep it 0 to force full rebuild mode
                // max_index is only set by rebuild_with_headroom() which allocates proper headroom
            } else {
                // Cell already has entities - should be immediately before this in storage
                range.current_count += 1;
                // DON'T set max_index - keep it 0 to force full rebuild mode
            }
            
            range.current_count - 1  // Return index within cell
        }
    }
    
    /// Remove entity from cell by searching for it in the cell's range
    /// This marks the entity as "removed" by replacing it with a tombstone (Entity::PLACEHOLDER)
    /// Returns Some(true) if removal succeeded
    pub fn remove_entity(&mut self, col: usize, row: usize, entity: Entity) -> Option<bool> {
        let cell_idx = row * self.cols + col;
        
        if cell_idx >= self.cell_ranges.len() {
            warn!(
                "remove_entity: cell_idx {} out of bounds (cell_ranges.len={}), col={}, row={}, cols={}", 
                cell_idx, self.cell_ranges.len(), col, row, self.cols
            );
            return None;
        }
        
        let range = &self.cell_ranges[cell_idx];
        if range.current_count == 0 {
            return None;
        }
        
        // Search for entity within the cell's range
        let end = range.start_index + range.current_count;
        if end > self.entity_storage.len() {
            warn!(
                "remove_entity: range.start_index {} + range.current_count {} exceeds storage len {}", 
                range.start_index, range.current_count, self.entity_storage.len()
            );
            return None;
        }
        
        for i in range.start_index..end {
            if self.entity_storage[i] == entity {
                // Mark as tombstone
                self.entity_storage[i] = Entity::PLACEHOLDER;
                self.entity_count -= 1;
                
                // NOTE: We DON'T decrement range.current_count here!
                // The range still spans the full area including tombstones.
                // Queries filter tombstones, and compaction will shrink the range properly.
                
                return Some(true);
            }
        }
        
        // Entity not found in this cell
        None
    }
    
    /// Remove entity from cell using swap-with-last-element trick (O(1), no fragmentation)
    /// This is the INCREMENTAL UPDATE approach - avoids tombstones entirely
    /// 
    /// Returns (success, swapped_entity_option):
    /// - (true, None): entity was last in cell, no swap needed
    /// - (true, Some(swapped)): entity removed, swapped entity now at vec_idx position
    /// - (false, None): entity not found or invalid cell
    pub fn remove_entity_swap(&mut self, col: usize, row: usize, vec_idx: usize) -> (bool, Option<Entity>) {
        let cell_idx = row * self.cols + col;
        
        if cell_idx >= self.cell_ranges.len() {
            return (false, None);
        }
        
        let range = &mut self.cell_ranges[cell_idx];
        if range.current_count == 0 || vec_idx >= range.current_count {
            return (false, None);
        }
        
        // O(1) direct access using vec_idx!
        let absolute_idx = range.start_index + vec_idx;
        let last_idx = range.start_index + range.current_count - 1;
        
        let swapped_entity = if absolute_idx != last_idx {
            // Swap with last element in this cell
            let last_entity = self.entity_storage[last_idx];
            self.entity_storage[absolute_idx] = last_entity;
            Some(last_entity)
        } else {
            // Entity was already last, no swap needed
            None
        };
        
        // Shrink range by 1 (no gap, no fragmentation!)
        range.current_count -= 1;
        self.entity_count -= 1;
        
        (true, swapped_entity)
    }
    
    /// Update entity that moved from old_cell to new_cell using incremental approach
    /// Total cost: O(1) amortized with swap-based removal
    /// 
    /// Returns Ok((new_vec_idx, swapped_entity_option)) where:
    /// - new_vec_idx: index of entity in new cell
    /// - swapped_entity_option: entity that was swapped in old cell (needs vec_idx update)
    pub fn update_entity_incremental(
        &mut self,
        entity: Entity,
        old_col: usize,
        old_row: usize,
        old_vec_idx: usize,
        new_col: usize,
        new_row: usize,
    ) -> Result<(usize, Option<Entity>), &'static str> {
        // Remove from old cell (swap-based, no fragmentation!)
        let (success, swapped_entity) = self.remove_entity_swap(old_col, old_row, old_vec_idx);
        if !success {
            return Err("Entity not found in old cell");
        }
        
        // Insert into new cell (uses headroom)
        let new_vec_idx = self.insert_entity(new_col, new_row, entity);
        if new_vec_idx == usize::MAX {
            return Err("Cell overflow - rebuild needed");
        }
        
        Ok((new_vec_idx, swapped_entity))
    }
    
    /// Check if rebuild is needed (any cell overflowed or global capacity critical)
    pub fn should_rebuild(&self) -> bool {
        // Check global fill ratio
        let global_usage = self.entity_count as f32 / self.entity_storage.capacity() as f32;
        if global_usage > 0.85 {
            return true;
        }
        
        // Check if any cell is at max capacity
        for range in &self.cell_ranges {
            let next_write_pos = range.start_index + range.current_count;
            if next_write_pos >= range.max_index && range.max_index > 0 {
                return true;
            }
        }
        
        false
    }
    
    /// Rebuild with headroom re-distribution
    /// 
    /// CRITICAL: ALL cells get equal headroom allocated (even empty ones) to support incremental updates.
    /// This prevents panics when entities are later inserted into previously-empty cells.
    /// 
    /// Per the design document (Section 2.8.2):
    /// - headroom_per_cell = (arena_capacity - total_entity_count) / num_cells
    /// - ALL cells get equal headroom for fairness and predictability
    /// - Entities are tightly packed at start of each cell's range
    /// 
    /// Behavior depends on overcapacity_ratio:
    /// - ratio ~= 1.0: Full rebuild mode (minimal headroom, rebuild every frame)
    /// - ratio > 1.1: Incremental mode (distribute extra capacity equally across cells)
    pub fn rebuild_with_headroom(&mut self, entities_by_cell: &[Vec<Entity>]) {
        let total_used: usize = entities_by_cell.iter().map(|v| v.len()).sum();
        let capacity = self.entity_storage.capacity();
        
        // Use overcapacity_ratio to decide strategy
        let use_incremental = self.overcapacity_ratio > 1.1;
        
        // FREE_ARENA_CAPACITY = capacity - total_used
        // For incremental mode, this includes the extra space from overcapacity_ratio
        let total_free_space = capacity.saturating_sub(total_used);
        
        // Each cell gets EQUAL share of free space for future insertions
        // This is the key invariant: ALL cells have equal headroom after rebuild
        let num_cells = self.cell_ranges.len();
        let headroom_per_cell = if use_incremental && num_cells > 0 {
            total_free_space / num_cells
        } else {
            0  // Full rebuild mode: no headroom
        };
        let extra_slots = if use_incremental && num_cells > 0 {
            total_free_space % num_cells  // Distribute remainder to first N cells
        } else {
            0
        };
        
        // CRITICAL: Different storage strategy based on mode
        self.entity_storage.clear();
        
        if use_incremental {
            // INCREMENTAL MODE: Pre-fill entity_storage with placeholders to full capacity
            // Direct indexing (entity_storage[idx] = entity) requires Vec to have actual elements!
            self.entity_storage.resize(capacity, Entity::PLACEHOLDER);
        }
        // FULL REBUILD MODE: Leave storage empty, will use push() during rebuild
        
        let mut write_pos = 0;
        
        // Empty vec for cells beyond entities_by_cell.len()
        let empty_vec: Vec<Entity> = Vec::new();
        
        // Process ALL cells (not just entities_by_cell)
        for cell_idx in 0..num_cells {
            let entities = if cell_idx < entities_by_cell.len() {
                &entities_by_cell[cell_idx]
            } else {
                &empty_vec
            };
            
            let count = entities.len();
            
            // Give first `extra_slots` cells one additional slot to handle remainder
            let this_cell_headroom = headroom_per_cell + if cell_idx < extra_slots { 1 } else { 0 };
            
            // Write entities based on mode
            if use_incremental {
                // INCREMENTAL: Direct assignment to pre-allocated slots
                for (i, &entity) in entities.iter().enumerate() {
                    self.entity_storage[write_pos + i] = entity;
                }
                // Leave remaining slots as PLACEHOLDER (headroom for future insertions)
            } else {
                // FULL REBUILD: Push to storage (contiguous, no gaps)
                for &entity in entities {
                    self.entity_storage.push(entity);
                }
            }
            
            // Set range for this cell
            // CRITICAL: max_index is exclusive (first position that would overflow)
            self.cell_ranges[cell_idx] = CellRange {
                start_index: write_pos,
                current_count: count,
                max_index: if use_incremental { write_pos + count + this_cell_headroom } else { 0 },
            };
            
            // Advance write_pos
            if use_incremental {
                write_pos += count + this_cell_headroom;
            } else {
                write_pos += count;
            }
        }
        
        self.entity_count = total_used;
    }
    
    /// Get all entities in a cell (returns slice of entity_storage)
    /// NOTE: May contain Entity::PLACEHOLDER tombstones - caller must filter
    /// 
    /// ZERO-ALLOCATION QUERY: Returns immutable view of cell's entities
    /// This is just a fat pointer (16 bytes) - NO HEAP ALLOCATION
    pub fn get_cell_entities(&self, col: usize, row: usize) -> &[Entity] {
        let cell_idx = row * self.cols + col;
        
        if cell_idx >= self.cell_ranges.len() {
            return &[];
        }
        
        let range = &self.cell_ranges[cell_idx];
        if range.current_count == 0 {
            return &[];
        }
        
        // Return slice of entity_storage (ZERO-COPY, just a view!)
        let end = range.start_index + range.current_count;
        if end <= self.entity_storage.len() {
            &self.entity_storage[range.start_index..end]
        } else {
            &[]
        }
    }
    
    /// Get all cells within radius of position
    /// ZERO-ALLOCATION: Uses preallocated out_cells buffer
    /// Clears out_cells before populating
    pub fn cells_in_radius(&self, pos: FixedVec2, radius: FixedNum, out_cells: &mut Vec<(usize, usize)>) {
        out_cells.clear();  // O(1), keeps capacity
        
        // Use pre-computed half_map constants instead of recalculating
        let min_x = pos.x - radius + self.half_map_width - self.offset.x;
        let max_x = pos.x + radius + self.half_map_width - self.offset.x;
        let min_y = pos.y - radius + self.half_map_height - self.offset.y;
        let max_y = pos.y + radius + self.half_map_height - self.offset.y;
        
        let min_col = (min_x / self.cell_size).floor().to_num::<isize>().max(0).min((self.cols - 1) as isize) as usize;
        let max_col = (max_x / self.cell_size).floor().to_num::<isize>().min((self.cols - 1) as isize) as usize;
        let min_row = (min_y / self.cell_size).floor().to_num::<isize>().max(0) as usize;
        let max_row = (max_y / self.cell_size).floor().to_num::<isize>().min((self.rows - 1) as isize) as usize;
        
        let capacity = out_cells.capacity();
        for row in min_row..=max_row {
            for col in min_col..=max_col {
                // CRITICAL: Check capacity before push to prevent reallocation
                if out_cells.len() < capacity {
                    out_cells.push((col, row));
                } else {
                    #[cfg(debug_assertions)]
                    panic!("Cell coords buffer overflow! Need more than {} capacity", capacity);
                    
                    #[cfg(not(debug_assertions))]
                    {
                        warn!("Cell coords buffer overflow - results truncated");
                        return;
                    }
                }
            }
        }
    }
    
    pub fn clear(&mut self) {
        // Clear entity storage (doesn't deallocate - keeps capacity)
        self.entity_storage.clear();
        self.entity_count = 0;
        
        // Reset all cell ranges (including max_index to force full rebuild mode)
        for range in &mut self.cell_ranges {
            range.start_index = 0;
            range.current_count = 0;
            range.max_index = 0; // Reset headroom - next insert will use full rebuild
        }
    }
    
    pub fn total_entries(&self) -> usize {
        self.entity_count
    }
    
    /// Calculate fragmentation ratio (tombstones / total storage)
    pub fn fragmentation_ratio(&self) -> f32 {
        if self.entity_storage.is_empty() {
            return 0.0;
        }
        
        let tombstones = self.entity_storage.iter()
            .filter(|&&e| e == Entity::PLACEHOLDER)
            .count();
        
        tombstones as f32 / self.entity_storage.len() as f32
    }
    
    /// Get storage usage ratio (current size / capacity)
    pub fn storage_usage_ratio(&self) -> f32 {
        let capacity = self.entity_storage.capacity();
        if capacity == 0 {
            return 0.0;
        }
        self.entity_storage.len() as f32 / capacity as f32
    }
    
    /// Compact the entity storage by removing tombstones
    /// This is the "cold path" that runs asynchronously
    pub fn compact(&mut self) {
        if self.entity_storage.is_empty() {
            return;
        }
        
        // Build new compacted storage
        let mut new_storage = Vec::with_capacity(self.entity_count);
        let mut new_ranges = vec![CellRange::new(); self.cell_ranges.len()];
        
        // Rebuild storage and ranges without tombstones
        for cell_idx in 0..self.cell_ranges.len() {
            let range = &self.cell_ranges[cell_idx];
            if range.current_count == 0 {
                continue;
            }
            
            let new_start = new_storage.len();
            let mut new_count = 0;
            
            // Copy non-tombstone entities
            let end = range.start_index + range.current_count;
            if end <= self.entity_storage.len() {
                for i in range.start_index..end {
                    let entity = self.entity_storage[i];
                    if entity != Entity::PLACEHOLDER {
                        new_storage.push(entity);
                        new_count += 1;
                    }
                }
            }
            
            new_ranges[cell_idx] = CellRange {
                start_index: new_start,
                current_count: new_count,
                max_index: new_count,  // Reset max_index to actual count after compaction
            };
        }
        
        // Replace old storage with compacted version
        self.entity_storage = new_storage;
        self.cell_ranges = new_ranges;
    }
}

/// One size class = one cell_size with two staggered grids
#[derive(Debug, Clone)]
pub struct SizeClass {
    pub cell_size: FixedNum,
    pub grid_a: StaggeredGrid,
    pub grid_b: StaggeredGrid,
    pub entity_count: usize,
}

impl SizeClass {
    pub fn new(map_width: FixedNum, map_height: FixedNum, cell_size: FixedNum) -> Self {
        Self::with_capacity(map_width, map_height, cell_size, 10_000_000, 1.0)
    }
    
    pub fn with_capacity(
        map_width: FixedNum, 
        map_height: FixedNum, 
        cell_size: FixedNum,
        max_entities: usize,
        overcapacity_ratio: f32,
    ) -> Self {
        let half_cell = cell_size / FixedNum::from_num(2.0);
        
        Self {
            cell_size,
            grid_a: StaggeredGrid::with_capacity(
                map_width, 
                map_height, 
                cell_size, 
                FixedVec2::ZERO,
                max_entities,
                overcapacity_ratio,
            ),
            grid_b: StaggeredGrid::with_capacity(
                map_width, 
                map_height, 
                cell_size, 
                FixedVec2::new(half_cell, half_cell),
                max_entities,
                overcapacity_ratio,
            ),
            entity_count: 0,
        }
    }
    
    pub fn clear(&mut self) {
        self.grid_a.clear();
        self.grid_b.clear();
        self.entity_count = 0;
    }
    
    pub fn total_entries(&self) -> usize {
        self.grid_a.total_entries() + self.grid_b.total_entries()
    }
    
    /// Compact both grids to remove tombstones
    pub fn compact(&mut self) {
        self.grid_a.compact();
        self.grid_b.compact();
    }
    
    /// Calculate average fragmentation across both grids
    pub fn fragmentation_ratio(&self) -> f32 {
        (self.grid_a.fragmentation_ratio() + self.grid_b.fragmentation_ratio()) / 2.0
    }
}
