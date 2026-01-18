use bevy::prelude::*;
use crate::game::fixed_math::{FixedNum, FixedVec2};

/// Tracks which entities belong to a cell in the arena storage
#[derive(Debug, Clone, Copy)]
pub struct CellRange {
    pub start: usize,  // Index into entity_storage
    pub count: usize,  // Number of entities in this cell
}

impl CellRange {
    pub fn new() -> Self {
        Self { start: 0, count: 0 }
    }
    
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// One grid in a staggered pair using arena-based storage
#[derive(Debug, Clone)]
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
}

impl StaggeredGrid {
    pub fn new(map_width: FixedNum, map_height: FixedNum, cell_size: FixedNum, offset: FixedVec2) -> Self {
        Self::with_capacity(map_width, map_height, cell_size, offset, 10_000_000)
    }
    
    pub fn with_capacity(
        map_width: FixedNum, 
        map_height: FixedNum, 
        cell_size: FixedNum, 
        offset: FixedVec2,
        max_entities: usize,
    ) -> Self {
        let cols = (map_width / cell_size).ceil().to_num::<usize>() + 2;  // Extra padding
        let rows = (map_height / cell_size).ceil().to_num::<usize>() + 2;
        let num_cells = cols * rows;
        
        Self {
            // Pre-allocate entity storage to max capacity (zero allocation guarantee)
            entity_storage: Vec::with_capacity(max_entities),
            
            // One range per cell
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
    /// NOTE: With full rebuild every frame, ranges are always contiguous.
    /// Each cell's range extends as we append entities to that cell.
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
        
        // Check capacity before push (zero-allocation guarantee)
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
        
        // Append entity to storage
        let storage_idx = self.entity_storage.len();
        self.entity_storage.push(entity);
        self.entity_count += 1;
        
        // Update cell range
        // With full rebuild, ranges are always contiguous - just extend the range
        let range = &mut self.cell_ranges[cell_idx];
        if range.count == 0 {
            // First entity in this cell
            range.start = storage_idx;
            range.count = 1;
        } else {
            // Cell already has entities - should be immediately before this in storage
            // (because we're rebuilding in order)
            range.count += 1;
        }
        
        storage_idx
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
        if range.count == 0 {
            return None;
        }
        
        // Search for entity within the cell's range
        let end = range.start + range.count;
        if end > self.entity_storage.len() {
            warn!(
                "remove_entity: range.start {} + range.count {} exceeds storage len {}", 
                range.start, range.count, self.entity_storage.len()
            );
            return None;
        }
        
        for i in range.start..end {
            if self.entity_storage[i] == entity {
                // Mark as tombstone
                self.entity_storage[i] = Entity::PLACEHOLDER;
                self.entity_count -= 1;
                
                // NOTE: We DON'T decrement range.count here!
                // The range still spans the full area including tombstones.
                // Queries filter tombstones, and compaction will shrink the range properly.
                
                return Some(true);
            }
        }
        
        // Entity not found in this cell
        None
    }
    
    /// Get all entities in a cell (returns slice of entity_storage)
    /// NOTE: May contain Entity::PLACEHOLDER tombstones - caller must filter
    pub fn get_cell_entities(&self, col: usize, row: usize) -> &[Entity] {
        let cell_idx = row * self.cols + col;
        
        if cell_idx >= self.cell_ranges.len() {
            return &[];
        }
        
        let range = &self.cell_ranges[cell_idx];
        if range.count == 0 {
            return &[];
        }
        
        // Return slice of entity_storage
        let end = range.start + range.count;
        if end <= self.entity_storage.len() {
            &self.entity_storage[range.start..end]
        } else {
            &[]
        }
    }
    
    /// Get all cells within radius of position
    /// Returns iterator-friendly structure for querying
    pub fn cells_in_radius(&self, pos: FixedVec2, radius: FixedNum) -> Vec<(usize, usize)> {
        let mut result = Vec::new();
        
        let half_w = self.map_width / FixedNum::from_num(2.0);
        let half_h = self.map_height / FixedNum::from_num(2.0);
        
        let min_x = pos.x - radius + half_w - self.offset.x;
        let max_x = pos.x + radius + half_w - self.offset.x;
        let min_y = pos.y - radius + half_h - self.offset.y;
        let max_y = pos.y + radius + half_h - self.offset.y;
        
        let min_col = (min_x / self.cell_size).floor().to_num::<isize>().max(0) as usize;
        let max_col = (max_x / self.cell_size).floor().to_num::<isize>().min((self.cols - 1) as isize) as usize;
        let min_row = (min_y / self.cell_size).floor().to_num::<isize>().max(0) as usize;
        let max_row = (max_y / self.cell_size).floor().to_num::<isize>().min((self.rows - 1) as isize) as usize;
        
        for row in min_row..=max_row {
            for col in min_col..=max_col {
                result.push((col, row));
            }
        }
        
        result
    }
    
    pub fn clear(&mut self) {
        // Clear entity storage (doesn't deallocate - keeps capacity)
        self.entity_storage.clear();
        self.entity_count = 0;
        
        // Reset all cell ranges
        for range in &mut self.cell_ranges {
            range.start = 0;
            range.count = 0;
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
            if range.count == 0 {
                continue;
            }
            
            let new_start = new_storage.len();
            let mut new_count = 0;
            
            // Copy non-tombstone entities
            let end = range.start + range.count;
            if end <= self.entity_storage.len() {
                for i in range.start..end {
                    let entity = self.entity_storage[i];
                    if entity != Entity::PLACEHOLDER {
                        new_storage.push(entity);
                        new_count += 1;
                    }
                }
            }
            
            new_ranges[cell_idx] = CellRange {
                start: new_start,
                count: new_count,
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
        Self::with_capacity(map_width, map_height, cell_size, 10_000_000)
    }
    
    pub fn with_capacity(
        map_width: FixedNum, 
        map_height: FixedNum, 
        cell_size: FixedNum,
        max_entities: usize,
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
            ),
            grid_b: StaggeredGrid::with_capacity(
                map_width, 
                map_height, 
                cell_size, 
                FixedVec2::new(half_cell, half_cell),
                max_entities,
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
