use bevy::prelude::*;
use crate::game::fixed_math::{FixedNum, FixedVec2};
use super::SpatialHash;

impl SpatialHash {
    /// Query all entities within radius of position
    /// Excludes the query entity itself if provided
    /// 
    /// NOTE: Only returns Entity IDs. Callers must query SimPosition component for positions.
    pub fn get_potential_collisions(&self, pos: FixedVec2, query_radius: FixedNum, exclude_entity: Option<Entity>) -> Vec<Entity> {
        let mut result = Vec::new();
        
        // Query each size class (both Grid A and Grid B)
        for size_class in &self.size_classes {
            if size_class.entity_count == 0 {
                continue;  // Skip empty size classes
            }
            
            // Query Grid A
            let cells_a = size_class.grid_a.cells_in_radius(pos, query_radius);
            for cell in cells_a {
                for &entity in cell {
                    if Some(entity) != exclude_entity && !result.contains(&entity) {
                        result.push(entity);
                    }
                }
            }
            
            // Query Grid B
            let cells_b = size_class.grid_b.cells_in_radius(pos, query_radius);
            for cell in cells_b {
                for &entity in cell {
                    if Some(entity) != exclude_entity && !result.contains(&entity) {
                        result.push(entity);
                    }
                }
            }
        }
        
        result
    }

    /// General proximity query for boids, aggro, and other proximity-based systems.
    /// Returns all entities within the specified radius, excluding the query entity itself.
    /// 
    /// NOTE: Only returns Entity IDs. Callers must query SimPosition component for positions.
    pub fn query_radius(&self, query_entity: Entity, pos: FixedVec2, radius: FixedNum) -> Vec<Entity> {
        self.get_potential_collisions(pos, radius, Some(query_entity))
    }
}
