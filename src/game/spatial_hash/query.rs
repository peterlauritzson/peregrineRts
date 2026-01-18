use bevy::prelude::*;
use crate::game::fixed_math::{FixedNum, FixedVec2};
use super::SpatialHash;
use std::collections::HashSet;

impl SpatialHash {
    /// Query all entities within radius of position
    /// Excludes the query entity itself if provided
    /// 
    /// Populates `out_entities` instead of allocating a new Vec to avoid runtime allocations.
    /// Clears `out_entities` before populating.
    /// 
    /// NOTE: Only returns Entity IDs. Callers must query SimPosition component for positions.
    pub fn get_potential_collisions(&self, pos: FixedVec2, query_radius: FixedNum, exclude_entity: Option<Entity>, out_entities: &mut Vec<Entity>) {
        let mut seen = HashSet::new();
        out_entities.clear();
        
        // Query each size class (both Grid A and Grid B)
        for size_class in &self.size_classes {
            if size_class.entity_count == 0 {
                continue;  // Skip empty size classes
            }
            
            // Query Grid A
            let cells_a = size_class.grid_a.cells_in_radius(pos, query_radius);
            for (col, row) in cells_a {
                let entities = size_class.grid_a.get_cell_entities(col, row);
                for &entity in entities {
                    // Filter out tombstones and excluded entity
                    if entity != Entity::PLACEHOLDER && Some(entity) != exclude_entity && seen.insert(entity) {
                        out_entities.push(entity);
                    }
                }
            }
            
            // Query Grid B
            let cells_b = size_class.grid_b.cells_in_radius(pos, query_radius);
            for (col, row) in cells_b {
                let entities = size_class.grid_b.get_cell_entities(col, row);
                for &entity in entities {
                    // Filter out tombstones and excluded entity
                    if entity != Entity::PLACEHOLDER && Some(entity) != exclude_entity && seen.insert(entity) {
                        out_entities.push(entity);
                    }
                }
            }
        }
    }

    /// General proximity query for boids, aggro, and other proximity-based systems.
    /// Returns all entities within the specified radius, excluding the query entity itself.
    /// 
    /// Populates `out_entities` instead of allocating a new Vec to avoid runtime allocations.
    /// Clears `out_entities` before populating.
    /// 
    /// NOTE: Only returns Entity IDs. Callers must query SimPosition component for positions.
    pub fn query_radius(&self, query_entity: Entity, pos: FixedVec2, radius: FixedNum, out_entities: &mut Vec<Entity>) {
        self.get_potential_collisions(pos, radius, Some(query_entity), out_entities)
    }
}
