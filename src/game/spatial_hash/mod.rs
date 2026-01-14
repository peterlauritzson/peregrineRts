use bevy::prelude::*;
use crate::game::fixed_math::FixedNum;

mod grid;
mod query;
#[cfg(test)]
mod tests;

pub use grid::{StaggeredGrid, SizeClass};
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

impl SpatialHash {
    /// Initialize spatial hash with staggered multi-resolution grids
    ///
    /// # Arguments
    /// * `map_width` - Width of the game map
    /// * `map_height` - Height of the game map
    /// * `entity_radii` - Expected entity sizes in game (e.g., [0.5, 10.0, 25.0])
    /// * `radius_to_cell_ratio` - Desired ratio between cell size and entity radius (e.g., 4.0)
    pub fn new(
        map_width: FixedNum,
        map_height: FixedNum,
        entity_radii: &[f32],
        radius_to_cell_ratio: f32,
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
            .map(|&cell_size| SizeClass::new(map_width, map_height, cell_size))
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
    
    pub fn resize(&mut self, map_width: FixedNum, map_height: FixedNum, entity_radii: &[f32], radius_to_cell_ratio: f32) {
        *self = Self::new(map_width, map_height, entity_radii, radius_to_cell_ratio);
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
                sc.grid_a.cells_in_radius(FixedVec2::ZERO, self.map_width).iter().filter(|c| !c.is_empty()).count() +
                sc.grid_b.cells_in_radius(FixedVec2::ZERO, self.map_width).iter().filter(|c| !c.is_empty()).count()
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
    
    /// Remove entity from spatial hash
    /// Returns Some(swapped_entity) if another entity was swapped to this index
    pub fn remove(&mut self, occupied: &OccupiedCell) -> Option<Entity> {
        let size_class = &mut self.size_classes[occupied.size_class as usize];
        
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
            // Remove from current cell
            self.remove(occupied);
            
            // Insert into new cell
            let size_class = &mut self.size_classes[occupied.size_class as usize];
            let grid = if new_grid_offset == 0 {
                &mut size_class.grid_a
            } else {
                &mut size_class.grid_b
            };
            
            let vec_idx = grid.insert_entity(new_col, new_row, entity);
            size_class.entity_count += 1;
            
            Some(OccupiedCell {
                size_class: occupied.size_class,
                grid_offset: new_grid_offset,
                col: new_col,
                row: new_row,
                vec_idx,
            })
        } else {
            None
        }
    }
    
    /// Update entity position in spatial hash, returning both new cell and any swapped entity
    /// Returns Some((new_occupied_cell, swapped_entity)) if entity changed cells
    pub fn update_with_swap(
        &mut self,
        entity: Entity,
        pos: FixedVec2,
        occupied: &OccupiedCell,
    ) -> Option<(OccupiedCell, Option<Entity>)> {
        if let Some((new_grid_offset, new_col, new_row)) = self.should_update(pos, occupied) {
            // Remove from current cell and capture any swapped entity
            let swapped_entity = self.remove(occupied);
            
            // Insert into new cell
            let size_class = &mut self.size_classes[occupied.size_class as usize];
            let grid = if new_grid_offset == 0 {
                &mut size_class.grid_a
            } else {
                &mut size_class.grid_b
            };
            
            let vec_idx = grid.insert_entity(new_col, new_row, entity);
            size_class.entity_count += 1;
            
            Some((
                OccupiedCell {
                    size_class: occupied.size_class,
                    grid_offset: new_grid_offset,
                    col: new_col,
                    row: new_row,
                    vec_idx,
                },
                swapped_entity,
            ))
        } else {
            None
        }
    }
    
    /// Insert entity into new cell (used by parallel updates)
    /// Returns the vec_idx where the entity was inserted
    pub fn insert_into_cell(&mut self, entity: Entity, new_occupied: &OccupiedCell) -> usize {
        let size_class = &mut self.size_classes[new_occupied.size_class as usize];
        let grid = if new_occupied.grid_offset == 0 {
            &mut size_class.grid_a
        } else {
            &mut size_class.grid_b
        };
        
        let vec_idx = grid.insert_entity(new_occupied.col, new_occupied.row, entity);
        size_class.entity_count += 1;
        vec_idx
    }
}


