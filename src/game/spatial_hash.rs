use bevy::prelude::*;
use crate::game::math::{FixedNum, FixedVec2};

#[derive(Resource)]
pub struct SpatialHash {
    cell_size: FixedNum,
    cols: usize,
    rows: usize,
    cells: Vec<Vec<Entity>>,
    map_width: FixedNum,
    map_height: FixedNum,
}

impl SpatialHash {
    pub fn new(map_width: FixedNum, map_height: FixedNum, cell_size: FixedNum) -> Self {
        let cols = (map_width / cell_size).ceil().to_num::<usize>() + 1;
        let rows = (map_height / cell_size).ceil().to_num::<usize>() + 1;
        
        Self {
            cell_size,
            cols,
            rows,
            cells: vec![Vec::new(); cols * rows],
            map_width,
            map_height,
        }
    }

    pub fn resize(&mut self, map_width: FixedNum, map_height: FixedNum, cell_size: FixedNum) {
        let cols = (map_width / cell_size).ceil().to_num::<usize>() + 1;
        let rows = (map_height / cell_size).ceil().to_num::<usize>() + 1;
        
        self.map_width = map_width;
        self.map_height = map_height;
        self.cell_size = cell_size;
        self.cols = cols;
        self.rows = rows;
        self.cells = vec![Vec::new(); cols * rows];
    }

    pub fn clear(&mut self) {
        for cell in &mut self.cells {
            cell.clear();
        }
    }

    fn get_cell_idx(&self, pos: FixedVec2) -> Option<usize> {
        // Map is centered at 0,0. Coordinates are [-half_w, half_w].
        // Shift to [0, w]
        let half_w = self.map_width / FixedNum::from_num(2.0);
        let half_h = self.map_height / FixedNum::from_num(2.0);
        
        let x = pos.x + half_w;
        let y = pos.y + half_h;
        
        if x < FixedNum::ZERO || x >= self.map_width || y < FixedNum::ZERO || y >= self.map_height {
            return None;
        }
        
        let col = (x / self.cell_size).to_num::<usize>();
        let row = (y / self.cell_size).to_num::<usize>();
        
        if col >= self.cols || row >= self.rows {
            return None;
        }
        
        Some(row * self.cols + col)
    }

    pub fn insert(&mut self, entity: Entity, pos: FixedVec2) {
        if let Some(idx) = self.get_cell_idx(pos) {
            self.cells[idx].push(entity);
        }
    }

    pub fn get_potential_collisions(&self, pos: FixedVec2, query_radius: FixedNum) -> Vec<Entity> {
        let mut result = Vec::new();
        
        let half_w = self.map_width / FixedNum::from_num(2.0);
        let half_h = self.map_height / FixedNum::from_num(2.0);
        
        let min_x = pos.x - query_radius + half_w;
        let max_x = pos.x + query_radius + half_w;
        let min_y = pos.y - query_radius + half_h;
        let max_y = pos.y + query_radius + half_h;
        
        let min_col = (min_x / self.cell_size).floor().to_num::<isize>().max(0) as usize;
        let max_col = (max_x / self.cell_size).floor().to_num::<isize>().min((self.cols as isize) - 1) as usize;
        let min_row = (min_y / self.cell_size).floor().to_num::<isize>().max(0) as usize;
        let max_row = (max_y / self.cell_size).floor().to_num::<isize>().min((self.rows as isize) - 1) as usize;

        for row in min_row..=max_row {
            for col in min_col..=max_col {
                let idx = row * self.cols + col;
                if idx < self.cells.len() {
                    result.extend_from_slice(&self.cells[idx]);
                }
            }
        }
        
        result
    }
}
