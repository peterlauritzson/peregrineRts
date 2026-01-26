mod types;
mod cluster;
mod graph;
mod systems;
mod navigation;
mod debug;
mod navigation_lookup;
mod navigation_routing;
mod resources;

// Region-based pathfinding modules
mod region_decomposition;
mod region_connectivity;
mod island_detection;

#[cfg(test)]
mod tests;

// ============================================================================
// PUBLIC API
// ============================================================================

pub use types::{PathRequest, Path, PathState, Portal, Node, CLUSTER_SIZE, Region, RegionId, IslandId, ClusterId, Direction, GoalNavCell};
pub use graph::{HierarchicalGraph, GraphStats};
pub use systems::process_path_requests;
pub use navigation::{follow_path, sweep_inactive_paths};
pub use navigation_lookup::NavigationLookup;
pub use navigation_routing::NavigationRouting;
pub use resources::ActivePathSet;

// ============================================================================
// CRATE-INTERNAL API
// ============================================================================

pub(crate) use types::{ClusterIslandId, NO_PATH};
pub(crate) use region_decomposition::{get_region_id, get_region_id_by_world_pos, get_island_id_by_world_pos, world_to_cluster_local, point_in_cluster, point_in_region};

use bevy::prelude::*;
use crate::game::GameState;

pub struct PathfindingPlugin;

impl Plugin for PathfindingPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PathRequest>();
        app.init_resource::<HierarchicalGraph>();
        app.init_resource::<NavigationLookup>();        
        app.init_resource::<NavigationRouting>();
        app.init_resource::<ActivePathSet>();  // PERF: Track active paths for O(active) iteration
        app.add_systems(Update, (debug::draw_graph_gizmos).run_if(in_state(GameState::InGame).or(in_state(GameState::Editor))));
        app.add_systems(FixedUpdate, (
            systems::process_path_requests,
            navigation::follow_path,
            navigation::sweep_inactive_paths,  // Batch cleanup after navigation
        ).chain().run_if(in_state(GameState::InGame).or(in_state(GameState::Editor))));
    }
}
