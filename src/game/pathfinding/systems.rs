use bevy::prelude::*;
use crate::game::simulation::MapFlowField;
use super::types::{PathRequest, CLUSTER_SIZE, IslandId};
use super::graph::HierarchicalGraph;
use super::world_to_cluster_local;
use super::cluster::Cluster;
use crate::game::fixed_math::{FixedVec2, FixedNum};

/// Snap a position to the nearest walkable tile
/// Returns None if no walkable tile found within search radius
fn snap_to_walkable(
    pos: FixedVec2, 
    flow_field: &crate::game::structures::FlowField,
    max_radius: f32,
) -> Option<FixedVec2> {
    // Check if already walkable
    if let Some((gx, gy)) = flow_field.world_to_grid(pos) {
        let idx = flow_field.get_index(gx, gy);
        if idx < flow_field.cost_field.len() && flow_field.cost_field[idx] < 255 {
            return Some(pos);
        }
    }
    
    // Search in expanding radius
    let search_steps = (max_radius as usize).max(1);
    for radius in 1..=search_steps {
        // Check 8 directions
        for angle_idx in 0..8 {
            let angle = (angle_idx as f32) * std::f32::consts::PI / 4.0;
            let offset_x = angle.cos() * radius as f32;
            let offset_y = angle.sin() * radius as f32;
            
            let test_pos = FixedVec2::new(
                pos.x + FixedNum::from_num(offset_x),
                pos.y + FixedNum::from_num(offset_y)
            );
            
            if let Some((gx, gy)) = flow_field.world_to_grid(test_pos) {
                let idx = flow_field.get_index(gx, gy);
                if idx < flow_field.cost_field.len() && flow_field.cost_field[idx] < 255 {
                    return Some(test_pos);
                }
            }
        }
    }
    
    None
}

/// Find the nearest region's island when a position is not directly in any region
fn find_nearest_island(cluster: &Cluster, local_pos: FixedVec2) -> IslandId {
    let mut nearest_island = IslandId(0);
    let mut min_dist_sq = f32::MAX;
    
    for region_opt in &cluster.regions[0..cluster.region_count] {
        if let Some(region) = region_opt {
            let center = region.bounds.center();
            let dx = center.x - local_pos.x;
            let dy = center.y - local_pos.y;
            let dist_sq = dx * dx + dy * dy;
            
            if dist_sq < FixedNum::from_num(min_dist_sq) {
                min_dist_sq = dist_sq.to_num::<f32>();
                nearest_island = region.island;
            }
        }
    }
    
    nearest_island
}

#[allow(dead_code)]
pub fn OLD_process_path_requests(
    mut path_requests: MessageReader<PathRequest>,
    mut commands: Commands,
    map_flow_field: Res<MapFlowField>,
    graph: Res<HierarchicalGraph>,
) {
    if path_requests.is_empty() {
        return;
    }

    let flow_field = &map_flow_field.0;
    if flow_field.width == 0 {
        warn!("Flow field empty");
        return;
    }
    if !graph.initialized {
        warn!("Graph not initialized");
        return;
    }

    // Process each path request with proper validation
    // Note: Commands already batches operations internally - no need for intermediate Vec
    for request in path_requests.read() {
        // STEP 1: Snap goal to walkable tile
        let walkable_goal = match snap_to_walkable(request.goal, flow_field, 10.0) {
            Some(pos) => pos,
            None => {
                warn!("Path request for entity {:?} rejected: goal {:?} is not walkable and no walkable tile nearby", 
                    request.entity, request.goal);
                continue; // Skip this request
            }
        };
        
        let goal_node_opt = flow_field.world_to_grid(walkable_goal);

        if let Some(goal_node) = goal_node_opt {
            let goal_cluster = (goal_node.0 / CLUSTER_SIZE, goal_node.1 / CLUSTER_SIZE);
            
            // STEP 2: Determine which island and region the goal is in
            let (cx, cy) = goal_cluster;
            let (goal_island, goal_region) = if let Some(cluster) = graph.get_cluster(cx, cy) {
                // PERF: Use O(1) HashMap lookup with world coordinates directly (no conversion needed)
                let region_id = crate::game::pathfinding::get_region_id_by_world_pos(cluster, walkable_goal);
                
                if let Some(region_id) = region_id {
                    // Goal is in a region - use its island
                    let island = cluster.regions[region_id.0 as usize]
                        .as_ref()
                        .map(|region| region.island)
                        .unwrap_or_else(|| {
                            // Region exists but is None - find nearest
                            // Only need local coords for fallback case
                            let local_goal = world_to_cluster_local(walkable_goal, goal_cluster, flow_field)
                                .unwrap_or_else(|| FixedVec2::ZERO);
                            find_nearest_island(&cluster, local_goal)
                        });
                    (island, Some(region_id))
                } else {
                    // Goal not in any region - find nearest region's island
                    let local_goal = world_to_cluster_local(walkable_goal, goal_cluster, flow_field)
                        .unwrap_or_else(|| FixedVec2::ZERO);
                    let island = find_nearest_island(&cluster, local_goal);
                    (island, None)
                }
            } else {
                (IslandId(0), None)
            };
            
            // STEP 3: Validate reachability (optional but recommended)
            // We'll skip this for now since units might be spawning, but log a warning
            // In production, you'd check:
            // if graph.get_next_portal_for_island(start_island_id, goal_island_id).is_none() { ... }
            
            // Debug logging for first few requests
            static LOGGED_REQUESTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            if LOGGED_REQUESTS.load(std::sync::atomic::Ordering::Relaxed) < 5 {
                info!("[PATH REQUEST] Entity {:?} -> goal cluster {:?}, island {}, region {:?} (pos: {:?})", 
                    request.entity, goal_cluster, goal_island.0, goal_region, walkable_goal);
                LOGGED_REQUESTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            
            // Insert Path component directly - Commands batches internally
            commands.entity(request.entity).insert(super::types::Path::Active(super::types::PathState::Hierarchical {
                goal: walkable_goal,  // Use snapped position
                goal_cluster: super::types::ClusterId::new(goal_cluster.0, goal_cluster.1),
                goal_region,  // Cache goal region
                goal_island,
                // PERF: Navigation state initialized to None - will be computed on first frame
                current_cluster: None,
                current_region: None,
                next_expected_cluster: None,
                next_expected_region: None,
                current_target: None,
                is_inter_cluster_target: false,
            }));
        } else {
            warn!("Path request for entity {:?} rejected: goal {:?} is outside grid bounds", 
                request.entity, walkable_goal);
        }
    }
}

/// Process path requests and assign paths to entities (NEW IMPLEMENTATION)
pub fn process_path_requests(
    mut path_requests: MessageReader<PathRequest>,
    mut commands: Commands,
    map_flow_field: Res<MapFlowField>,
    _graph: Res<HierarchicalGraph>,  // Kept for now, may be needed for validation later
    nav_lookup: Res<super::navigation_lookup::NavigationLookup>,
) {
    if path_requests.is_empty() {
        return;
    }

    let walkability_map = &map_flow_field.0;
    if walkability_map.width == 0 {
        return;
    }

    for request in path_requests.read() {
        // Validate goal (for now, just pass through)
        let goal = request.goal;
        
        // Convert world position to grid coordinates
        let Some((grid_x, grid_y)) = walkability_map.world_to_grid(goal) else {
            continue;
        };
        
        // O(1) lookup from NavigationLookup - gets precomputed cluster/region/island indices
        let Some(nav_cell) = nav_lookup.lookup(grid_x, grid_y) else {
            continue;
        };
        
        // Extract IDs directly from arena using precomputed indices
        let goal_cluster = nav_lookup.arenas.get_cluster(nav_cell.cluster_idx)
            .map(|c| super::types::ClusterId::new(c.id.0, c.id.1))
            .unwrap_or_else(|| super::types::ClusterId::new(0, 0));
        
        let goal_region = nav_lookup.arenas.get_region(nav_cell.region_idx).map(|r| r.id);
        
        let goal_island = nav_lookup.arenas.get_island(nav_cell.island_idx)
            .map(|i| i.id)
            .unwrap_or(IslandId(0));
        
        // Insert Path component (deferred automatically by Commands)
        commands.entity(request.entity).insert((
            super::types::Path::Active(super::types::PathState::Hierarchical {
                goal,
                goal_cluster,
                goal_region,
                goal_island,
                current_cluster: None,
                current_region: None,
                next_expected_cluster: None,
                next_expected_region: None,
                current_target: None,
                is_inter_cluster_target: false,
            }),
            super::types::GoalNavCell(nav_cell),  // Cache goal navigation cell
        ));
    }
}

// These helper functions are deprecated - will be replaced by NavigationLookup
#[allow(dead_code)]
fn validate_goal(goal: FixedVec2, walkability_map: &crate::game::structures::FlowField) -> Option<FixedVec2> {
    // For now, just pass through - assume goal is valid
    // Later: implement snap_to_walkable logic
    Some(goal)
}

#[allow(dead_code)]
fn find_cluster(
    graph: &HierarchicalGraph,
    pos: FixedVec2,
    walkability_map: &crate::game::structures::FlowField,
) -> Option<super::types::ClusterId> {
    // Convert world position to grid coordinates
    let (grid_x, grid_y) = walkability_map.world_to_grid(pos)?;
    
    // Calculate cluster from grid position
    let cluster_x = grid_x / CLUSTER_SIZE;
    let cluster_y = grid_y / CLUSTER_SIZE;
    
    // Verify cluster exists in graph
    if graph.get_cluster(cluster_x, cluster_y).is_some() {
        Some(super::types::ClusterId::new(cluster_x, cluster_y))
    } else {
        None
    }
}

#[allow(dead_code)]
fn find_region(
    graph: &HierarchicalGraph,
    cluster: super::types::ClusterId,
    pos: FixedVec2,
) -> Option<super::types::RegionId> {
    let (cx, cy) = cluster.as_tuple();
    let cluster_data = graph.get_cluster(cx, cy)?;
    
    // Use O(1) HashMap lookup with world coordinates
    crate::game::pathfinding::get_region_id_by_world_pos(cluster_data, pos)
}

#[allow(dead_code)]
fn find_island(
    graph: &HierarchicalGraph,
    cluster: super::types::ClusterId,
    pos: FixedVec2,
    region: Option<super::types::RegionId>,
) -> IslandId {
    let (cx, cy) = cluster.as_tuple();
    let Some(cluster_data) = graph.get_cluster(cx, cy) else {
        return IslandId(0); // Default fallback
    };
    
    if let Some(region_id) = region {
        // Goal is in a region - use its island
        cluster_data.regions[region_id.0 as usize]
            .as_ref()
            .map(|r| r.island)
            .unwrap_or(IslandId(0))
    } else {
        // Goal not in any region - find nearest island
        // Use existing helper function (requires flow_field for now, could be optimized)
        let mut nearest_island = IslandId(0);
        let mut min_dist_sq = f32::MAX;
        
        for region_opt in &cluster_data.regions[0..cluster_data.region_count] {
            if let Some(region) = region_opt {
                let center = region.bounds.center();
                let dx = center.x - pos.x;
                let dy = center.y - pos.y;
                let dist_sq = dx * dx + dy * dy;
                
                if dist_sq < FixedNum::from_num(min_dist_sq) {
                    min_dist_sq = dist_sq.to_num::<f32>();
                    nearest_island = region.island;
                }
            }
        }
        
        nearest_island
    }
}
