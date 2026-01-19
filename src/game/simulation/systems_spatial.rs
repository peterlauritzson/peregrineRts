/// Spatial partitioning and flow field management systems
///
/// This module contains systems responsible for:
/// - Updating the spatial hash with entity positions
/// - Initializing and managing flow fields
/// - Applying obstacles to flow fields dynamically

use bevy::prelude::*;
use crate::game::fixed_math::{FixedVec2, FixedNum};
use crate::game::pathfinding::{HierarchicalGraph, CLUSTER_SIZE};
use crate::game::spatial_hash::SpatialHash;
use crate::game::structures::{FlowField, CELL_SIZE};

use crate::game::simulation::components::*;
use crate::game::simulation::resources::*;
use super::systems_config::SpatialHashRebuilt;

// ============================================================================
// Spatial Hash
// ============================================================================

/// Update spatial hash with entity positions
/// 
/// **Dual Update Strategy:**
/// - **Full Rebuild Mode** (overcapacity_ratio = 1.0): O(N) rebuild every frame, simple and fast for <1M entities
/// - **Incremental Mode** (overcapacity_ratio > 1.1): O(moved entities), tracks movement and only updates changed cells
/// 
/// The mode is auto-detected based on overcapacity_ratio configured in initial_config.ron.
/// See SPATIAL_PARTITIONING.md Section 2.8 for performance analysis.
pub fn update_spatial_hash(
    mut spatial_hash: ResMut<SpatialHash>,
    query: Query<(Entity, &SimPosition, &Collider, &OccupiedCell), Without<StaticObstacle>>,
    query_new: Query<(Entity, &SimPosition, &Collider), (Without<StaticObstacle>, Without<OccupiedCell>)>,
    mut commands: Commands,
    _sim_config: Res<SimConfig>,
    rebuilt: Option<Res<SpatialHashRebuilt>>,
) {
    // If spatial hash was rebuilt (e.g., map resize), just clear the marker
    if rebuilt.is_some() {
        commands.remove_resource::<SpatialHashRebuilt>();
        info!("Spatial hash rebuilt - will repopulate this frame");
    }
    
    // Determine if we're using incremental updates
    // This is auto-detected: overcapacity_ratio > 1.1 means incremental mode
    let use_incremental = spatial_hash.uses_incremental_updates();
    
    if use_incremental {
        // INCREMENTAL MODE: Only update entities that changed cells
        let mut moved_count = 0;
        let mut rebuild_needed = false;
        
        // 1. Check for moved entities (have OccupiedCell component)
        for (entity, pos, _collider, occupied) in query.iter() {
            if let Some((new_grid_offset, new_col, new_row)) = spatial_hash.should_update(pos.0, occupied) {
                // Entity changed cells - perform incremental update
                if spatial_hash.update_incremental(entity, occupied, new_grid_offset, new_col, new_row) {
                    moved_count += 1;
                    
                    // Update OccupiedCell component
                    commands.entity(entity).insert(OccupiedCell {
                        size_class: occupied.size_class,
                        grid_offset: new_grid_offset,
                        col: new_col,
                        row: new_row,
                        vec_idx: 0, // Will be set by insert
                    });
                }
            }
        }
        
        // 2. Insert new entities (don't have OccupiedCell yet)
        for (entity, pos, collider) in query_new.iter() {
            let occupied = spatial_hash.insert(entity, pos.0, collider.radius);
            commands.entity(entity).insert(occupied);
        }
        
        // 3. Check if rebuild is needed (any cell exceeded headroom capacity)
        if spatial_hash.should_rebuild() {
            rebuild_needed = true;
        }
        
        // 4. Perform full rebuild if needed
        if rebuild_needed {
            debug!("Spatial hash overflow detected - rebuilding with headroom redistribution");
            
            spatial_hash.rebuild_all_with_headroom(|| {
                query.iter()
                    .map(|(entity, pos, collider, occupied)| {
                        (entity, pos.0, collider.radius, occupied.clone())
                    })
                    .collect()
            });
            
            // Update all OccupiedCell components after rebuild
            for (entity, pos, collider, _old_occupied) in query.iter() {
                let new_occupied = spatial_hash.insert(entity, pos.0, collider.radius);
                commands.entity(entity).insert(new_occupied);
            }
        }
        
        if moved_count > 0 {
            trace!("Incremental update: {} entities moved cells", moved_count);
        }
    } else {
        // FULL REBUILD MODE: Clear and repopulate every frame
        // This is the correct approach per design doc: "Rebuilt every physics tick"
        //
        // Benefits:
        // - Zero fragmentation (no tombstones)
        // - Consistent performance (no spikes from compaction)
        // - Simple and correct (no complex incremental update logic)
        // - Efficient with arena storage (just reset ranges, reuse storage)
        //
        // Cost: O(N) where N = entity count, but very fast (append-only with pre-allocated storage)
        
        spatial_hash.clear();
        
        for (entity, pos, collider) in query.iter().map(|(e, p, c, _)| (e, p, c))
            .chain(query_new.iter())
        {
            // Insert fresh - OccupiedCell component is managed but not used in full rebuild mode
            spatial_hash.insert(entity, pos.0, collider.radius);
        }
    }
}

// ============================================================================
// Spatial Hash Compaction
// ============================================================================
// ============================================================================
// NOTE: Compaction is NO LONGER NEEDED with full rebuild every frame!
// The spatial hash is completely rebuilt each tick, so there's zero fragmentation.
// ============================================================================

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
    _graph: ResMut<HierarchicalGraph>,
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
            
            // Affected clusters - region-based system rebuilds entire graph when obstacles change
            // TODO: Implement incremental region decomposition for dynamic obstacles
            for cy in min_cluster_y..=max_cluster_y {
                for cx in min_cluster_x..=max_cluster_x {
                    let _cluster_key = (cx, cy);
                    // Region decomposition would need to be rerun here
                }
            }
        }
    }
}
