/// Path following and navigation systems.
///
/// This module contains the systems that actually move units along their paths,
/// separated from the path request processing in systems.rs.

use bevy::prelude::*;
use crate::game::fixed_math::{FixedVec2, FixedNum};
use crate::game::simulation::components::{SimPosition, SimVelocity, SimAcceleration};
use crate::game::simulation::resources::{SimConfig, MapFlowField};
use crate::game::simulation::physics::seek;
use crate::game::spatial_hash::{SpatialHash, SpatialHashScratch};
use super::{Path, HierarchicalGraph, CLUSTER_SIZE, IslandId, ClusterId, RegionId, point_in_cluster, point_in_region};

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

// ============================================================================
// Navigation Logic
// ============================================================================

/// Update cached location (cluster and region) for an entity.
/// This is called after recomputing navigation to keep cache fresh.
/// Note: Island lookup is now O(1) via HashMap, no caching needed!
fn update_location_cache(
    pos: FixedVec2,
    flow_field: &crate::game::structures::FlowField,
    graph: &HierarchicalGraph,
    current_cluster: &mut Option<ClusterId>,
    current_region: &mut Option<RegionId>,
) {
    // Determine current cluster
    let grid = match flow_field.world_to_grid(pos) {
        Some(g) => g,
        None => {
            *current_cluster = None;
            *current_region = None;
            return;
        }
    };
    
    let cluster = ClusterId::new(grid.0 / CLUSTER_SIZE, grid.1 / CLUSTER_SIZE);
    *current_cluster = Some(cluster);
    
    // Determine current region within cluster (O(1) HashMap lookup)
    let (cx, cy) = cluster.as_tuple();
    *current_region = graph.get_cluster(cx, cy)
        .and_then(|cluster_data| {
            crate::game::pathfinding::get_region_id_by_world_pos(cluster_data, pos)
        });
}

/// Compute where the entity should move next based on hierarchical pathfinding
/// 
/// This function separates pathfinding logic from steering, making the code
/// much more maintainable and testable.
fn compute_navigation_target(
    pos: FixedVec2,
    goal: FixedVec2,
    goal_cluster: ClusterId,
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
    
    let current_cluster = ClusterId::new(current_grid.0 / CLUSTER_SIZE, current_grid.1 / CLUSTER_SIZE);
    let (ccx, ccy) = current_cluster.as_tuple();
    let current_region_opt = graph.get_cluster(ccx, ccy)
        .and_then(|cluster| {
            // PERF: O(1) HashMap lookup using world coords directly (no conversion)
            crate::game::pathfinding::get_region_id_by_world_pos(cluster, pos)
        });
    
    // CASE 1: Same cluster as goal
    if current_cluster == goal_cluster {
        let (cx, cy) = current_cluster.as_tuple();
        let cluster = match graph.get_cluster(cx, cy) {
            Some(c) => c,
            None => return NavigationTarget::Direct(goal), // No cluster data
        };
        
        // Get current region (use cache if available)
        let current_region = current_region_opt.or_else(|| {
            // PERF: O(1) HashMap lookup
            crate::game::pathfinding::get_region_id_by_world_pos(cluster, pos)
        });
        
        // Get goal region
        let goal_region = crate::game::pathfinding::get_region_id_by_world_pos(cluster, goal);
        
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
                    // No intra-cluster path - check if different islands
                    // (if so, we can use inter-island routing which may go through other clusters)
                    let curr_island = cluster.regions[curr_reg.0 as usize].as_ref().map(|r| r.island);
                    let goal_island_region = cluster.regions[goal_reg.0 as usize].as_ref().map(|r| r.island);
                    
                    if let (Some(ci), Some(gi)) = (curr_island, goal_island_region) {
                        if ci != gi {
                            // Different islands in same cluster - use island routing
                            let current_island_id = crate::game::pathfinding::ClusterIslandId::new(current_cluster.as_tuple(), ci);
                            let goal_island_id_same_cluster = crate::game::pathfinding::ClusterIslandId::new(current_cluster.as_tuple(), gi);
                            
                            if let Some(next_portal_id) = graph.get_next_portal_for_island(current_island_id, goal_island_id_same_cluster) {
                                if let Some(portal) = graph.portals.get(next_portal_id) {
                                    return NavigationTarget::InterClusterPortal(portal.world_pos);
                                }
                            }
                        }
                    }
                    
                    return NavigationTarget::Direct(goal); // Still no path - try direct
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
        let (cx, cy) = current_cluster.as_tuple();
        let current_cluster_data = match graph.get_cluster(cx, cy) {
            Some(c) => c,
            None => return NavigationTarget::Direct(goal),
        };
        
        // Determine current island
        let current_island = if let Some(curr_reg) = current_region_opt {
            // In a region - use its island
            current_cluster_data.regions[curr_reg.0 as usize]
                .as_ref()
                .map(|r| r.island)
        } else {
            // PERF: Not in any region - use O(1) HashMap lookup (no searching!)
            crate::game::pathfinding::get_island_id_by_world_pos(current_cluster_data, pos)
        };
        
        let current_island = match current_island {
            Some(island) => island,
            None => return NavigationTarget::Direct(goal), // Can't determine island
        };
        
        let current_island_id = crate::game::pathfinding::ClusterIslandId::new(
            current_cluster.as_tuple(),
            current_island
        );
        let goal_island_id = crate::game::pathfinding::ClusterIslandId::new(
            goal_cluster.as_tuple(),
            goal_island
        );
        
        // Query island routing table
        match graph.get_next_portal_for_island(current_island_id, goal_island_id) {
            Some(next_portal_id) => {
                if let Some(portal) = graph.portals.get(next_portal_id) {
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

/// Follow assigned paths using flow fields and steering (OLD IMPLEMENTATION - PRESERVED FOR REFERENCE)
#[allow(dead_code)]
pub fn OLD_follow_path(
    mut query: Query<(&SimPosition, &SimVelocity, &mut SimAcceleration, &mut Path)>,
    _no_path_query: Query<Entity, (Without<Path>, With<SimPosition>)>,
    sim_config: Res<SimConfig>,
    graph: Res<HierarchicalGraph>,
    map_flow_field: Res<MapFlowField>,
    _spatial_hash: Res<SpatialHash>,
    _scratch: ResMut<SpatialHashScratch>,
) {
    
    let speed = sim_config.unit_speed;
    let max_force = sim_config.steering_force;
    let dt = FixedNum::ONE / FixedNum::from_num(sim_config.tick_rate);
    let step_dist = speed * dt;
    let threshold = if step_dist > sim_config.arrival_threshold { step_dist } else { sim_config.arrival_threshold };
    let threshold_sq = threshold * threshold;
    
    // Arrival spacing parameters to prevent pile-ups (currently disabled for performance)
    // let arrival_radius = FixedNum::from_num(0.5); 
    // let arrival_radius_sq = arrival_radius * arrival_radius;
    // const CROWDING_THRESHOLD: usize = 50;
    
    let flow_field = &map_flow_field.0;
    #[cfg(feature = "perf_stats")]
    let mut early_arrivals = 0;

    for (pos, vel, mut acc, mut path) in query.iter_mut() {
        // PERF: Skip completed/blocked paths without removal (cleanup system handles it)
        let path_state = match &mut *path {
            super::Path::Active(state) => state,
            super::Path::Completed | super::Path::Blocked => continue,
        };
        
        match path_state {
            super::PathState::Direct(target) => {
                let delta = *target - pos.0;
                let dist_sq = delta.length_squared();
                
                if dist_sq < threshold_sq {
                    // PERF: Mark completed instead of removing component
                    *path = super::Path::Completed;
                    acc.0 = acc.0 - vel.0 * sim_config.braking_force;
                    continue;
                }
                
                // Check for crowding at destination (pile-up prevention)
                // PERF: Disabled entity lookup in hot loop - too expensive for marginal benefit
                // TODO: Re-enable with cached neighbor list if needed
                /*
                if dist_sq < arrival_radius_sq {
                    spatial_hash.query_radius(pos.0, arrival_radius, None, &mut scratch);
                    let stopped_count = scratch.query_results.iter()
                        .filter(|&&neighbor_entity| no_path_query.contains(neighbor_entity))
                        .count();
                    
                    if stopped_count > CROWDING_THRESHOLD {
                        *path = super::Path::Completed;
                        acc.0 = acc.0 - vel.0 * sim_config.braking_force;
                        continue;
                    }
                }
                */
                
                seek(pos.0, *target, vel.0, &mut acc.0, speed, max_force);
            },
            super::PathState::LocalAStar { waypoints, current_index } => {
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
                         // PERF: Mark completed instead of removing component
                         *path = super::Path::Completed;
                     }
                     continue;
                }
                seek(pos.0, target, vel.0, &mut acc.0, speed, max_force);
            },
            super::PathState::Hierarchical { 
                goal, 
                goal_cluster, 
                goal_region: _goal_region, 
                goal_island,
                current_cluster,
                current_region,
                next_expected_cluster,
                next_expected_region,
                current_target,
                is_inter_cluster_target,
            } => {
                // PERF OPTIMIZATION: Use cached navigation state with fast invalidation checks
                
                // Step 1: Check if we need to recompute target (arrival or cache miss)
                let needs_recompute = if let Some(target) = current_target {
                    // Check if we've arrived at current waypoint
                    let delta = *target - pos.0;
                    delta.length_squared() < threshold_sq
                } else {
                    // First frame - no cached target
                    true
                };
                
                if !needs_recompute {
                    // Step 2: Fast path - validate we're still in current region (O(1) check)
                    if let (Some(cluster), Some(region)) = (*current_cluster, *current_region) {
                        if point_in_region(pos.0, cluster, region, &graph) {
                            // Still in same region - continue toward cached target
                            if let Some(target) = current_target {
                                seek(pos.0, *target, vel.0, &mut acc.0, speed, max_force);
                                continue;
                            }
                        }
                    }
                    
                    // Step 3: Medium path - check if we transitioned to next expected region
                    if let (Some(next_cluster), Some(next_region)) = (*next_expected_cluster, *next_expected_region) {
                        if point_in_region(pos.0, next_cluster, next_region, &graph) {
                            // Transitioned as expected - update current to next
                            *current_cluster = Some(next_cluster);
                            *current_region = Some(next_region);
                            // Target is still valid, continue
                            if let Some(target) = current_target {
                                seek(pos.0, *target, vel.0, &mut acc.0, speed, max_force);
                                continue;
                            }
                        }
                    }
                    
                    // Step 4: Slower path - check if we're still in current cluster
                    if let Some(cluster) = *current_cluster {
                        if point_in_cluster(pos.0, cluster, &graph, flow_field) {
                            // Still in cluster but different region - recompute target below
                            // (will update current_region automatically)
                        } else {
                            // Left current cluster - invalidate all cached state
                            *current_cluster = None;
                            *current_region = None;
                            *next_expected_cluster = None;
                            *next_expected_region = None;
                        }
                    }
                    
                    // If we're here and still have a valid target without recompute flag,
                    // continue toward it (handles edge cases)
                    if let Some(target) = current_target {
                        seek(pos.0, *target, vel.0, &mut acc.0, speed, max_force);
                        continue;
                    }
                }
                
                // Step 5: Recompute navigation target (cache miss or arrived at waypoint)
                let nav_target = compute_navigation_target(
                    pos.0,
                    *goal,
                    *goal_cluster,
                    *goal_island,
                    threshold_sq,
                    &graph,
                    flow_field,
                );
                
                // Step 6: Update cached state based on navigation result
                match nav_target {
                    NavigationTarget::Direct(target) => {
                        *current_target = Some(target);
                        *is_inter_cluster_target = false;
                        // Update current location cache
                        update_location_cache(pos.0, flow_field, &graph, current_cluster, current_region);
                        // No next expected - moving directly to goal
                        *next_expected_cluster = None;
                        *next_expected_region = None;
                        
                        seek(pos.0, target, vel.0, &mut acc.0, speed, max_force);
                    }
                    NavigationTarget::InterClusterPortal(target) => {
                        *current_target = Some(target);
                        *is_inter_cluster_target = true;
                        update_location_cache(pos.0, flow_field, &graph, current_cluster, current_region);
                        // TODO: Could predict next cluster/region based on portal direction
                        *next_expected_cluster = None;
                        *next_expected_region = None;
                        
                        seek(pos.0, target, vel.0, &mut acc.0, speed, max_force);
                    }
                    NavigationTarget::IntraClusterPortal(target) => {
                        *current_target = Some(target);
                        *is_inter_cluster_target = false;
                        update_location_cache(pos.0, flow_field, &graph, current_cluster, current_region);
                        // TODO: Could predict next region from portal data
                        *next_expected_cluster = *current_cluster; // Same cluster
                        *next_expected_region = None;
                        
                        seek(pos.0, target, vel.0, &mut acc.0, speed, max_force);
                    }
                    NavigationTarget::Arrived => {
                        // PERF: Mark completed instead of removing component
                        *path = super::Path::Completed;
                        acc.0 = acc.0 - vel.0 * sim_config.braking_force;
                    }
                    NavigationTarget::Blocked => {
                        // PERF: Mark blocked instead of removing component
                        *path = super::Path::Blocked;
                        acc.0 = acc.0 - vel.0 * sim_config.braking_force;
                    }
                }
            }
        }
    }
}

/// Follow assigned paths using hierarchical navigation with O(1) lookups
/// 
/// NEW: Uses NavigationLookup and NavigationRouting for precomputed pathfinding
/// Simple flow:
/// 1. Look up current cluster/region/island (O(1) via NavigationLookup)
/// 2. Use cached GoalNavCell (precomputed during path request)
/// 3. Determine navigation strategy:
///    - Same region → Direct movement
///    - Same cluster, different region → Region routing
///    - Different cluster → Island routing to find portal
pub fn follow_path(
    mut query: Query<(&SimPosition, &SimVelocity, &mut SimAcceleration, &mut Path, &super::types::GoalNavCell)>,
    sim_config: Res<SimConfig>,
    nav_lookup: Res<crate::game::pathfinding::NavigationLookup>,
    nav_routing: Res<crate::game::pathfinding::NavigationRouting>,
    map_flow_field: Res<MapFlowField>,
    graph: Res<HierarchicalGraph>,
) {
    use crate::game::fixed_math::FixedNum;
    use crate::game::simulation::physics::seek;
    use super::types::LocalRegionId;
    
    let speed = sim_config.unit_speed;
    let max_force = sim_config.steering_force;
    let dt = FixedNum::ONE / FixedNum::from_num(sim_config.tick_rate);
    let step_dist = speed * dt;
    let threshold = if step_dist > sim_config.arrival_threshold { step_dist } else { sim_config.arrival_threshold };
    let threshold_sq = threshold * threshold;
    
    for (pos, vel, mut acc, mut path, goal_nav_cell) in query.iter_mut() {
        let Path::Active(ref mut state) = *path else {
            continue; // Skip completed/blocked paths
        };
        
        match state {
            super::PathState::Hierarchical { 
                goal, 
                goal_cluster: _,
                goal_region: _,
                goal_island: _,
                current_cluster: _,
                current_region: _,
                next_expected_cluster: _,
                next_expected_region: _,
                current_target: _,
                is_inter_cluster_target: _,
            } => {
                // Convert current position to grid coordinates (only 1 conversion needed!)
                let Some((grid_x, grid_y)) = map_flow_field.0.world_to_grid(pos.0) else {
                    warn!("Unit at {:?} is out of bounds!", pos.0);
                    *path = super::Path::Blocked;
                    continue;
                };
                
                // O(1) lookup current navigation state
                let Some(current_nav) = nav_lookup.lookup(grid_x, grid_y) else {
                    warn!("Unit at {:?} is out of bounds!", pos.0);
                    *path = super::Path::Blocked;
                    continue;
                };
                
                // Use precomputed goal navigation cell (cached during path request)
                let goal_nav = goal_nav_cell.0;
                
                // CASE 1: Same region - direct movement to goal
                if current_nav.region_idx == goal_nav.region_idx {
                    let delta = *goal - pos.0;
                    let dist_sq = delta.length_squared();
                    
                    // Arrival check
                    if dist_sq < threshold_sq {
                        *path = super::Path::Completed;
                        acc.0 = acc.0 - vel.0 * sim_config.braking_force;
                        continue;
                    }
                    
                    // Steer directly toward goal (region is convex, guaranteed clear path)
                    seek(pos.0, *goal, vel.0, &mut acc.0, speed, max_force);
                    continue;
                }
                
                // CASE 2: Different region, same cluster - use region routing
                if current_nav.cluster_idx == goal_nav.cluster_idx {
                    // Get cluster data to extract local region IDs
                    let cluster = nav_lookup.arenas.get_cluster(current_nav.cluster_idx)
                        .expect("Current cluster must exist");
                    
                    // Find local region IDs (need to reverse-lookup from arena indices)
                    // Arena index: cluster_idx * MAX_REGIONS + local_region_id
                    let current_local_region = LocalRegionId(
                        (current_nav.region_idx.0 as usize % super::types::MAX_REGIONS) as u8
                    );
                    let goal_local_region = LocalRegionId(
                        (goal_nav.region_idx.0 as usize % super::types::MAX_REGIONS) as u8
                    );
                    
                    // O(1) routing lookup - which region to move toward next
                    let Some(next_region) = nav_routing.region_routing.get_next_region(
                        current_nav.cluster_idx,
                        goal_nav.cluster_idx,
                        current_local_region,
                        goal_local_region,
                    ) else {
                        warn!("No path from region {:?} to {:?} (different islands?)", 
                              current_local_region, goal_local_region);
                        *path = super::Path::Blocked;
                        continue;
                    };
                    
                    // If next region is the goal region, move to goal
                    if next_region == goal_local_region {
                        let delta = *goal - pos.0;
                        let dist_sq = delta.length_squared();
                        
                        if dist_sq < threshold_sq {
                            *path = super::Path::Completed;
                            acc.0 = acc.0 - vel.0 * sim_config.braking_force;
                        } else {
                            seek(pos.0, *goal, vel.0, &mut acc.0, speed, max_force);
                        }
                        continue;
                    }
                    
                    // Otherwise, move toward the next region's center
                    if let Some(next_region_data) = &cluster.regions[next_region.0 as usize] {
                        let region_center = next_region_data.bounds.center();
                        seek(pos.0, region_center, vel.0, &mut acc.0, speed, max_force);
                    } else {
                        warn!("Next region {:?} doesn't exist in cluster!", next_region);
                        *path = super::Path::Blocked;
                    }
                    continue;
                }
                
                // CASE 3: Different cluster - use island routing to find portal
                let Some(next_portal_id) = nav_routing.island_routing.find_next_portal(
                    current_nav.island_idx,
                    goal_nav.island_idx,
                ) else {
                    warn!("No path from island {:?} to island {:?}", 
                          current_nav.island_idx, goal_nav.island_idx);
                    *path = super::Path::Blocked;
                    continue;
                };
                
                // Get the portal and move toward it
                if let Some(portal) = graph.portals.get(next_portal_id) {
                    seek(pos.0, portal.world_pos, vel.0, &mut acc.0, speed, max_force);
                } else {
                    warn!("Portal {} doesn't exist!", next_portal_id);
                    *path = super::Path::Blocked;
                }
            }
            
            super::PathState::Direct(target) => {
                // Simple direct movement (no obstacles)
                let delta = *target - pos.0;
                let dist_sq = delta.length_squared();
                
                if dist_sq < threshold_sq {
                    *path = super::Path::Completed;
                    acc.0 = acc.0 - vel.0 * sim_config.braking_force;
                } else {
                    seek(pos.0, *target, vel.0, &mut acc.0, speed, max_force);
                }
            }
            
            super::PathState::LocalAStar { waypoints, current_index } => {
                // Follow waypoint list (for complex local navigation)
                if *current_index >= waypoints.len() {
                    *path = super::Path::Completed;
                    acc.0 = acc.0 - vel.0 * sim_config.braking_force;
                    return;
                }
                
                let target = waypoints[*current_index];
                let delta = target - pos.0;
                let dist_sq = delta.length_squared();
                
                if dist_sq < threshold_sq {
                    *current_index += 1;
                    if *current_index >= waypoints.len() {
                        *path = super::Path::Completed;
                        acc.0 = acc.0 - vel.0 * sim_config.braking_force;
                    }
                } else {
                    seek(pos.0, target, vel.0, &mut acc.0, speed, max_force);
                }
            }
        }
    }
}

/// Cleanup system - removes completed/blocked paths periodically to prevent query bloat
/// Runs less frequently than follow_path to avoid structural changes in hot loop
pub fn cleanup_completed_paths(
    mut commands: Commands,
    query: Query<(Entity, &Path)>,
) {
    for (entity, path) in query.iter() {
        if matches!(path, Path::Completed | Path::Blocked) {
            commands.entity(entity).remove::<Path>();
        }
    }
}
