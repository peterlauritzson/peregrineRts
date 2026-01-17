use bevy::prelude::*;
use crate::game::simulation::MapFlowField;
use super::types::{PathRequest, CLUSTER_SIZE, IslandId};
use super::graph::HierarchicalGraph;
use super::{get_region_id, world_to_cluster_local};

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
                    get_region_id(&cluster.regions, cluster.region_count, local_goal)
                        .and_then(|region_id| {
                            cluster.regions[region_id.0 as usize]
                                .as_ref()
                                .map(|region| region.island)
                        })
                        .unwrap_or(IslandId(0)) // Default to island 0 if not found
                } else {
                    IslandId(0)
                }
            } else {
                IslandId(0)
            };
            
            // Path component just stores goal - no portal list needed!
            commands.entity(request.entity).insert(super::types::Path::Hierarchical {
                goal: request.goal,
                goal_cluster,
                goal_island,
            });
        }
    }
}
