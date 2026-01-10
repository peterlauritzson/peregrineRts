use bevy::prelude::*;
use crate::game::fixed_math::{FixedNum, FixedVec2};
use super::SpatialHash;

impl SpatialHash {
    pub(crate) fn get_cell_idx(&self, pos: FixedVec2) -> Option<usize> {
        // Map is centered at 0,0. Coordinates are [-half_w, half_w].
        // Shift to [0, w]
        let half_w = self.map_width() / FixedNum::from_num(2.0);
        let half_h = self.map_height() / FixedNum::from_num(2.0);
        
        let x = pos.x + half_w;
        let y = pos.y + half_h;
        
        if x < FixedNum::ZERO || x >= self.map_width() || y < FixedNum::ZERO || y >= self.map_height() {
            return None;
        }
        
        let col = (x / self.cell_size()).to_num::<usize>();
        let row = (y / self.cell_size()).to_num::<usize>();
        
        if col >= self.cols() || row >= self.rows() {
            return None;
        }
        
        Some(row * self.cols() + col)
    }

    pub fn insert(&mut self, entity: Entity, pos: FixedVec2) {
        if let Some(idx) = self.get_cell_idx(pos) {
            self.cells_mut()[idx].push((entity, pos));
        }
    }

    /// Insert with logging for debugging obstacle detection
    pub fn insert_with_log(&mut self, entity: Entity, pos: FixedVec2, is_obstacle: bool, _radius: Option<FixedNum>) {
        if let Some(idx) = self.get_cell_idx(pos) {
            self.cells_mut()[idx].push((entity, pos));
            // Removed per-obstacle logging - too verbose
        } else {
            if is_obstacle {
                warn!("[SPATIAL_HASH] Failed to insert OBSTACLE entity {:?} at pos ({:.2}, {:.2}) - position out of bounds",
                    entity, pos.x.to_num::<f32>(), pos.y.to_num::<f32>());
            }
        }
    }

    /// Remove an entity from a specific cell.
    /// Used when an entity moves from one cell to another.
    pub fn remove(&mut self, entity: Entity, col: usize, row: usize) {
        let idx = row * self.cols() + col;
        if idx < self.cells().len() {
            self.cells_mut()[idx].retain(|&(e, _)| e != entity);
        }
    }

    /// Remove an entity from multiple cells.
    /// Used when updating entities that occupy multiple cells.
    pub fn remove_multi_cell(&mut self, entity: Entity, cells: &[(usize, usize)]) {
        for &(col, row) in cells {
            self.remove(entity, col, row);
        }
    }

    /// Calculate all grid cells that an entity's radius overlaps.
    /// Returns a vector of (col, row) tuples.
    ///
    /// # Multi-Cell Storage
    ///
    /// This is critical for correct collision detection with variable entity sizes.
    /// An entity is inserted into **all** cells its radius overlaps, ensuring that
    /// queries from nearby entities will always find it.
    ///
    /// # Example
    ///
    /// - Small entity (radius 0.5, cell_size 2.0): Occupies 1-4 cells
    /// - Medium obstacle (radius 10): Occupies ~25 cells  
    /// - Large obstacle (radius 20): Occupies ~100 cells
    pub fn calculate_occupied_cells(&self, pos: FixedVec2, radius: FixedNum) -> Vec<(usize, usize)> {
        let mut cells = Vec::new();
        
        let half_w = self.map_width() / FixedNum::from_num(2.0);
        let half_h = self.map_height() / FixedNum::from_num(2.0);
        
        // Calculate the bounding box of cells the entity overlaps
        let min_x = pos.x - radius + half_w;
        let max_x = pos.x + radius + half_w;
        let min_y = pos.y - radius + half_h;
        let max_y = pos.y + radius + half_h;
        
        // Convert to grid coordinates, clamped to valid range
        // IMPORTANT: Must clamp to 0 AFTER min() to avoid usize underflow!
        let min_col = (min_x / self.cell_size()).floor().to_num::<isize>().max(0) as usize;
        let max_col = (max_x / self.cell_size()).floor().to_num::<isize>().min((self.cols() as isize) - 1).max(0) as usize;
        let min_row = (min_y / self.cell_size()).floor().to_num::<isize>().max(0) as usize;
        let max_row = (max_y / self.cell_size()).floor().to_num::<isize>().min((self.rows() as isize) - 1).max(0) as usize;
        
        // Generate all cells in the bounding box
        for row in min_row..=max_row {
            for col in min_col..=max_col {
                cells.push((col, row));
            }
        }
        
        cells
    }

    /// Insert an entity into all cells its radius overlaps.
    /// Returns the list of cells the entity was inserted into.
    ///
    /// This should be used for all entity insertions to ensure correct collision
    /// detection with variable entity sizes.
    pub fn insert_multi_cell(&mut self, entity: Entity, pos: FixedVec2, radius: FixedNum) -> Vec<(usize, usize)> {
        let cells = self.calculate_occupied_cells(pos, radius);
        
        // Insert into each overlapping cell
        for &(col, row) in &cells {
            let idx = row * self.cols() + col;
            if idx < self.cells().len() {
                self.cells_mut()[idx].push((entity, pos));
            }
        }
        
        cells
    }

    /// Insert an entity into all cells its radius overlaps, with logging.
    /// Returns the list of cells the entity was inserted into.
    pub fn insert_multi_cell_with_log(&mut self, entity: Entity, pos: FixedVec2, radius: FixedNum, is_obstacle: bool) -> Vec<(usize, usize)> {
        let cells = self.calculate_occupied_cells(pos, radius);
        
        // Only log summary for obstacles, not every cell
        if is_obstacle && cells.len() > 20 {
            info!("[SPATIAL_HASH] Inserting OBSTACLE entity {:?} at pos ({:.2}, {:.2}) with radius {:.2} into {} cells",
                entity, pos.x.to_num::<f32>(), pos.y.to_num::<f32>(), radius.to_num::<f32>(), cells.len());
        }
        
        // Insert into each overlapping cell
        for &(col, row) in &cells {
            let idx = row * self.cols() + col;
            if idx < self.cells().len() {
                self.cells_mut()[idx].push((entity, pos));
            }
        }
        
        cells
    }
}
