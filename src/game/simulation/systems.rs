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
use crate::game::pathfinding::{Path, PathRequest, HierarchicalGraph, CLUSTER_SIZE, world_to_cluster_local, IslandId};
use crate::game::spatial_hash::{SpatialHash, SpatialHashScratch};
use peregrine_macros::profile;

use super::components::*;
use super::resources::*;
use super::events::*;
use super::physics::seek;

// ============================================================================
// Navigation Target Types
// ============================================================================

/// Result of pathfinding query - tells entity where to move next
#[derive(Debug, Clone, Copy)]
enum NavigationTarget {
    /// Move directly toward goal (same region, convex guarantee)
    Direct(FixedVec2),
    /// Move toward inter-cluster portal
    InterClusterPortal(FixedVec2),
    /// Move toward intra-cluster region portal
    IntraClusterPortal(FixedVec2),
    /// Reached destination
    Arrived,
    /// No path exists (unreachable)
    Blocked,
}

// Re-export systems from submodules
pub use systems_spatial::{update_spatial_hash, init_flow_field, apply_obstacle_to_flow_field, apply_new_obstacles, PendingVecIdxUpdates};
pub use systems_config::{init_sim_config_from_initial, update_sim_from_runtime_config, SpatialHashRebuilt};

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
            CollisionState::default(),
            // OccupiedCell added by update_spatial_hash on first frame
        ));
    }
}

// ============================================================================
// Path Following
// ============================================================================

/// Compute where the entity should move next based on hierarchical pathfinding
/// 
/// This function separates pathfinding logic from steering, making the code
/// much more maintainable and testable.
fn compute_navigation_target(
    pos: FixedVec2,
    goal: FixedVec2,
    goal_cluster: (usize, usize),
    goal_island: IslandId,
    threshold_sq: FixedNum,
    graph: &HierarchicalGraph,
    flow_field: &crate::game::structures::FlowField,
) -> NavigationTarget {
    // Check if we've arrived
    let delta = goal - pos;
    if delta.length_squared() < threshold_sq {
        return NavigationTarget::Arrived;
    }
    
    // Determine current location
    let current_grid = match flow_field.world_to_grid(pos) {
        Some(grid) => grid,
        None => return NavigationTarget::Direct(goal), // Off grid - move toward goal
    };
    
    let current_cluster = (current_grid.0 / CLUSTER_SIZE, current_grid.1 / CLUSTER_SIZE);
    let current_region_opt = graph.clusters.get(&current_cluster)
        .and_then(|cluster| {
            world_to_cluster_local(pos, current_cluster, flow_field)
                .and_then(|local_pos| {
                    crate::game::pathfinding::get_region_id(
                        &cluster.regions,
                        cluster.region_count,
                        local_pos
                    )
                })
        });
    
    // CASE 1: Same cluster as goal
    if current_cluster == goal_cluster {
        let cluster = match graph.clusters.get(&current_cluster) {
            Some(c) => c,
            None => return NavigationTarget::Direct(goal), // No cluster data
        };
        
        // Get current region (use cache if available)
        let current_region = current_region_opt.or_else(|| {
            world_to_cluster_local(pos, current_cluster, flow_field)
                .and_then(|local_pos| {
                    crate::game::pathfinding::get_region_id(
                        &cluster.regions,
                        cluster.region_count,
                        local_pos
                    )
                })
        });
        
        // Get goal region
        let goal_region = world_to_cluster_local(goal, current_cluster, flow_field)
            .and_then(|local_goal| {
                crate::game::pathfinding::get_region_id(
                    &cluster.regions,
                    cluster.region_count,
                    local_goal
                )
            });
        
        match (current_region, goal_region) {
            (Some(curr_reg), Some(goal_reg)) if curr_reg == goal_reg => {
                // Same region - direct movement (convexity guarantee)
                if let Some(region_data) = &cluster.regions[curr_reg.0 as usize] {
                    if region_data.is_dangerous {
                        warn_once!("[PATHFINDING] Moving through dangerous region - using direct path");
                    }
                }
                NavigationTarget::Direct(goal)
            }
            (Some(curr_reg), Some(goal_reg)) => {
                // Different regions in same cluster - use local routing
                let next_region_id = cluster.local_routing[curr_reg.0 as usize][goal_reg.0 as usize];
                
                if next_region_id == crate::game::pathfinding::NO_PATH {
                    return NavigationTarget::Direct(goal); // No path - try direct
                }
                
                // Find portal to next region
                if let Some(current_region_data) = &cluster.regions[curr_reg.0 as usize] {
                    if let Some(portal) = current_region_data.portals.iter()
                        .find(|p| p.next_region.0 == next_region_id) {
                        return NavigationTarget::IntraClusterPortal(portal.center);
                    }
                }
                
                NavigationTarget::Direct(goal) // Portal not found
            }
            _ => NavigationTarget::Direct(goal), // Can't determine regions
        }
    } else {
        // CASE 2: Different cluster - use island routing
        let current_cluster_data = match graph.clusters.get(&current_cluster) {
            Some(c) => c,
            None => return NavigationTarget::Direct(goal),
        };
        
        // Determine current island
        let current_island = if let Some(curr_reg) = current_region_opt {
            current_cluster_data.regions[curr_reg.0 as usize]
                .as_ref()
                .map(|r| r.island)
        } else {
            // Not in any region - find nearest region's island
            world_to_cluster_local(pos, current_cluster, flow_field)
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
        
        let current_island = match current_island {
            Some(island) => island,
            None => return NavigationTarget::Direct(goal), // Can't determine island
        };
        
        let current_island_id = crate::game::pathfinding::ClusterIslandId::new(
            current_cluster,
            current_island
        );
        let goal_island_id = crate::game::pathfinding::ClusterIslandId::new(
            goal_cluster,
            goal_island
        );
        
        // Query island routing table
        match graph.get_next_portal_for_island(current_island_id, goal_island_id) {
            Some(next_portal_id) => {
                if let Some(portal) = graph.portals.get(&next_portal_id) {
                    NavigationTarget::InterClusterPortal(portal.world_pos)
                } else {
                    NavigationTarget::Direct(goal) // Portal not found
                }
            }
            None => {
                // No route - unreachable goal
                warn_once!("[PATHFINDING] No route from {:?} to {:?}",
                    current_island_id, goal_island_id);
                NavigationTarget::Blocked
            }
        }
    }
}

/// Follow assigned paths using flow fields and steering
pub fn follow_path(
    mut commands: Commands,
    mut query: Query<(Entity, &SimPosition, &SimVelocity, &mut SimAcceleration, &mut Path)>,
    no_path_query: Query<Entity, (Without<Path>, With<SimPosition>)>,
    sim_config: Res<SimConfig>,
    graph: Res<HierarchicalGraph>,
    map_flow_field: Res<MapFlowField>,
    spatial_hash: Res<SpatialHash>,
    mut scratch: ResMut<SpatialHashScratch>,
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

    for (entity, pos, vel, mut acc, mut path) in query.iter_mut() {
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
                    // Query spatial hash for nearby stopped units
                    spatial_hash.query_radius(pos.0, arrival_radius, Some(entity), &mut scratch);
                    
                    // Count nearby stopped units (units without Path component)
                    let stopped_count = scratch.query_results.iter()
                        .filter(|&&neighbor_entity| no_path_query.contains(neighbor_entity))
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
                // Query navigation system for next target
                let nav_target = compute_navigation_target(
                    pos.0,
                    *goal,
                    *goal_cluster,
                    *goal_island,
                    threshold_sq,
                    &graph,
                    flow_field,
                );
                
                // Act on navigation decision
                match nav_target {
                    NavigationTarget::Direct(target) |
                    NavigationTarget::InterClusterPortal(target) |
                    NavigationTarget::IntraClusterPortal(target) => {
                        seek(pos.0, target, vel.0, &mut acc.0, speed, max_force);
                    }
                    NavigationTarget::Arrived => {
                        commands.entity(entity).remove::<Path>();
                        acc.0 = acc.0 - vel.0 * sim_config.braking_force;
                    }
                    NavigationTarget::Blocked => {
                        commands.entity(entity).remove::<Path>();
                        acc.0 = acc.0 - vel.0 * sim_config.braking_force;
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
