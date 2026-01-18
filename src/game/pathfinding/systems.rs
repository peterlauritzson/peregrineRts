use bevy::prelude::*;
use crate::game::simulation::MapFlowField;
use super::types::{PathRequest, CLUSTER_SIZE, IslandId};
use super::graph::HierarchicalGraph;
use super::{get_region_id, world_to_cluster_local};
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

pub fn process_path_requests(
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
            let (goal_island, goal_region) = if let Some(cluster) = graph.clusters.get(&goal_cluster) {
                // Convert goal from world coordinates to cluster-local coordinates
                if let Some(local_goal) = world_to_cluster_local(walkable_goal, goal_cluster, flow_field) {
                    // Try to get region directly
                    let region_id = get_region_id(&cluster.regions, cluster.region_count, local_goal);
                    
                    if let Some(region_id) = region_id {
                        // Goal is in a region - use its island
                        let island = cluster.regions[region_id.0 as usize]
                            .as_ref()
                            .map(|region| region.island)
                            .unwrap_or_else(|| {
                                // Region exists but is None - find nearest
                                find_nearest_island(&cluster, local_goal)
                            });
                        (island, Some(region_id))
                    } else {
                        // Goal not in any region - find nearest region's island
                        let island = find_nearest_island(&cluster, local_goal);
                        (island, None)
                    }
                } else {
                    // Can't convert to local coords - default to island 0
                    (IslandId(0), None)
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
            
            // Create Path component with validated goal
            commands.entity(request.entity).insert(super::types::Path::Hierarchical {
                goal: walkable_goal,  // Use snapped position
                goal_cluster,
                goal_region,  // Cache goal region
                goal_island,
            });
        } else {
            warn!("Path request for entity {:?} rejected: goal {:?} is outside grid bounds", 
                request.entity, walkable_goal);
        }
    }
}
