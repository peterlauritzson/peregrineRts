/// Spatial partitioning and flow field management systems
///
/// This module contains systems responsible for:
/// - Updating the spatial hash with entity positions
/// - Initializing and managing flow fields
/// - Applying obstacles to flow fields dynamically

use bevy::prelude::*;
use crate::game::fixed_math::{FixedVec2, FixedNum};
use crate::game::pathfinding::{HierarchicalGraph, CLUSTER_SIZE, regenerate_cluster_flow_fields};
use crate::game::spatial_hash::SpatialHash;
use crate::game::structures::{FlowField, CELL_SIZE};

use crate::game::simulation::components::*;
use crate::game::simulation::resources::*;

// ============================================================================
// Spatial Hash
// ============================================================================

/// Update spatial hash with entity positions
pub fn update_spatial_hash(
    mut spatial_hash: ResMut<SpatialHash>,
    mut query: Query<(Entity, &SimPosition, &Collider, &mut OccupiedCell), Without<StaticObstacle>>,
    new_entities: Query<(Entity, &SimPosition, &Collider), Without<OccupiedCell>>,
    mut commands: Commands,
    _sim_config: Res<SimConfig>,  // Keep for signature compatibility
) {
    
    
    // Handle entities that don't have OccupiedCell yet (first time in spatial hash)
    for (entity, pos, collider) in new_entities.iter() {
        // Insert entity into spatial hash (automatically classifies by size and picks optimal grid)
        let occupied = spatial_hash.insert(entity, pos.0, collider.radius);
        commands.entity(entity).insert(occupied);
    }
    
    // Collect index updates needed for swapped entities (avoid double-borrow)
    let mut pending_index_updates: Vec<(Entity, usize, usize, usize)> = Vec::new();
    
    // Handle dynamic entities - check if they should update cells
    for (entity, pos, _collider, mut occupied_cell) in query.iter_mut() {
        // Check if entity moved closer to opposite grid
        // Note: spatial_hash.update() internally handles remove + insert
        if let Some((new_occupied, swapped_entity)) = spatial_hash.update_with_swap(entity, pos.0, &occupied_cell) {
            // Entity changed cells - check if anything was swapped
            if let Some(swapped) = swapped_entity {
                // Queue index update for swapped entity
                pending_index_updates.push((
                    swapped,
                    occupied_cell.col,
                    occupied_cell.row,
                    occupied_cell.vec_idx,
                ));
            }
            
            *occupied_cell = new_occupied;
        }
    }
    
    // Second pass: Apply pending index updates to swapped entities
    for (swapped_entity, col, row, new_idx) in pending_index_updates {
        if let Ok((_, _, _, mut swapped_occupied)) = query.get_mut(swapped_entity) {
            // Update the vec_idx if this is the right cell
            if swapped_occupied.col == col && swapped_occupied.row == row {
                swapped_occupied.vec_idx = new_idx;
            }
        }
    }
}

// ============================================================================
// Flow Field Management
// ============================================================================

/// Initialize flow field at startup
pub fn init_flow_field(
    mut map_flow_field: ResMut<MapFlowField>,
    sim_config: Res<SimConfig>,
) {
    let width = (sim_config.map_width / FixedNum::from_num(CELL_SIZE)).ceil().to_num::<usize>();
    let height = (sim_config.map_height / FixedNum::from_num(CELL_SIZE)).ceil().to_num::<usize>();
    let cell_size = FixedNum::from_num(CELL_SIZE);
    let origin = FixedVec2::new(
        -sim_config.map_width / FixedNum::from_num(2.0),
        -sim_config.map_height / FixedNum::from_num(2.0),
    );

    map_flow_field.0 = FlowField::new(width, height, cell_size, origin);
}

/// Apply an obstacle to the flow field cost map
pub fn apply_obstacle_to_flow_field(flow_field: &mut FlowField, pos: FixedVec2, radius: FixedNum) {
    // Rasterize circle
    // Even if center is outside, part of it might be inside.
    // But world_to_grid returns None if outside.
    // We should compute bounding box in grid coords.
    
    let min_world = pos - FixedVec2::new(radius, radius);
    let max_world = pos + FixedVec2::new(radius, radius);
    
    // Convert to grid coords manually to handle out of bounds
    let cell_size = flow_field.cell_size;
    let origin = flow_field.origin;
    
    let min_local = min_world - origin;
    let max_local = max_world - origin;
    
    let min_x = (min_local.x / cell_size).floor().to_num::<i32>();
    let min_y = (min_local.y / cell_size).floor().to_num::<i32>();
    let max_x = (max_local.x / cell_size).ceil().to_num::<i32>();
    let max_y = (max_local.y / cell_size).ceil().to_num::<i32>();
    
    for y in min_y..max_y {
        for x in min_x..max_x {
            if x >= 0 && x < flow_field.width as i32 && y >= 0 && y < flow_field.height as i32 {
                let cell_center = flow_field.grid_to_world(x as usize, y as usize);
                
                // Block cells whose center is within the obstacle radius
                // This matches the actual collision radius used by physics
                let dist_sq = (cell_center - pos).length_squared();
                let threshold = radius;
                
                if dist_sq < threshold * threshold {
                    flow_field.set_obstacle(x as usize, y as usize);
                }
            }
        }
    }
}

/// Apply newly added obstacles to flow field and invalidate affected cluster caches
pub fn apply_new_obstacles(
    mut map_flow_field: ResMut<MapFlowField>,
    mut graph: ResMut<HierarchicalGraph>,
    obstacles: Query<(&SimPosition, &Collider), Added<StaticObstacle>>,
) {
    let obstacle_count = obstacles.iter().count();
    if obstacle_count == 0 {
        return;
    }
    
    
    let flow_field = &mut map_flow_field.0;
    
    for (_i, (pos, collider)) in obstacles.iter().enumerate() {
        apply_obstacle_to_flow_field(flow_field, pos.0, collider.radius);
        
        // Invalidate affected cluster caches so units reroute around the new obstacle
        // Determine which clusters are affected by this obstacle
        let obstacle_world_pos = pos.0;
        let grid_pos = flow_field.world_to_grid(obstacle_world_pos);
        
        if let Some((grid_x, grid_y)) = grid_pos {
            // Calculate the radius in grid cells
            let radius_cells = (collider.radius / flow_field.cell_size).ceil().to_num::<usize>();
            
            // Find all affected clusters
            let min_x: usize = grid_x.saturating_sub(radius_cells);
            let max_x = (grid_x + radius_cells).min(flow_field.width - 1);
            let min_y: usize = grid_y.saturating_sub(radius_cells);
            let max_y = (grid_y + radius_cells).min(flow_field.height - 1);
            
            let min_cluster_x = min_x / CLUSTER_SIZE;
            let max_cluster_x = max_x / CLUSTER_SIZE;
            let min_cluster_y = min_y / CLUSTER_SIZE;
            let max_cluster_y = max_y / CLUSTER_SIZE;
            
            // Invalidate all affected clusters and regenerate their flow fields
            for cy in min_cluster_y..=max_cluster_y {
                for cx in min_cluster_x..=max_cluster_x {
                    let cluster_key = (cx, cy);
                    graph.clear_cluster_cache(cluster_key);
                    // Regenerate flow fields for this cluster immediately
                    regenerate_cluster_flow_fields(&mut graph, flow_field, cluster_key);
                }
            }
        }
    }
}
