mod types;
mod cluster;
mod graph;
mod components;
mod cluster_flow;
mod astar;
mod graph_build;
mod systems;
mod debug;

// Re-export public API
pub use types::{Path, PathRequest, Node, CLUSTER_SIZE, LocalFlowField, Portal};
pub use graph::HierarchicalGraph;
pub use components::ConnectedComponents;
pub use graph_build::{GraphBuildState, GraphBuildStep, regenerate_cluster_flow_fields};
// find_path_hierarchical is deprecated - use lazy routing table walk instead
pub use systems::process_path_requests;

use bevy::prelude::*;
use crate::game::GameState;

pub struct PathfindingPlugin;

impl Plugin for PathfindingPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PathRequest>();
        app.init_resource::<HierarchicalGraph>();
        app.init_resource::<ConnectedComponents>();
        app.init_resource::<GraphBuildState>();
        // Removed synchronous build_graph system that froze the game for 10+ seconds on large maps
        app.add_systems(Update, (debug::draw_graph_gizmos).run_if(in_state(GameState::InGame).or(in_state(GameState::Editor))));
        app.add_systems(FixedUpdate, systems::process_path_requests.run_if(in_state(GameState::InGame).or(in_state(GameState::Editor))));
        app.add_systems(Update, graph_build::incremental_build_graph.run_if(in_state(GameState::Loading).or(in_state(GameState::Editor)).or(in_state(GameState::InGame))));
        app.add_systems(OnEnter(GameState::Loading), graph_build::start_graph_build);
    }
}
