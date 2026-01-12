use bevy::prelude::*;
use crate::game::fixed_math::{FixedNum, FixedVec2};

/// One grid in a staggered pair
#[derive(Debug, Clone)]
pub struct StaggeredGrid {
    cells: Vec<Vec<Entity>>,
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
}

impl StaggeredGrid {
    pub fn new(map_width: FixedNum, map_height: FixedNum, cell_size: FixedNum, offset: FixedVec2) -> Self {
        let cols = (map_width / cell_size).ceil().to_num::<usize>() + 2;  // Extra padding
        let rows = (map_height / cell_size).ceil().to_num::<usize>() + 2;
        
        Self {
            cells: vec![Vec::new(); cols * rows],
            cols,
            rows,
            cell_size,
            offset,
            map_width,
            map_height,
            half_map_width: map_width / FixedNum::from_num(2.0),
            half_map_height: map_height / FixedNum::from_num(2.0),
            half_cell: FixedNum::from_num(0.5),
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
    
    /// Insert entity into cell and return Vec index
    pub fn insert_entity(&mut self, col: usize, row: usize, entity: Entity) -> usize {
        let idx = row * self.cols + col;
        let vec_idx = self.cells[idx].len();
        self.cells[idx].push(entity);
        vec_idx
    }
    
    /// Remove entity from cell using Vec index (O(1) swap_remove)
    /// Returns Some(swapped_entity) if an entity was swapped to this index
    pub fn remove_entity(&mut self, col: usize, row: usize, vec_idx: usize) -> Option<Entity> {
        let idx = row * self.cols + col;
        if idx < self.cells.len() && vec_idx < self.cells[idx].len() {
            self.cells[idx].swap_remove(vec_idx);
            // If we swapped, return the entity now at vec_idx
            if vec_idx < self.cells[idx].len() {
                Some(self.cells[idx][vec_idx])
            } else {
                None
            }
        } else {
            None
        }
    }
    
    /// Get all cells within radius of position
    pub fn cells_in_radius(&self, pos: FixedVec2, radius: FixedNum) -> Vec<&Vec<Entity>> {
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
                let idx = row * self.cols + col;
                if idx < self.cells.len() {
                    result.push(&self.cells[idx]);
                }
            }
        }
        
        result
    }
    
    pub fn clear(&mut self) {
        for cell in &mut self.cells {
            cell.clear();
        }
    }
    
    pub fn total_entries(&self) -> usize {
        self.cells.iter().map(|cell| cell.len()).sum()
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
        let half_cell = cell_size / FixedNum::from_num(2.0);
        
        Self {
            cell_size,
            grid_a: StaggeredGrid::new(map_width, map_height, cell_size, FixedVec2::ZERO),
            grid_b: StaggeredGrid::new(map_width, map_height, cell_size, FixedVec2::new(half_cell, half_cell)),
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
}

