/// Core simulation systems.
///
/// This module contains systems for:
/// - Input processing
/// - Path following
/// - Spatial hash updates  
/// - Flow field management
/// - Simulation timing/performance tracking

use bevy::prelude::*;
use crate::game::fixed_math::{FixedVec2, FixedNum};
use crate::game::config::{GameConfig, GameConfigHandle, InitialConfig};
use crate::game::pathfinding::{Path, PathRequest, HierarchicalGraph, CLUSTER_SIZE, regenerate_cluster_flow_fields};
use crate::game::spatial_hash::SpatialHash;
use crate::game::structures::{FlowField, CELL_SIZE};
use peregrine_macros::profile;

use super::components::*;
use super::resources::*;
use super::events::*;
use super::physics::seek;

// ============================================================================
// Tick Management
// ============================================================================

/// Increment the global simulation tick counter.
/// 
/// This system runs first in the FixedUpdate schedule to ensure all other
/// systems have access to the current tick value for deterministic logic
/// and conditional logging.
pub fn increment_sim_tick(mut tick: ResMut<SimTick>) {
    tick.increment();
}

// ============================================================================
// Input Processing
// ============================================================================

/// Process player input commands deterministically
pub fn process_input(
    mut commands: Commands,
    mut move_events: MessageReader<UnitMoveCommand>,
    mut stop_events: MessageReader<UnitStopCommand>,
    mut spawn_events: MessageReader<SpawnUnitCommand>,
    mut path_requests: MessageWriter<PathRequest>,
    query: Query<&SimPosition>,
) {
    
    
    // Deterministic Input Processing:
    // 1. Collect all events
    // 2. Sort by Player ID (and potentially sequence number if we had one)
    // 3. Execute in order
    
    // Handle Stop Commands
    let mut stops: Vec<&UnitStopCommand> = stop_events.read().collect();
    stops.sort_by_key(|e| e.player_id);

    for event in stops {
        commands.entity(event.entity).remove::<Path>();
        // Also reset velocity?
        commands.entity(event.entity).insert(SimVelocity(FixedVec2::ZERO));
    }

    // Handle Move Commands
    let mut moves: Vec<&UnitMoveCommand> = move_events.read().collect();
    moves.sort_by_key(|e| e.player_id);
    
    for event in moves {
        if let Ok(pos) = query.get(event.entity) {
            // Send Path Request instead of setting target directly
            path_requests.write(PathRequest {
                entity: event.entity,
                start: pos.0,
                goal: event.target,
            });
            // Remove old path component to stop movement until path is found
            commands.entity(event.entity).remove::<Path>();
        }
    }

    // Handle Spawn Commands
    let mut spawns: Vec<&SpawnUnitCommand> = spawn_events.read().collect();
    spawns.sort_by_key(|e| e.player_id);

    for event in spawns {
        // Note: In a real game, we'd need a way to deterministically assign Entity IDs 
        // or use a reservation system. For now, we let Bevy spawn.
        // To be strictly deterministic across clients, we would need to reserve Entity IDs 
        // or use a deterministic ID generator.
        commands.spawn((
            crate::game::GameEntity,
            crate::game::unit::Unit,
            crate::game::unit::Health { current: 100.0, max: 100.0 },
            SimPosition(event.position),
            SimPositionPrev(event.position),
            SimVelocity(FixedVec2::ZERO),
            SimAcceleration(FixedVec2::ZERO),
            Collider::default(),
            CachedNeighbors::default(),
            BoidsNeighborCache::default(),
            OccupiedCell::default(), // Will be populated on first spatial hash update
        ));
    }
}

// ============================================================================
// Path Following
// ============================================================================

/// Follow assigned paths using flow fields and steering
pub fn follow_path(
    mut commands: Commands,
    mut query: Query<(Entity, &SimPosition, &SimVelocity, &mut SimAcceleration, &mut Path, &CachedNeighbors)>,
    no_path_query: Query<Entity, (Without<Path>, With<SimPosition>)>,
    sim_config: Res<SimConfig>,
    mut graph: ResMut<HierarchicalGraph>,
    map_flow_field: Res<MapFlowField>,
) {
    
    let speed = sim_config.unit_speed;
    let max_force = sim_config.steering_force;
    let dt = FixedNum::ONE / FixedNum::from_num(sim_config.tick_rate);
    let step_dist = speed * dt;
    let threshold = if step_dist > sim_config.arrival_threshold { step_dist } else { sim_config.arrival_threshold };
    let threshold_sq = threshold * threshold;
    
    // Arrival spacing parameters to prevent pile-ups
    let arrival_radius = FixedNum::from_num(0.5); // Stop 0.5 units from exact target
    let arrival_radius_sq = arrival_radius * arrival_radius;
    const CROWDING_THRESHOLD: usize = 50; // Number of stopped units to consider "crowded"
    
    let flow_field = &map_flow_field.0;
    #[cfg(feature = "perf_stats")]
    let mut early_arrivals = 0;

    for (entity, pos, vel, mut acc, mut path, cache) in query.iter_mut() {
        match &mut *path {
            Path::Direct(target) => {
                let delta = *target - pos.0;
                let dist_sq = delta.length_squared();
                
                if dist_sq < threshold_sq {
                    commands.entity(entity).remove::<Path>();
                    acc.0 = acc.0 - vel.0 * sim_config.braking_force;
                    continue;
                }
                
                // Check for crowding at destination (pile-up prevention)
                if dist_sq < arrival_radius_sq {
                    // Count nearby stopped units (units without Path component)
                    let stopped_count = cache.neighbors.iter()
                        .filter(|&neighbor_entity| no_path_query.contains(*neighbor_entity))
                        .count();
                    
                    if stopped_count > CROWDING_THRESHOLD {
                        // Destination is crowded - arrive early to prevent pile-up
                        #[cfg(feature = "perf_stats")]
                        {
                            early_arrivals += 1;
                        }
                        commands.entity(entity).remove::<Path>();
                        acc.0 = acc.0 - vel.0 * sim_config.braking_force;
                        continue;
                    }
                }
                
                seek(pos.0, *target, vel.0, &mut acc.0, speed, max_force);
            },
            Path::LocalAStar { waypoints, current_index } => {
                if *current_index >= waypoints.len() {
                    let braking_force = -vel.0 * sim_config.braking_force; 
                    acc.0 = acc.0 + braking_force;
                    continue;
                }

                let target = waypoints[*current_index];
                let delta = target - pos.0;
                let dist_sq = delta.length_squared();
                
                if dist_sq < threshold_sq {
                     *current_index += 1;
                     if *current_index >= waypoints.len() {
                         commands.entity(entity).remove::<Path>();
                     }
                     continue;
                }
                seek(pos.0, target, vel.0, &mut acc.0, speed, max_force);
            },
            Path::Hierarchical { goal, goal_cluster } => {
                // Lazy routing table walk: lookup next portal on-demand
                let current_grid = flow_field.world_to_grid(pos.0);
                if let Some((gx, gy)) = current_grid {
                    let cx = gx / CLUSTER_SIZE;
                    let cy = gy / CLUSTER_SIZE;
                    let current_cluster = (cx, cy);
                    
                    if current_cluster == *goal_cluster {
                        // In final cluster - navigate directly to goal
                        let delta = *goal - pos.0;
                        if delta.length_squared() < threshold_sq {
                            commands.entity(entity).remove::<Path>();
                            acc.0 = acc.0 - vel.0 * sim_config.braking_force;
                            continue;
                        }
                        seek(pos.0, *goal, vel.0, &mut acc.0, speed, max_force);
                    } else {
                        // Lookup next portal from routing table
                        if let Some(next_portal_id) = graph.get_next_portal(current_cluster, *goal_cluster) {
                            if let Some(portal) = graph.nodes.get(next_portal_id).cloned() {
                                // Navigate to portal using cluster's flow field
                                if let Some(cluster) = graph.clusters.get_mut(&current_cluster) {
                                    let local_field = cluster.get_or_generate_flow_field(next_portal_id, &portal, flow_field);
                                    
                                    let min_x = cx * CLUSTER_SIZE;
                                    let min_y = cy * CLUSTER_SIZE;
                                    
                                    if gx >= min_x && gy >= min_y {
                                        let lx = gx - min_x;
                                        let ly = gy - min_y;
                                        let idx = ly * local_field.width + lx;
                                        
                                        if idx < local_field.vectors.len() {
                                            let dir = local_field.vectors[idx];
                                            if dir != FixedVec2::ZERO {
                                                let desired_vel = dir * speed;
                                                let steer = desired_vel - vel.0;
                                                let steer_len_sq = steer.length_squared();
                                                let final_steer = if steer_len_sq > max_force * max_force {
                                                    steer.normalize() * max_force
                                                } else {
                                                    steer
                                                };
                                                acc.0 = acc.0 + final_steer;
                                            } else {
                                                let target_pos = flow_field.grid_to_world(portal.node.x, portal.node.y);
                                                seek(pos.0, target_pos, vel.0, &mut acc.0, speed, max_force);
                                            }
                                        }
                                    }
                                } else {
                                    // Fallback: seek directly to portal
                                    let target_pos = flow_field.grid_to_world(portal.node.x, portal.node.y);
                                    seek(pos.0, target_pos, vel.0, &mut acc.0, speed, max_force);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

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
        if let Some(new_occupied) = spatial_hash.update(entity, pos.0, &occupied_cell) {
            // Entity changed cells - check if anything was swapped
            if let Some(swapped_entity) = spatial_hash.remove(&occupied_cell) {
                // Queue index update for swapped entity
                pending_index_updates.push((
                    swapped_entity,
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

/// Parallel spatial hash update using zero-contention fold/reduce
/// 
/// Phase 1 (Parallel): Each thread builds its own Vec independently - ZERO mutex contention
/// Phase 2 (Sequential): Apply updates to spatial hash (batch processing)
/// 
/// This achieves true parallelization by eliminating the mutex bottleneck.
pub fn update_spatial_hash_parallel(
    mut spatial_hash: ResMut<SpatialHash>,
    mut query: Query<(Entity, &SimPosition, &Collider, &mut OccupiedCell), Without<StaticObstacle>>,
    new_entities: Query<(Entity, &SimPosition, &Collider), Without<OccupiedCell>>,
    mut commands: Commands,
    _sim_config: Res<SimConfig>,
) {
    
    
    // Handle new entities (rare, keep sequential)
    for (entity, pos, collider) in new_entities.iter() {
        let occupied = spatial_hash.insert(entity, pos.0, collider.radius);
        commands.entity(entity).insert(occupied);
    }
    
    // Phase 1: Parallel computation - find entities that need updates
    // Use 16 mutexes (power of 2 for fast modulo via bitwise AND) to reduce contention
    // With 8 CPU cores, this gives ~2 mutexes per thread on average
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    
    const NUM_SHARDS: usize = 16;
    let per_shard_updates: Vec<Mutex<Vec<(Entity, OccupiedCell, OccupiedCell)>>> = 
        (0..NUM_SHARDS).map(|_| Mutex::new(Vec::with_capacity(5000))).collect();
    let counter = AtomicUsize::new(0);
    
    query.par_iter().for_each(|(entity, pos, _collider, occupied_cell)| {
        if let Some((grid_offset, col, row)) = spatial_hash.should_update(pos.0, occupied_cell) {
            let new_occupied = OccupiedCell {
                size_class: occupied_cell.size_class,
                grid_offset,
                col,
                row,
                vec_idx: 0,
            };
            
            // Simple round-robin distribution across shards
            let shard_idx = counter.fetch_add(1, Ordering::Relaxed) & (NUM_SHARDS - 1);
            per_shard_updates[shard_idx].lock().unwrap()
                .push((entity, occupied_cell.clone(), new_occupied));
        }
    });
    
    // Combine all shard results
    let updates: Vec<_> = per_shard_updates.into_iter()
        .flat_map(|mutex| mutex.into_inner().unwrap())
        .collect();
    
    // Phase 2: Sequential apply - update spatial hash and components
    // This is fast because we pre-computed everything
    let mut pending_swaps: Vec<(Entity, usize, usize, usize)> = Vec::new();
    
    for (entity, old_occupied, new_occupied) in updates {
        // Remove from old cell
        if let Some(swapped_entity) = spatial_hash.remove(&old_occupied) {
            pending_swaps.push((swapped_entity, old_occupied.col, old_occupied.row, old_occupied.vec_idx));
        }
        
        // Insert into new cell
        let vec_idx = spatial_hash.insert_into_cell(entity, &new_occupied);
        
        // Update component
        if let Ok((_, _, _, mut occupied_cell)) = query.get_mut(entity) {
            *occupied_cell = OccupiedCell { vec_idx, ..new_occupied };
        }
    }
    
    // Fix swapped entity indices
    for (swapped_entity, col, row, new_idx) in pending_swaps {
        if let Ok((_, _, _, mut swapped_occupied)) = query.get_mut(swapped_entity) {
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
            let min_x = grid_x.saturating_sub(radius_cells);
            let max_x = (grid_x + radius_cells).min(flow_field.width - 1);
            let min_y = grid_y.saturating_sub(radius_cells);
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

// ============================================================================
// Configuration
// ============================================================================

/// Initialize SimConfig from InitialConfig at startup
pub fn init_sim_config_from_initial(
    mut fixed_time: ResMut<Time<Fixed>>,
    mut sim_config: ResMut<SimConfig>,
    mut spatial_hash: ResMut<SpatialHash>,
    initial_config: Option<Res<InitialConfig>>,
) {
    info!("Initializing SimConfig from InitialConfig (lightweight startup init)");
    
    let config = match initial_config {
        Some(cfg) => cfg.clone(),
        None => {
            warn!("InitialConfig not found, using defaults ");
            InitialConfig::default()
        }
    };
    
    // Set fixed timestep
    fixed_time.set_timestep_seconds(1.0 / config.tick_rate);
    
    // Copy all values from InitialConfig to SimConfig
    sim_config.tick_rate = config.tick_rate;
    sim_config.unit_speed = FixedNum::from_num(config.unit_speed);
    sim_config.map_width = FixedNum::from_num(config.map_width);
    sim_config.map_height = FixedNum::from_num(config.map_height);
    sim_config.unit_radius = FixedNum::from_num(config.unit_radius);
    sim_config.collision_push_strength = FixedNum::from_num(config.collision_push_strength);
    sim_config.collision_restitution = FixedNum::from_num(config.collision_restitution);
    sim_config.collision_drag = FixedNum::from_num(config.collision_drag);
    sim_config.collision_iterations = config.collision_iterations;
    sim_config.collision_search_radius_multiplier = FixedNum::from_num(config.collision_search_radius_multiplier);
    sim_config.obstacle_search_range = config.obstacle_search_range;
    sim_config.epsilon = FixedNum::from_num(config.epsilon);
    sim_config.obstacle_push_strength = FixedNum::from_num(config.obstacle_push_strength);
    sim_config.friction = FixedNum::from_num(config.friction);
    sim_config.min_velocity = FixedNum::from_num(config.min_velocity);
    sim_config.braking_force = FixedNum::from_num(config.braking_force);
    sim_config.touch_dist_multiplier = FixedNum::from_num(config.touch_dist_multiplier);
    sim_config.check_dist_multiplier = FixedNum::from_num(config.check_dist_multiplier);
    sim_config.arrival_threshold = FixedNum::from_num(config.arrival_threshold);
    sim_config.max_force = FixedNum::from_num(config.max_force);
    sim_config.steering_force = FixedNum::from_num(config.steering_force);
    sim_config.repulsion_force = FixedNum::from_num(config.repulsion_force);
    sim_config.repulsion_decay = FixedNum::from_num(config.repulsion_decay);
    sim_config.separation_weight = FixedNum::from_num(config.separation_weight);
    sim_config.alignment_weight = FixedNum::from_num(config.alignment_weight);
    sim_config.cohesion_weight = FixedNum::from_num(config.cohesion_weight);
    sim_config.neighbor_radius = FixedNum::from_num(config.neighbor_radius);
    sim_config.separation_radius = FixedNum::from_num(config.separation_radius);
    sim_config.boids_max_neighbors = config.boids_max_neighbors;
    sim_config.black_hole_strength = FixedNum::from_num(config.black_hole_strength);
    sim_config.wind_spot_strength = FixedNum::from_num(config.wind_spot_strength);
    sim_config.force_source_radius = FixedNum::from_num(config.force_source_radius);
    
    // Spatial hash parallel updates
    sim_config.spatial_hash_parallel_updates = config.spatial_hash_parallel_updates;
    sim_config.spatial_hash_regions_per_axis = config.spatial_hash_regions_per_axis;
    
    // Initialize spatial hash with proper configuration
    spatial_hash.resize(
        sim_config.map_width,
        sim_config.map_height,
        &config.spatial_hash_entity_radii,
        config.spatial_hash_radius_to_cell_ratio,
    );
    
    info!("SimConfig initialized with map size: {}x{}", 
          sim_config.map_width.to_num::<f32>(), sim_config.map_height.to_num::<f32>());
    info!("SpatialHash initialized with {} size classes ", spatial_hash.size_classes().len());
}

/// Handle hot-reloadable runtime configuration
pub fn update_sim_from_runtime_config(
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    mut events: MessageReader<AssetEvent<GameConfig>>,
) {
    for event in events.read() {
        if event.is_modified(config_handle.0.id()) || event.is_loaded_with_dependencies(config_handle.0.id()) {
            if let Some(_config) = game_configs.get(&config_handle.0) {
                info!("Runtime config loaded/updated (controls, camera, debug settings)");
                // The config is stored in the asset and accessed when needed by other systems
                // No need to copy values here since systems read from GameConfig directly
            }
        }
    }
}

// ============================================================================
// Performance Tracking
// ============================================================================

/// Log simulation status periodically
pub fn sim_start(
    #[allow(unused_variables)] stats: Res<SimPerformance>,
    #[allow(unused_variables)] tick: Res<SimTick>,
    #[allow(unused_variables)] units_query: Query<Entity, With<crate::game::unit::Unit>>,
    #[allow(unused_variables)] paths_query: Query<&Path>,
) {
    use crate::profile_log;
    
    profile_log!(tick, "[SIM STATUS] Tick: {} | Units: {} | Active Paths: {} | Last sim duration: {:?}", 
          tick.0, units_query.iter().len(), paths_query.iter().len(), stats.last_duration);
}

/// Update simulation performance stats
/// 
/// NOTE: Individual system timing is handled by #[profile] macro.
/// This tracks overall fixed update duration for monitoring.
#[profile(16)]  // Warn if entire simulation tick > 16ms
pub fn sim_end(mut stats: ResMut<SimPerformance>, time: Res<Time<Fixed>>) {
    // Store the actual fixed timestep duration for status reporting
    // This represents the configured tick duration, not the wall-clock time
    stats.last_duration = time.delta();
}

// ... Additional loading/setup systems will be added later if needed
