use bevy::prelude::*;
use crate::game::fixed_math::{FixedNum, FixedVec2};
use super::{SpatialHash, SpatialHashScratch};

impl SpatialHash {
    /// Low-level query: Get all entities in a specific cell across all size classes
    /// 
    /// ZERO-ALLOCATION: Uses preallocated buffers from `scratch`.
    /// Results are appended to scratch.query_results (does NOT clear).
    /// 
    /// # Arguments
    /// * `col` - Column index
    /// * `row` - Row index  
    /// * `exclude_entity` - Optional entity to exclude from results
    /// * `scratch` - Scratch buffer for results
    pub fn query_cell(&self, col: usize, row: usize, exclude_entity: Option<Entity>, scratch: &mut SpatialHashScratch) {
        let capacity = scratch.query_results.capacity();
        
        for size_class in &self.size_classes {
            if size_class.entity_count == 0 {
                continue;
            }
            
            // Check Grid A
            let entities = size_class.grid_a.get_cell_entities(col, row);
            for &entity in entities {
                if entity != Entity::PLACEHOLDER && Some(entity) != exclude_entity && scratch.seen_entities.insert(entity) {
                    if scratch.query_results.len() < capacity {
                        scratch.query_results.push(entity);
                    } else {
                        #[cfg(debug_assertions)]
                        panic!("Query result buffer overflow! Need more capacity than {}", capacity);
                        
                        #[cfg(not(debug_assertions))]
                        {
                            warn!("Query result buffer overflow - results truncated at {} entities", capacity);
                            return;
                        }
                    }
                }
            }
            
            // Check Grid B
            let entities = size_class.grid_b.get_cell_entities(col, row);
            for &entity in entities {
                if entity != Entity::PLACEHOLDER && Some(entity) != exclude_entity && scratch.seen_entities.insert(entity) {
                    if scratch.query_results.len() < capacity {
                        scratch.query_results.push(entity);
                    } else {
                        #[cfg(debug_assertions)]
                        panic!("Query result buffer overflow! Need more capacity than {}", capacity);
                        
                        #[cfg(not(debug_assertions))]
                        {
                            warn!("Query result buffer overflow - results truncated at {} entities", capacity);
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Query all entities within radius of position
    /// 
    /// ZERO-ALLOCATION: Uses preallocated buffers from `scratch`.
    /// Clears buffers before populating.
    /// 
    /// # Arguments
    /// * `pos` - Query position
    /// * `radius` - Query radius
    /// * `exclude_entity` - Optional entity to exclude from results (e.g., the querying entity itself)
    /// * `scratch` - Scratch buffer for results
    /// 
    /// # Results
    /// Entities are in `scratch.query_results` after call.
    /// 
    /// CRITICAL: Checks capacity before push to prevent reallocation.
    /// If buffer overflows, logs warning and truncates results (no panic in release).
    /// 
    /// NOTE: Only returns Entity IDs. Callers must query SimPosition component for positions.
    pub fn query_radius(&self, pos: FixedVec2, radius: FixedNum, exclude_entity: Option<Entity>, scratch: &mut SpatialHashScratch) {
        scratch.seen_entities.clear();  // O(1), keeps capacity
        scratch.query_results.clear();
        
        let capacity = scratch.query_results.capacity();
        
        // Query each size class (both Grid A and Grid B)
        for size_class in &self.size_classes {
            if size_class.entity_count == 0 {
                continue;  // Skip empty size classes
            }
            
            // Query Grid A
            size_class.grid_a.cells_in_radius(pos, radius, &mut scratch.cell_coords);
            let num_cells = scratch.cell_coords.len();
            for i in 0..num_cells {
                let (col, row) = scratch.cell_coords[i];
                let entities = size_class.grid_a.get_cell_entities(col, row);
                for &entity in entities {
                    // Filter out tombstones and excluded entity, use scratch.seen_entities
                    if entity != Entity::PLACEHOLDER && Some(entity) != exclude_entity && scratch.seen_entities.insert(entity) {
                        // CRITICAL: Check capacity before push to prevent reallocation
                        if scratch.query_results.len() < capacity {
                            scratch.query_results.push(entity);
                        } else {
                            #[cfg(debug_assertions)]
                            panic!("Query result buffer overflow! Need more capacity than {}", capacity);
                            
                            #[cfg(not(debug_assertions))]
                            {
                                warn!("Query result buffer overflow - results truncated at {} entities", capacity);
                                return;  // Truncate results
                            }
                        }
                    }
                }
            }
            
            // Query Grid B
            size_class.grid_b.cells_in_radius(pos, radius, &mut scratch.cell_coords);
            let num_cells = scratch.cell_coords.len();
            for i in 0..num_cells {
                let (col, row) = scratch.cell_coords[i];
                let entities = size_class.grid_b.get_cell_entities(col, row);
                for &entity in entities {
                    // Filter out tombstones and excluded entity, use scratch.seen_entities
                    if entity != Entity::PLACEHOLDER && Some(entity) != exclude_entity && scratch.seen_entities.insert(entity) {
                        // CRITICAL: Check capacity before push to prevent reallocation
                        if scratch.query_results.len() < capacity {
                            scratch.query_results.push(entity);
                        } else {
                            #[cfg(debug_assertions)]
                            panic!("Query result buffer overflow! Need more capacity than {}", capacity);
                            
                            #[cfg(not(debug_assertions))]
                            {
                                warn!("Query result buffer overflow - results truncated at {} entities", capacity);
                                return;  // Truncate results
                            }
                        }
                    }
                }
            }
        }
    }
}
