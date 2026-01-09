use bevy::prelude::*;
use crate::game::simulation::MapFlowField;
use crate::game::structures::CELL_SIZE;
use crate::game::math::{FixedVec2, FixedNum};
use super::types::{PathRequest, Node};
use super::graph::HierarchicalGraph;
use super::components::ConnectedComponents;
use super::astar::find_path_hierarchical;

pub(super) fn process_path_requests(
    mut path_requests: MessageReader<PathRequest>,
    mut commands: Commands,
    map_flow_field: Res<MapFlowField>,
    graph: Res<HierarchicalGraph>,
    components: Res<ConnectedComponents>,
) {
    if path_requests.is_empty() {
        return;
    }

    let start_time = std::time::Instant::now();
    let request_count = path_requests.len();
    
    // Warn if too many pending requests (possible accumulation)
    if request_count > 10 {
        warn!("[PATHFINDING] High path request count: {} pending requests!", request_count);
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

    for (_i, request) in path_requests.read().enumerate() {
        let start_node_opt = flow_field.world_to_grid(request.start);
        let goal_node_opt = flow_field.world_to_grid(request.goal);

        if let (Some(start_node), Some(goal_node)) = (start_node_opt, goal_node_opt) {
            if let Some(path) = find_path_hierarchical(
                Node { x: start_node.0, y: start_node.1 },
                Node { x: goal_node.0, y: goal_node.1 },
                flow_field,
                &graph,
                &components,
            ) {
                commands.entity(request.entity).insert(path);
            }
        } else {
            if start_node_opt.is_none() {
                warn!("Start position {:?} is OUT OF BOUNDS! Map bounds: {:?} to {:?}", 
                      request.start, flow_field.origin, 
                      FixedVec2::new(flow_field.origin.x + FixedNum::from_num(flow_field.width as f32 * CELL_SIZE),
                                     flow_field.origin.y + FixedNum::from_num(flow_field.height as f32 * CELL_SIZE)));
            }
            if goal_node_opt.is_none() {
                warn!("Goal position {:?} is OUT OF BOUNDS! Map bounds: {:?} to {:?}", 
                      request.goal, flow_field.origin,
                      FixedVec2::new(flow_field.origin.x + FixedNum::from_num(flow_field.width as f32 * CELL_SIZE),
                                     flow_field.origin.y + FixedNum::from_num(flow_field.height as f32 * CELL_SIZE)));
            }
        }
    }
    
    let total_duration = start_time.elapsed();
    if total_duration.as_millis() > 100 {
        warn!("[PATHFINDING] Slow batch processing: {:?} for {} requests", total_duration, request_count);
    }
}
