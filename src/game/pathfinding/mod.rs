mod types;
mod cluster;
mod graph;
mod systems;
mod debug;

// Region-based pathfinding modules
mod region_decomposition;
mod region_connectivity;
mod island_detection;

#[cfg(test)]
mod tests;

// ============================================================================
// PUBLIC API
// ============================================================================

pub use types::{PathRequest, Path, Portal, Node, CLUSTER_SIZE, Region, RegionId, IslandId, Direction};
pub use graph::{HierarchicalGraph, GraphStats};
pub use systems::process_path_requests;

// ============================================================================
// CRATE-INTERNAL API
// ============================================================================

pub(crate) use types::{ClusterIslandId, NO_PATH};
pub(crate) use region_decomposition::{get_region_id, world_to_cluster_local};

use bevy::prelude::*;
use crate::game::GameState;

pub struct PathfindingPlugin;

impl Plugin for PathfindingPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PathRequest>();
        app.init_resource::<HierarchicalGraph>();
        app.add_systems(Update, (debug::draw_graph_gizmos).run_if(in_state(GameState::InGame).or(in_state(GameState::Editor))));
        app.add_systems(FixedUpdate, systems::process_path_requests.run_if(in_state(GameState::InGame).or(in_state(GameState::Editor))));
    }
}
