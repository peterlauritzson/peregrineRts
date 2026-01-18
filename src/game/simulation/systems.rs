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
use crate::game::pathfinding::{Path, PathRequest, HierarchicalGraph, CLUSTER_SIZE, world_to_cluster_local};
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
        // MEMORY_OK: ECS component insert, not collection growth
        commands.entity(event.entity).insert(SimVelocity(FixedVec2::ZERO));
    }

    // Handle Move Commands
    let mut moves: Vec<&UnitMoveCommand> = move_events.read().collect();
    moves.sort_by_key(|e| e.player_id);
    
    for event in moves {
        if let Ok(_pos) = query.get(event.entity) {
            // Send Path Request instead of setting target directly
            path_requests.write(PathRequest {
                entity: event.entity,
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
            PathCache::default(), // NEW: Add pathfinding cache
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
    mut query: Query<(Entity, &SimPosition, &SimVelocity, &mut SimAcceleration, &mut Path, &CachedNeighbors, &mut PathCache)>,
    no_path_query: Query<Entity, (Without<Path>, With<SimPosition>)>,
    sim_config: Res<SimConfig>,
    graph: Res<HierarchicalGraph>,
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

    for (entity, pos, vel, mut acc, mut path, cache, mut path_cache) in query.iter_mut() {
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
            Path::Hierarchical { goal, goal_cluster, goal_region: _goal_region, goal_island } => {
                // NEW: Region-based hierarchical pathfinding with caching
                // Skip-frame validation: only revalidate region every 4 frames
                
                let current_grid = flow_field.world_to_grid(pos.0);
                if let Some((gx, gy)) = current_grid {
                    // PERFORMANCE OPTIMIZATION: Skip-frame validation
                    // Only revalidate cluster/region every 4 frames (3.75x speedup)
                    let (current_cluster, current_region_opt) = if path_cache.frames_since_validation >= 4 {
                        path_cache.frames_since_validation = 0;
                        
                        // Full validation - check actual cluster and region
                        let cx = gx / CLUSTER_SIZE;
                        let cy = gy / CLUSTER_SIZE;
                        let new_cluster = (cx, cy);
                        
                        let new_region = if let Some(cluster) = graph.clusters.get(&new_cluster) {
                            world_to_cluster_local(pos.0, new_cluster, flow_field)
                                .and_then(|local_pos| {
                                    crate::game::pathfinding::get_region_id(
                                        &cluster.regions,
                                        cluster.region_count,
                                        local_pos
                                    )
                                })
                        } else {
                            None
                        };
                        
                        // Update cache
                        path_cache.cached_cluster = new_cluster;
                        if let Some(region) = new_region {
                            path_cache.cached_region = region;
                        }
                        
                        (new_cluster, new_region)
                    } else {
                        // Use cached values (fast path)
                        path_cache.frames_since_validation += 1;
                        (path_cache.cached_cluster, Some(path_cache.cached_region))
                    };
                    
                    let current_cluster = current_cluster;
                    let current_cluster = current_cluster;
                    
                    if current_cluster == *goal_cluster {
                        // Same cluster - use local routing or direct movement
                        if let Some(cluster) = graph.clusters.get(&current_cluster) {
                            // Use cached region if available, otherwise recompute
                            let current_region = current_region_opt.or_else(|| {
                                world_to_cluster_local(pos.0, current_cluster, flow_field)
                                    .and_then(|local_pos| {
                                        crate::game::pathfinding::get_region_id(
                                            &cluster.regions,
                                            cluster.region_count,
                                            local_pos
                                        )
                                    })
                            });
                            
                            let goal_region = world_to_cluster_local(*goal, current_cluster, flow_field)
                                .and_then(|local_goal| {
                                    crate::game::pathfinding::get_region_id(
                                        &cluster.regions,
                                        cluster.region_count,
                                        local_goal
                                    )
                                });
                            
                            match (current_region, goal_region) {
                                (Some(curr_reg), Some(goal_reg)) if curr_reg == goal_reg => {
                                    // Same region - check if region is dangerous
                                    if let Some(region_data) = &cluster.regions[curr_reg.0 as usize] {
                                        if region_data.is_dangerous {
                                            // TODO: IMPROVE - Add local A* for dangerous regions
                                            // For now, use direct movement and rely on collision avoidance
                                            warn_once!("[PATHFINDING] Moving through dangerous region - using direct path. \
                                                Consider implementing local A* for non-convex regions.");
                                        }
                                    }
                                    
                                    // Direct movement (convexity guarantee, or best effort for dangerous regions)
                                    let delta = *goal - pos.0;
                                    if delta.length_squared() < threshold_sq {
                                        commands.entity(entity).remove::<Path>();
                                        acc.0 = acc.0 - vel.0 * sim_config.braking_force;
                                        continue;
                                    }
                                    seek(pos.0, *goal, vel.0, &mut acc.0, speed, max_force);
                                }
                                (Some(curr_reg), Some(goal_reg)) => {
                                    // Different region in same cluster - use local routing
                                    let next_region_id = cluster.local_routing[curr_reg.0 as usize][goal_reg.0 as usize];
                                    
                                    if next_region_id == crate::game::pathfinding::NO_PATH {
                                        // No path - fallback to direct movement
                                        seek(pos.0, *goal, vel.0, &mut acc.0, speed, max_force);
                                    } else {
                                        // Find portal to next region
                                        if let Some(current_region_data) = &cluster.regions[curr_reg.0 as usize] {
                                            // Find portal to next_region_id
                                            let target = if let Some(portal) = current_region_data.portals.iter()
                                                .find(|p| p.next_region.0 == next_region_id) {
                                                // Move toward portal center
                                                portal.center
                                            } else {
                                                // No portal found, move toward goal directly
                                                *goal
                                            };
                                            
                                            seek(pos.0, target, vel.0, &mut acc.0, speed, max_force);
                                        } else {
                                            seek(pos.0, *goal, vel.0, &mut acc.0, speed, max_force);
                                        }
                                    }
                                }
                                _ => {
                                    // Can't determine region - fallback to direct movement
                                    seek(pos.0, *goal, vel.0, &mut acc.0, speed, max_force);
                                }
                            }
                        } else {
                            // No cluster data - fallback
                            seek(pos.0, *goal, vel.0, &mut acc.0, speed, max_force);
                        }
                    } else {
                        // Different cluster - use island-aware routing
                        if let Some(current_cluster_data) = graph.clusters.get(&current_cluster) {
                            // Use cached region if available
                            let current_region = current_region_opt.or_else(|| {
                                world_to_cluster_local(pos.0, current_cluster, flow_field)
                                    .and_then(|local_pos| {
                                        crate::game::pathfinding::get_region_id(
                                            &current_cluster_data.regions,
                                            current_cluster_data.region_count,
                                            local_pos
                                        )
                                    })
                            });
                            
                            // Determine current island - if region lookup fails, find nearest region
                            let current_island = if let Some(curr_reg) = current_region {
                                // In a region - use its island
                                current_cluster_data.regions[curr_reg.0 as usize]
                                    .as_ref()
                                    .map(|r| r.island)
                            } else {
                                // Not in any region - find nearest region's island
                                world_to_cluster_local(pos.0, current_cluster, flow_field)
                                    .and_then(|local_pos| {
                                        let mut nearest_island = None;
                                        let mut min_dist_sq = f32::MAX;
                                        
                                        for region_opt in &current_cluster_data.regions[0..current_cluster_data.region_count] {
                                            if let Some(region) = region_opt {
                                                let center = region.bounds.center();
                                                let dx = center.x - local_pos.x;
                                                let dy = center.y - local_pos.y;
                                                let dist_sq = dx * dx + dy * dy;
                                                
                                                if dist_sq < FixedNum::from_num(min_dist_sq) {
                                                    min_dist_sq = dist_sq.to_num::<f32>();
                                                    nearest_island = Some(region.island);
                                                }
                                            }
                                        }
                                        nearest_island
                                    })
                            };
                            
                            if let Some(current_island) = current_island {
                                let current_island_id = crate::game::pathfinding::ClusterIslandId::new(
                                    current_cluster,
                                    current_island
                                );
                                let goal_island_id = crate::game::pathfinding::ClusterIslandId::new(
                                    *goal_cluster,
                                    *goal_island
                                );
                                
                                // Debug logging for first few frames
                                static LOGGED_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                                if LOGGED_COUNT.load(std::sync::atomic::Ordering::Relaxed) < 10 {
                                    info!("[NAV] Unit at cluster {:?} island {} -> goal cluster {:?} island {}",
                                        current_cluster, current_island.0, goal_cluster, goal_island.0);
                                    LOGGED_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                }
                                
                                // Lookup next portal from island routing table
                                if let Some(next_portal_id) = graph.get_next_portal_for_island(
                                    current_island_id,
                                    goal_island_id
                                ) {

                                    if let Some(portal) = graph.portals.get(&next_portal_id) {
                                        let portal_pos = flow_field.grid_to_world(portal.node.x, portal.node.y);
                                        
                                        // Debug: Log which portal was chosen
                                        if LOGGED_COUNT.load(std::sync::atomic::Ordering::Relaxed) < 10 {
                                            info!("  -> Portal {} at ({}, {})", next_portal_id, portal.node.x, portal.node.y);
                                        }
                                        
                                        seek(pos.0, portal_pos, vel.0, &mut acc.0, speed, max_force);
                                    } else {
                                        // Portal not found - move toward goal
                                        seek(pos.0, *goal, vel.0, &mut acc.0, speed, max_force);
                                    }
                                } else {
                                    // No route found - invalidate path
                                    if LOGGED_COUNT.load(std::sync::atomic::Ordering::Relaxed) < 10 {
                                        warn!("  -> NO ROUTE FOUND! Invalidating path.");
                                    }
                                    warn_once!("[PATHFINDING] No route from {:?} to {:?} - path invalidated. \
                                        This usually means routing table is incomplete or islands are unreachable.",
                                        current_island_id, goal_island_id);
                                    
                                    // TODO: IMPROVE - Emit PathFailed event for higher-level systems
                                    commands.entity(entity).remove::<Path>();
                                    acc.0 = acc.0 - vel.0 * sim_config.braking_force;
                                    continue;
                                }
                            } else {
                                // Can't determine current island - move toward goal
                                warn_once!("[PATHFINDING] Can't determine current island for cluster {:?} - falling back to direct movement", current_cluster);
                                seek(pos.0, *goal, vel.0, &mut acc.0, speed, max_force);
                            }
                        } else {
                            // No cluster data - fallback to direct movement
                            seek(pos.0, *goal, vel.0, &mut acc.0, speed, max_force);
                        }
                    }
                } else {
                    // Off grid - move toward goal
                    seek(pos.0, *goal, vel.0, &mut acc.0, speed, max_force);
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
