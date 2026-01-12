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
use std::time::Instant;

use super::components::*;
use super::resources::*;
use super::events::*;
use super::physics::seek;

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
    time: Res<Time<Fixed>>,
) {
    let start_time = std::time::Instant::now();
    
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
            OccupiedCells::default(), // Will be populated on first spatial hash update
        ));
    }
    
    let duration = start_time.elapsed();
    let tick = (time.elapsed_secs() * 30.0) as u64;
    if duration.as_millis() > 2 || tick % 100 == 0 {
        info!("[PROCESS_INPUT] {:?}", duration);
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
    time: Res<Time<Fixed>>,
) {
    let start_time = std::time::Instant::now();
    let path_count = query.iter().count();
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
                        early_arrivals += 1;
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
    
    // Log path processing timing
    let duration = start_time.elapsed();
    let tick = (time.elapsed_secs() * 30.0) as u64;
    
    // Always log on every 100th tick or if slow
    if duration.as_millis() > 2 || tick % 100 == 0 {
        info!("[FOLLOW_PATH] {:?} | Paths: {} | Early arrivals: {}", duration, path_count, early_arrivals);
    }
}

// ============================================================================
// Spatial Hash
// ============================================================================

/// Update spatial hash with entity positions
pub fn update_spatial_hash(
    mut spatial_hash: ResMut<SpatialHash>,
    mut query: Query<(Entity, &SimPosition, &Collider, &mut OccupiedCells), Without<StaticObstacle>>,
    new_entities: Query<(Entity, &SimPosition, &Collider), Without<OccupiedCells>>,
    obstacles_query: Query<Entity, With<StaticObstacle>>,
    mut commands: Commands,
    time: Res<Time<Fixed>>,
) {
    let start_time = std::time::Instant::now();
    
    let mut updates = 0;
    let mut unchanged = 0;
    let mut new_count = 0;
    let mut new_obstacles = 0;
    let mut multi_cell_count = 0;
    let mut box_check_skips = 0;  // SC2-style: grid box didn't change
    
    // Handle entities that don't have OccupiedCells yet (first time in spatial hash)
    for (entity, pos, collider) in new_entities.iter() {
        let is_obstacle = obstacles_query.contains(entity);
        
        // Insert into all cells the entity's radius overlaps
        // Now returns Vec<(col, row, vec_index)> for O(1) removal later
        let occupied = if is_obstacle {
            spatial_hash.insert_multi_cell_with_log(entity, pos.0, collider.radius, true)
        } else {
            spatial_hash.insert_multi_cell(entity, pos.0, collider.radius)
        };
        
        if occupied.len() > 1 {
            multi_cell_count += 1;
        }
        
        // Calculate and cache the grid bounding box
        let grid_box = spatial_hash.calculate_grid_box(pos.0, collider.radius);
        
        // Track which cells this entity occupies (now with Vec indices)
        commands.entity(entity).insert(OccupiedCells {
            cells: occupied,
            last_grid_box: grid_box,
        });
        
        new_count += 1;
        
        if is_obstacle {
            new_obstacles += 1;
        }
    }
    
    // Handle static obstacles - they never move, so skip them entirely
    // (They were already inserted in the new_entities pass above)
    
    // Collect index updates needed for swapped entities (avoid double-borrow)
    let mut pending_index_updates: Vec<(Entity, usize, usize, usize)> = Vec::new();
    
    // Handle dynamic entities - SC2 approach: only update if grid box changed
    for (entity, pos, collider, mut occupied_cells) in query.iter_mut() {
        // Calculate what the grid bounding box should be now
        let new_grid_box = spatial_hash.calculate_grid_box(pos.0, collider.radius);
        
        // SC2 Optimization: Compare bounding boxes (4 integer comparisons)
        // If the box didn't change, the occupied cells cannot have changed
        if new_grid_box == occupied_cells.last_grid_box {
            box_check_skips += 1;
            unchanged += 1;
            continue;
        }
        
        // Grid box changed - recalculate which cells the entity should be in
        let new_cell_coords = spatial_hash.calculate_occupied_cells(pos.0, collider.radius);
        
        // SIMPLIFIED: Just remove from all old cells and insert into all new cells
        // No symmetric difference - simpler and avoids sorting overhead
        
        // Remove from ALL old cells using O(1) swap_remove
        let swapped_entities = spatial_hash.remove_multi_cell(&occupied_cells.cells);
        
        // Queue index updates for swapped entities (will apply in second pass)
        for (col, row, swapped_entity) in swapped_entities {
            // Find which index the removed entity was at (now where swapped entity is)
            if let Some(&(_, _, removed_idx)) = occupied_cells.cells.iter()
                .find(|&&(c, r, _)| c == col && r == row) {
                pending_index_updates.push((swapped_entity, col, row, removed_idx));
            }
        }
        
        // Insert into ALL new cells and get back the Vec indices
        let new_cells_with_indices = spatial_hash.insert_into_cells(entity, &new_cell_coords);
        
        // Update cached grid box and cells
        occupied_cells.last_grid_box = new_grid_box;
        occupied_cells.cells = new_cells_with_indices;
        updates += 1;
        
        if occupied_cells.cells.len() > 1 {
            multi_cell_count += 1;
        }
    }
    
    // Second pass: Apply pending index updates to swapped entities
    for (swapped_entity, col, row, new_idx) in pending_index_updates {
        if let Ok((_, _, _, mut swapped_occupied)) = query.get_mut(swapped_entity) {
            // Find this (col, row) in swapped entity's cells and update its vec_idx
            for cell_entry in &mut swapped_occupied.cells {
                if cell_entry.0 == col && cell_entry.1 == row {
                    cell_entry.2 = new_idx;
                    break;
                }
            }
        }
    }
    
    let duration = start_time.elapsed();
    let tick = (time.elapsed_secs() * 30.0) as u64;
    if duration.as_millis() > 2 || tick % 100 == 0 || new_obstacles > 0 {
        let total = new_count + updates + unchanged;
        let box_skip_percent = if total > 0 { (box_check_skips as f32 / total as f32) * 100.0 } else { 0.0 };
        
        info!("[SPATIAL_HASH_UPDATE] {:?} | Entities: {} (new: {} [{} obstacles], updated: {}, unchanged: {}, multi-cell: {})", 
              duration, total, new_count, new_obstacles, updates, unchanged, multi_cell_count);
        info!("  SC2 Grid Box Check: {}/{} skipped ({}%)",
              box_check_skips, total, box_skip_percent as u32);
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
    
    let start_time = std::time::Instant::now();
    info!("apply_new_obstacles: START - Processing {} new obstacles", obstacle_count);
    let flow_field = &mut map_flow_field.0;
    
    for (i, (pos, collider)) in obstacles.iter().enumerate() {
        if i % 10 == 0 && i > 0 {
            info!("  Applied {}/{} obstacles to flow field", i, obstacle_count);
        }
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
    
    let duration = start_time.elapsed();
    info!("apply_new_obstacles: END - Completed processing {} obstacles in {:?}", obstacle_count, duration);
}

// ============================================================================
// Configuration
// ============================================================================

/// Initialize SimConfig from InitialConfig at startup
pub fn init_sim_config_from_initial(
    mut fixed_time: ResMut<Time<Fixed>>,
    mut sim_config: ResMut<SimConfig>,
    initial_config: Option<Res<InitialConfig>>,
) {
    info!("Initializing SimConfig from InitialConfig (lightweight startup init)");
    
    let config = match initial_config {
        Some(cfg) => cfg.clone(),
        None => {
            warn!("InitialConfig not found, using defaults");
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
    
    info!("SimConfig initialized with map size: {}x{}", 
          sim_config.map_width.to_num::<f32>(), sim_config.map_height.to_num::<f32>());
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

/// Mark simulation tick start
pub fn sim_start(
    mut stats: ResMut<SimPerformance>,
    time: Res<Time<Fixed>>,
    units_query: Query<Entity, With<crate::game::unit::Unit>>,
    paths_query: Query<&Path>,
) {
    stats.start_time = Some(Instant::now());
    
    // Log every 5 seconds (100 ticks at 20 Hz)
    let tick = (time.elapsed_secs() * 20.0) as u64;
    if tick % 100 == 0 {
        let unit_count = units_query.iter().count();
        let path_count = paths_query.iter().count();
        info!("[SIM STATUS] Tick: {} | Units: {} | Active Paths: {} | Last sim duration: {:?}", 
              tick, unit_count, path_count, stats.last_duration);
    }
}

/// Mark simulation tick end and check performance
pub fn sim_end(mut stats: ResMut<SimPerformance>) {
    if let Some(start) = stats.start_time {
        stats.last_duration = start.elapsed();
        
        // Performance threshold depends on build mode:
        // - Debug builds are much slower (10-50x), so use a higher threshold
        // - Release builds should target 60fps (16ms) or better
        #[cfg(debug_assertions)]
        const THRESHOLD_MS: u128 = 100; // Debug builds: warn if > 100ms
        
        #[cfg(not(debug_assertions))]
        const THRESHOLD_MS: u128 = 16; // Release builds: warn if > 16ms (60fps)
        
        if stats.last_duration.as_millis() > THRESHOLD_MS {
            warn!("Sim tick took too long: {:?}", stats.last_duration);
        }
    }
}

// ... Additional loading/setup systems will be added later if needed
