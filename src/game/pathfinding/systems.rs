use bevy::prelude::*;
use crate::game::simulation::MapFlowField;
use super::types::{PathRequest, CLUSTER_SIZE, IslandId};
use super::graph::HierarchicalGraph;
use super::{get_region_id, world_to_cluster_local};
use super::cluster::Cluster;
use crate::game::fixed_math::{FixedVec2, FixedNum};

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

    // Lazy pathfinding: Just set the goal position on the Path component
    // Movement system will use routing table to look up next portal on-demand
    for request in path_requests.read() {
        let goal_node_opt = flow_field.world_to_grid(request.goal);

        if let Some(goal_node) = goal_node_opt {
            let goal_cluster = (goal_node.0 / CLUSTER_SIZE, goal_node.1 / CLUSTER_SIZE);
            
            // Determine which island the goal is in
            let goal_island = if let Some(cluster) = graph.clusters.get(&goal_cluster) {
                // Convert goal from world coordinates to cluster-local coordinates
                if let Some(local_goal) = world_to_cluster_local(request.goal, goal_cluster, flow_field) {
                    // Try to get region directly
                    let region_id = get_region_id(&cluster.regions, cluster.region_count, local_goal);
                    
                    if let Some(region_id) = region_id {
                        // Goal is in a region - use its island
                        cluster.regions[region_id.0 as usize]
                            .as_ref()
                            .map(|region| region.island)
                            .unwrap_or_else(|| {
                                // Region exists but is None - find nearest
                                find_nearest_island(&cluster, local_goal)
                            })
                    } else {
                        // Goal not in any region - find nearest region's island
                        find_nearest_island(&cluster, local_goal)
                    }
                } else {
                    // Can't convert to local coords - default to island 0
                    IslandId(0)
                }
            } else {
                IslandId(0)
            };
            
            // Debug logging for path requests
            static LOGGED_REQUESTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            if LOGGED_REQUESTS.load(std::sync::atomic::Ordering::Relaxed) < 5 {
                info!("[PATH REQUEST] Goal cluster {:?}, island {} (pos: {:?})", 
                    goal_cluster, goal_island.0, request.goal);
                LOGGED_REQUESTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            
            // Path component just stores goal - no portal list needed!
            commands.entity(request.entity).insert(super::types::Path::Hierarchical {
                goal: request.goal,
                goal_cluster,
                goal_island,
            });
        }
    }
}
