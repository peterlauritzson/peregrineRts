use bevy::prelude::*;
use crate::game::fixed_math::{FixedNum, FixedVec2};
use super::SpatialHash;

impl SpatialHash {
    /// Returns all entities within query_radius of pos.
    /// If exclude_entity is Some, that entity will be excluded from results.
    /// This avoids wasted self-collision checks in collision detection.
    /// 
    /// NOTE: Only returns Entity IDs. Callers must query SimPosition component for positions.
    pub fn get_potential_collisions(&self, pos: FixedVec2, query_radius: FixedNum, exclude_entity: Option<Entity>) -> Vec<Entity> {
        self.get_potential_collisions_internal(pos, query_radius, exclude_entity, false)
    }

    /// Get potential collisions with optional debug logging
    pub fn get_potential_collisions_with_log(&self, pos: FixedVec2, query_radius: FixedNum, exclude_entity: Option<Entity>) -> Vec<Entity> {
        self.get_potential_collisions_internal(pos, query_radius, exclude_entity, true)
    }

    fn get_potential_collisions_internal(&self, pos: FixedVec2, query_radius: FixedNum, exclude_entity: Option<Entity>, log: bool) -> Vec<Entity> {
        let mut result = Vec::new();
        
        let half_w = self.map_width() / FixedNum::from_num(2.0);
        let half_h = self.map_height() / FixedNum::from_num(2.0);
        
        let min_x = pos.x - query_radius + half_w;
        let max_x = pos.x + query_radius + half_w;
        let min_y = pos.y - query_radius + half_h;
        let max_y = pos.y + query_radius + half_h;
        
        let min_col = (min_x / self.cell_size()).floor().to_num::<isize>().max(0) as usize;
        let max_col = (max_x / self.cell_size()).floor().to_num::<isize>().min((self.cols() as isize) - 1) as usize;
        let min_row = (min_y / self.cell_size()).floor().to_num::<isize>().max(0) as usize;
        let max_row = (max_y / self.cell_size()).floor().to_num::<isize>().min((self.rows() as isize) - 1) as usize;
        
        if log {
            info!("[SPATIAL_HASH_QUERY] Querying from pos ({:.2}, {:.2}) with radius {:.2}",
                pos.x.to_num::<f32>(), pos.y.to_num::<f32>(), query_radius.to_num::<f32>());
            info!("[SPATIAL_HASH_QUERY] Checking cells - cols: {}..={}, rows: {}..={}",
                min_col, max_col, min_row, max_row);
        }

        for row in min_row..=max_row {
            for col in min_col..=max_col {
                let idx = row * self.cols() + col;
                if idx < self.cells().len() {
                    if log && !self.cells()[idx].is_empty() {
                        info!("[SPATIAL_HASH_QUERY] Cell [{}, {}] (idx {}) contains {} entities",
                            col, row, idx, self.cells()[idx].len());
                    }
                    
                    if let Some(exclude) = exclude_entity {
                        // Exclude specific entity from results
                        for &entity in &self.cells()[idx] {
                            if entity != exclude {
                                result.push(entity);
                            }
                        }
                    } else {
                        // Include all entities
                        result.extend(self.cells()[idx].iter().copied());
                    }
                }
            }
        }
        
        if log {
            info!("[SPATIAL_HASH_QUERY] Found {} potential collision entities", result.len());
        }
        
        result
    }

    /// General proximity query for boids, aggro, and other proximity-based systems.
    /// Returns all entities within the specified radius, excluding the query entity itself.
    /// This enables O(1) amortized queries instead of O(N) brute force.
    /// 
    /// NOTE: Only returns Entity IDs. Callers must query SimPosition component for positions.
    pub fn query_radius(&self, query_entity: Entity, pos: FixedVec2, radius: FixedNum) -> Vec<Entity> {
        let mut result = Vec::new();
        
        let half_w = self.map_width() / FixedNum::from_num(2.0);
        let half_h = self.map_height() / FixedNum::from_num(2.0);
        
        let min_x = pos.x - radius + half_w;
        let max_x = pos.x + radius + half_w;
        let min_y = pos.y - radius + half_h;
        let max_y = pos.y + radius + half_h;
        
        let min_col = (min_x / self.cell_size()).floor().to_num::<isize>().max(0) as usize;
        let max_col = (max_x / self.cell_size()).floor().to_num::<isize>().min((self.cols() as isize) - 1) as usize;
        let min_row = (min_y / self.cell_size()).floor().to_num::<isize>().max(0) as usize;
        let max_row = (max_y / self.cell_size()).floor().to_num::<isize>().min((self.rows() as isize) - 1) as usize;

        for row in min_row..=max_row {
            for col in min_col..=max_col {
                let idx = row * self.cols() + col;
                if idx < self.cells().len() {
                    for &entity in &self.cells()[idx] {
                        // Exclude self from results to avoid wasted cycles
                        if entity != query_entity {
                            result.push(entity);
                        }
                    }
                }
            }
        }
        
        result
    }
}
