/// Core simulation systems.
///
/// This module contains systems for:
/// - Tick management
/// - Input processing (commands → pathfinding requests)
/// - Path following (hierarchical navigation)
/// - Performance tracking

#[path = "systems_spatial.rs"]
mod systems_spatial;
#[path = "systems_config.rs"]
mod systems_config;

use bevy::prelude::*;
use crate::game::fixed_math::{FixedVec2, FixedNum};
use crate::game::pathfinding::{Path, PathRequest, HierarchicalGraph, CLUSTER_SIZE};
use peregrine_macros::profile;

use super::components::*;
use super::resources::*;
use super::events::*;
use super::physics::seek;

// Re-export systems from submodules
pub use systems_spatial::{update_spatial_hash, init_flow_field, apply_obstacle_to_flow_field, apply_new_obstacles};
pub use systems_config::{init_sim_config_from_initial, update_sim_from_runtime_config};

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
