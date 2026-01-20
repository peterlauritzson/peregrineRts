use bevy::prelude::*;
use std::collections::HashSet;
use crate::game::fixed_math::FixedNum;

mod grid;
mod query;
#[cfg(test)]
mod tests;

pub use grid::{StaggeredGrid, SizeClass, CellRange};
use crate::game::fixed_math::FixedVec2;
use crate::game::simulation::components::OccupiedCell;

/// Staggered Multi-Resolution Spatial Hash for efficient proximity queries.
///
/// **NEW DESIGN (January 2026):**
/// - Multiple cell sizes for different entity size ranges
/// - Each cell size has TWO offset grids (Grid A and Grid B) staggered by half_cell
/// - Entities are ALWAYS single-cell (inserted into whichever grid they're closest to center of)
/// - Memory: 25 bytes per entity (vs 96 bytes for old multi-cell approach)
/// - Update threshold: ~half_cell distance (much rarer than multi-cell updates)
///
/// See SPATIAL_PARTITIONING.md Section 2.2 for detailed explanation.
///
/// # Example
///
/// ```rust
/// use bevy::prelude::Entity;
/// use peregrine::game::fixed_math::{FixedNum, FixedVec2};
/// use peregrine::game::spatial_hash::SpatialHash;
///
/// let mut hash = SpatialHash::new(
///     FixedNum::from_num(100.0),  // map width
///     FixedNum::from_num(100.0),  // map height  
///     &[0.5, 10.0],               // entity radii
///     4.0                         // radius to cell ratio
/// );
///
/// // Entities are automatically classified by size and inserted into appropriate grid
/// ```
#[derive(Resource)]
pub struct SpatialHash {
    /// Array of size classes, each with staggered grids
    size_classes: Vec<SizeClass>,
    
    /// Map from entity radius to size class index
    /// Precomputed during initialization: [(max_radius, class_index), ...]
    radius_to_class: Vec<(FixedNum, u8)>,
    
    map_width: FixedNum,
    map_height: FixedNum,
}

/// Preallocated scratch buffers for zero-allocation queries
/// 
/// Per the design document (Section 2.3), all queries must use preallocated buffers
/// to maintain zero-allocation guarantee. This resource holds these buffers.
/// 
/// Usage:
/// ```rust
/// fn my_system(
///     spatial_hash: Res<SpatialHash>,
///     mut scratch: ResMut<SpatialHashScratch>,
/// ) {
///     // Query will use scratch buffers (zero allocation)
///     spatial_hash.query_radius(pos, radius, Some(entity), &mut scratch);
///     
///     // Process results from scratch.query_results
///     for &entity in &scratch.query_results {
///         // ... use entity ...
///     }
/// }
/// ```
#[derive(Resource)]
pub struct SpatialHashScratch {
    /// Preallocated buffer for query results
    /// Capacity based on worst-case query size (e.g., 10,000 entities)
    pub query_results: Vec<Entity>,
    
    /// Preallocated buffer for secondary queries (e.g., nested queries)
    pub query_results_secondary: Vec<Entity>,
    
    /// Preallocated HashSet for deduplicating entities across Grid A and Grid B
    /// Avoids allocating new HashSet on every query
    pub seen_entities: HashSet<Entity>,
    
    /// Preallocated buffer for cell coordinates in radius queries
    /// Capacity needed: ((2 * max_radius / min_cell_size) + 1)^2
    /// Example: radius=60, cell_size=10 → (60*2/10+1)^2 = 13^2 = 169 cells
    /// Conservatively allocated to 256 to handle edge cases
    pub cell_coords: Vec<(usize, usize)>,
}

impl SpatialHashScratch {
    /// Create scratch buffers with specified capacities
    /// 
    /// # Arguments
    /// * `query_capacity` - Max entities that can be returned by a single query
    /// 
    /// For estimating capacity:
    /// - cells_per_query = ((query_radius * 2.0 / cell_size).ceil() + 1)^2
    /// - max_entities_per_cell = (max_entities / num_cells) * safety_factor
    /// - query_capacity = cells_per_query * max_entities_per_cell
    pub fn new(query_capacity: usize) -> Self {
        Self {
            query_results: Vec::with_capacity(query_capacity),
            query_results_secondary: Vec::with_capacity(query_capacity),
            seen_entities: HashSet::with_capacity(query_capacity),
            cell_coords: Vec::with_capacity(4096),  // Worst case: large radius on fine grid (e.g., r=60, cell=2 → 61²=3721)
        }
    }
    
    /// Create with default capacity (10,000 entities per query)
    pub fn default_capacity() -> Self {
        Self::new(10_000)
    }
}

impl SpatialHash {
    /// Initialize spatial hash with staggered multi-resolution grids
    ///
    /// # Arguments
    /// * `map_width` - Width of the game map
    /// * `map_height` - Height of the game map
    /// * `entity_radii` - Expected entity sizes in game (e.g., [0.5, 10.0, 25.0])
    /// * `radius_to_cell_ratio` - Desired ratio between cell size and entity radius (e.g., 4.0)
    /// * `max_entity_count` - Maximum entities per grid (pre-allocated capacity)
    /// * `overcapacity_ratio` - Arena over-provisioning (1.5 = 50% extra for incremental updates)
    pub fn new(
        map_width: FixedNum,
        map_height: FixedNum,
        entity_radii: &[f32],
        radius_to_cell_ratio: f32,
        max_entity_count: usize,
        overcapacity_ratio: f32,
    ) -> Self {
        // Step 1: Determine unique cell sizes needed
        let mut cell_sizes = Vec::new();
        for &radius in entity_radii {
            let cell_size = FixedNum::from_num(radius) * FixedNum::from_num(radius_to_cell_ratio);
            
            // Merge similar cell sizes (within 20% of each other)
            let existing = cell_sizes.iter().find(|&&cs: &&FixedNum| {
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
        let size_classes: Vec<SizeClass> = cell_sizes.iter()
            .map(|&cell_size| SizeClass::with_capacity(
                map_width, 
                map_height, 
                cell_size, 
                max_entity_count,
                overcapacity_ratio
            ))
            .collect();
        
        // Step 3: Build radius-to-class mapping
        let mut radius_to_class = Vec::new();
        for (idx, &cell_size) in cell_sizes.iter().enumerate() {
            let max_radius = cell_size / FixedNum::from_num(radius_to_cell_ratio);
            radius_to_class.push((max_radius, idx as u8));
        }
        
        Self {
            size_classes,
            radius_to_class,
            map_width,
            map_height,
        }
    }
    
    pub fn resize(&mut self, map_width: FixedNum, map_height: FixedNum, entity_radii: &[f32], radius_to_cell_ratio: f32, max_entity_count: usize, overcapacity_ratio: f32) {
        *self = Self::new(map_width, map_height, entity_radii, radius_to_cell_ratio, max_entity_count, overcapacity_ratio);
    }

    pub fn clear(&mut self) {
        for size_class in &mut self.size_classes {
            size_class.clear();
        }
    }

    pub fn total_entries(&self) -> usize {
        self.size_classes.iter().map(|sc| sc.total_entries()).sum()
    }

    pub fn non_empty_cells(&self) -> usize {
        // Count across all grids in all size classes
        self.size_classes.iter()
            .map(|sc| {
                sc.grid_a.cell_ranges.iter().filter(|r| !r.is_empty()).count() +
                sc.grid_b.cell_ranges.iter().filter(|r| !r.is_empty()).count()
            })
            .sum()
    }

    // Getters
    pub fn map_width(&self) -> FixedNum { self.map_width }
    pub fn map_height(&self) -> FixedNum { self.map_height }
    
    // For compatibility with old API (returns cell size of first size class)
    pub fn cell_size(&self) -> FixedNum {
        self.size_classes.first().map(|sc| sc.cell_size).unwrap_or(FixedNum::from_num(2.0))
    }
    
    pub fn cols(&self) -> usize {
        self.size_classes.first().map(|sc| sc.grid_a.cols).unwrap_or(0)
    }
    
    pub fn rows(&self) -> usize {
        self.size_classes.first().map(|sc| sc.grid_a.rows).unwrap_or(0)
    }
    
    /// Get reference to size classes (for advanced usage)
    pub fn size_classes(&self) -> &[SizeClass] {
        &self.size_classes
    }
    
    // ============================================================================
    // Entity Management (Insert, Remove, Update)
    // ============================================================================
    
    /// Classify entity by radius to determine which size class it belongs to
    fn classify_entity(&self, radius: FixedNum) -> u8 {
        for &(max_radius, class_idx) in &self.radius_to_class {
            if radius <= max_radius {
                return class_idx;
            }
        }
        // Default to largest size class
        (self.size_classes.len() - 1) as u8
    }
    
    /// Insert entity into spatial hash
    /// Returns OccupiedCell component to attach to the entity
    pub fn insert(&mut self, entity: Entity, pos: FixedVec2, radius: FixedNum) -> OccupiedCell {
        let size_class_idx = self.classify_entity(radius);
        let size_class = &mut self.size_classes[size_class_idx as usize];
        
        // Find nearest center in Grid A
        let (col_a, row_a) = size_class.grid_a.pos_to_cell(pos);
        let center_a = size_class.grid_a.cell_center(col_a, row_a);
        let dist_a_sq = (pos - center_a).length_squared();
        
        // Find nearest center in Grid B
        let (col_b, row_b) = size_class.grid_b.pos_to_cell(pos);
        let center_b = size_class.grid_b.cell_center(col_b, row_b);
        let dist_b_sq = (pos - center_b).length_squared();
        
        // Insert into whichever grid is closer
        let (grid_offset, col, row, storage_idx) = if dist_a_sq < dist_b_sq {
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
            vec_idx: storage_idx,  // Now stores index into entity_storage arena
        }
    }
    
    /// Remove entity from spatial hash
    /// Returns Some(true) if removal succeeded
    pub fn remove(&mut self, entity: Entity, occupied: &OccupiedCell) -> Option<bool> {
        let size_class = &mut self.size_classes[occupied.size_class as usize];
        
        let grid = if occupied.grid_offset == 0 {
            &mut size_class.grid_a
        } else {
            &mut size_class.grid_b
        };
        
        let removed = grid.remove_entity(occupied.col, occupied.row, entity);
        
        if removed.is_some() {
            size_class.entity_count -= 1;
        }
        
        removed
    }
    
    /// Check if entity should update its cell (moved closer to opposite grid)
    /// Returns Some(new_occupied_cell) if entity should be re-inserted
    pub fn should_update(&self, pos: FixedVec2, occupied: &OccupiedCell) -> Option<(u8, usize, usize)> {
        let size_class = &self.size_classes[occupied.size_class as usize];
        
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
        let (opp_col, opp_row) = opposite_grid.pos_to_cell(pos);
        let opposite_center = opposite_grid.cell_center(opp_col, opp_row);
        
        // Check if now closer to opposite grid
        let dist_current = (pos - current_center).length_squared();
        let dist_opposite = (pos - opposite_center).length_squared();
        
        if dist_opposite < dist_current {
            let opposite_offset = if occupied.grid_offset == 0 { 1 } else { 0 };
            Some((opposite_offset, opp_col, opp_row))
        } else {
            None
        }
    }
    
    /// Update entity position in spatial hash
    /// Returns Some(new_occupied_cell) if entity changed cells
    pub fn update(
        &mut self,
        entity: Entity,
        pos: FixedVec2,
        occupied: &OccupiedCell,
    ) -> Option<OccupiedCell> {
        if let Some((new_grid_offset, new_col, new_row)) = self.should_update(pos, occupied) {
            // Remove from current cell (mark as tombstone)
            self.remove(entity, occupied);
            
            // Insert into new cell
            let size_class = &mut self.size_classes[occupied.size_class as usize];
            let grid = if new_grid_offset == 0 {
                &mut size_class.grid_a
            } else {
                &mut size_class.grid_b
            };
            
            let storage_idx = grid.insert_entity(new_col, new_row, entity);
            size_class.entity_count += 1;
            
            Some(OccupiedCell {
                size_class: occupied.size_class,
                grid_offset: new_grid_offset,
                col: new_col,
                row: new_row,
                vec_idx: storage_idx,  // Still track for now, but won't rely on it for removal
            })
        } else {
            None
        }
    }
    
    /// Update entity position in spatial hash, returning both new cell and success status
    /// Returns Some((new_occupied_cell, true)) if entity changed cells
    pub fn update_with_swap(
        &mut self,
        entity: Entity,
        pos: FixedVec2,
        occupied: &OccupiedCell,
    ) -> Option<(OccupiedCell, Option<bool>)> {
        if let Some((new_grid_offset, new_col, new_row)) = self.should_update(pos, occupied) {
            // Remove from current cell and capture success status
            let removed = self.remove(entity, occupied);
            
            // Insert into new cell
            let size_class = &mut self.size_classes[occupied.size_class as usize];
            let grid = if new_grid_offset == 0 {
                &mut size_class.grid_a
            } else {
                &mut size_class.grid_b
            };
            
            let storage_idx = grid.insert_entity(new_col, new_row, entity);
            size_class.entity_count += 1;
            
            Some((
                OccupiedCell {
                    size_class: occupied.size_class,
                    grid_offset: new_grid_offset,
                    col: new_col,
                    row: new_row,
                    vec_idx: storage_idx,
                },
                removed,
            ))
        } else {
            None
        }
    }
    
    /// Insert entity into new cell (used by parallel updates)
    /// Returns the storage_idx where the entity was inserted
    pub fn insert_into_cell(&mut self, entity: Entity, new_occupied: &OccupiedCell) -> usize {
        let size_class = &mut self.size_classes[new_occupied.size_class as usize];
        let grid = if new_occupied.grid_offset == 0 {
            &mut size_class.grid_a
        } else {
            &mut size_class.grid_b
        };
        
        let storage_idx = grid.insert_entity(new_occupied.col, new_occupied.row, entity);
        size_class.entity_count += 1;
        storage_idx
    }
    
    /// Compact all grids in all size classes if fragmentation exceeds threshold
    /// Returns true if any compaction was performed
    pub fn compact_if_fragmented(&mut self, fragmentation_threshold: f32) -> bool {
        let mut compacted = false;
        
        for size_class in &mut self.size_classes {
            let frag_ratio = size_class.fragmentation_ratio();
            
            if frag_ratio > fragmentation_threshold {
                debug!(
                    "Compacting size class (cell_size={:.1}): fragmentation {:.1}%", 
                    size_class.cell_size.to_num::<f32>(),
                    frag_ratio * 100.0
                );
                size_class.compact();
                compacted = true;
            }
        }
        
        compacted
    }
    
    /// Get total fragmentation ratio across all grids
    pub fn fragmentation_ratio(&self) -> f32 {
        if self.size_classes.is_empty() {
            return 0.0;
        }
        
        let total: f32 = self.size_classes.iter()
            .map(|sc| sc.fragmentation_ratio())
            .sum();
        
        total / self.size_classes.len() as f32
    }
    
    /// Get storage usage ratio (current storage size / capacity)
    /// Returns the worst-case (highest) usage across all grids
    pub fn storage_usage_ratio(&self) -> f32 {
        let mut max_usage: f32 = 0.0;
        
        for size_class in &self.size_classes {
            let usage_a = size_class.grid_a.storage_usage_ratio();
            let usage_b = size_class.grid_b.storage_usage_ratio();
            max_usage = max_usage.max(usage_a).max(usage_b);
        }
        
        max_usage
    }
    
    // ============================================================================
    // Incremental Update Support (Arena Over-Provisioning Strategy)
    // ============================================================================
    
    /// Check if incremental updates are enabled (auto-detected from overcapacity_ratio > 1.1)
    pub fn uses_incremental_updates(&self) -> bool {
        self.size_classes.iter().any(|sc| {
            sc.grid_a.use_incremental_updates || sc.grid_b.use_incremental_updates
        })
    }
    
    /// Check if any grid needs a full rebuild due to capacity overflow
    /// Returns true if any cell has exceeded its headroom capacity
    pub fn should_rebuild(&self) -> bool {
        self.size_classes.iter().any(|sc| {
            sc.grid_a.should_rebuild() || sc.grid_b.should_rebuild()
        })
    }
    
    /// Perform incremental update: remove from old cell, insert into new cell
    /// Uses swap-with-last-element trick for O(1) removal with zero fragmentation
    /// 
    /// Returns Ok((new_vec_idx, swapped_entity_option)) or Err if operation failed
    /// - new_vec_idx: new position of moved entity
    /// - swapped_entity: entity that needs vec_idx update (was swapped during removal)
    pub fn update_incremental(
        &mut self,
        entity: Entity,
        occupied: &OccupiedCell,
        new_grid_offset: u8,
        new_col: usize,
        new_row: usize,
    ) -> Result<(usize, Option<Entity>), &'static str> {
        let size_class = &mut self.size_classes[occupied.size_class as usize];
        
        // Remove from old cell using swap-based removal (O(1) with vec_idx!)
        let old_grid = if occupied.grid_offset == 0 {
            &mut size_class.grid_a
        } else {
            &mut size_class.grid_b
        };
        
        let (success, swapped_entity) = old_grid.remove_entity_swap(
            occupied.col,
            occupied.row,
            occupied.vec_idx,
        );
        
        if !success {
            return Err("Entity not found in old cell");
        }
        
        // Insert into new cell
        let new_grid = if new_grid_offset == 0 {
            &mut size_class.grid_a
        } else {
            &mut size_class.grid_b
        };
        
        let new_vec_idx = new_grid.insert_entity(new_col, new_row, entity);
        if new_vec_idx == usize::MAX {
            return Err("Cell overflow - rebuild needed");
        }
        
        Ok((new_vec_idx, swapped_entity))
    }
    
    /// Rebuild all grids with proportional headroom distribution
    /// Call this when should_rebuild() returns true
    /// 
    /// entities_callback: Function that yields (Entity, pos, radius, occupied) for all entities
    pub fn rebuild_all_with_headroom<F>(&mut self, mut entities_callback: F)
    where
        F: FnMut() -> Vec<(Entity, FixedVec2, FixedNum, OccupiedCell)>,
    {
        let entities = entities_callback();
        
        // Group entities by size class
        let mut by_class: Vec<Vec<(Entity, FixedVec2, OccupiedCell)>> = 
            vec![Vec::new(); self.size_classes.len()];
        
        for (entity, pos, _radius, occupied) in entities {
            by_class[occupied.size_class as usize].push((entity, pos, occupied));
        }
        
        // Rebuild each size class
        for (class_idx, entities) in by_class.iter().enumerate() {
            if entities.is_empty() {
                continue;
            }
            
            let size_class = &mut self.size_classes[class_idx];
            
            // Create temporary cell collections for Grid A and Grid B
            let grid_a_cols = size_class.grid_a.cols;
            let grid_a_rows = size_class.grid_a.rows;
            let grid_b_cols = size_class.grid_b.cols;
            let grid_b_rows = size_class.grid_b.rows;
            
            let mut grid_a_cells: Vec<Vec<Entity>> = vec![Vec::new(); grid_a_cols * grid_a_rows];
            let mut grid_b_cells: Vec<Vec<Entity>> = vec![Vec::new(); grid_b_cols * grid_b_rows];
            
            // Distribute entities into cells
            for &(entity, _pos, ref occupied) in entities {
                if occupied.grid_offset == 0 {
                    let cell_idx = occupied.row * grid_a_cols + occupied.col;
                    grid_a_cells[cell_idx].push(entity);
                } else {
                    let cell_idx = occupied.row * grid_b_cols + occupied.col;
                    grid_b_cells[cell_idx].push(entity);
                }
            }
            
            // Rebuild with headroom
            size_class.grid_a.rebuild_with_headroom(&grid_a_cells);
            size_class.grid_b.rebuild_with_headroom(&grid_b_cells);
        }
    }
    
    /// Rebuild spatial hash from a flat list of entities
    /// Used in full rebuild mode where we don't have OccupiedCell components
    /// 
    /// This properly distributes entities into cells and calls rebuild_with_headroom
    /// to maintain correct arena structure.
    pub fn rebuild_from_entity_list(&mut self, entities: &[(Entity, FixedVec2, FixedNum)]) {
        // Group entities by size class and cell
        for size_class in &mut self.size_classes {
            size_class.entity_count = 0;
        }
        
        // Create cell collections for each size class
        let mut size_class_cells: Vec<(Vec<Vec<Entity>>, Vec<Vec<Entity>>)> = self.size_classes.iter()
            .map(|sc| {
                let grid_a_cells = vec![Vec::new(); sc.grid_a.cols * sc.grid_a.rows];
                let grid_b_cells = vec![Vec::new(); sc.grid_b.cols * sc.grid_b.rows];
                (grid_a_cells, grid_b_cells)
            })
            .collect();
        
        // Classify and distribute entities
        for &(entity, pos, radius) in entities {
            let size_class_idx = self.classify_entity(radius) as usize;
            let size_class = &self.size_classes[size_class_idx];
            
            // Find nearest center in Grid A
            let (col_a, row_a) = size_class.grid_a.pos_to_cell(pos);
            let center_a = size_class.grid_a.cell_center(col_a, row_a);
            let dist_a_sq = (pos - center_a).length_squared();
            
            // Find nearest center in Grid B
            let (col_b, row_b) = size_class.grid_b.pos_to_cell(pos);
            let center_b = size_class.grid_b.cell_center(col_b, row_b);
            let dist_b_sq = (pos - center_b).length_squared();
            
            // Insert into whichever grid is closer
            let (grid_a_cells, grid_b_cells) = &mut size_class_cells[size_class_idx];
            if dist_a_sq < dist_b_sq {
                let cell_idx = row_a * size_class.grid_a.cols + col_a;
                grid_a_cells[cell_idx].push(entity);
            } else {
                let cell_idx = row_b * size_class.grid_b.cols + col_b;
                grid_b_cells[cell_idx].push(entity);
            }
        }
        
        // Rebuild each size class with proper headroom distribution
        for (size_class_idx, (grid_a_cells, grid_b_cells)) in size_class_cells.into_iter().enumerate() {
            let size_class = &mut self.size_classes[size_class_idx];
            size_class.grid_a.rebuild_with_headroom(&grid_a_cells);
            size_class.grid_b.rebuild_with_headroom(&grid_b_cells);
            size_class.entity_count = size_class.grid_a.total_entries() + size_class.grid_b.total_entries();
        }
    }
}
