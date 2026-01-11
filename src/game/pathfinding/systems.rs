use bevy::prelude::*;
use crate::game::simulation::MapFlowField;
use super::types::{PathRequest, CLUSTER_SIZE};
use super::graph::HierarchicalGraph;
use super::components::ConnectedComponents;

pub fn process_path_requests(
    mut path_requests: MessageReader<PathRequest>,
    mut commands: Commands,
    map_flow_field: Res<MapFlowField>,
    graph: Res<HierarchicalGraph>,
    _components: Res<ConnectedComponents>,
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
            
            // Path component just stores goal - no portal list needed!
            commands.entity(request.entity).insert(super::types::Path::Hierarchical {
                goal: request.goal,
                goal_cluster,
            });
        }
    }
}
