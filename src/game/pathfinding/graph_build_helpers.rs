/// Helper functions for hierarchical graph construction
///
/// These functions handle the low-level operations of building the pathfinding graph:
/// - Cluster initialization
/// - Intra-cluster portal connections
/// - Flow field precomputation
/// - Connected component analysis

use bevy::prelude::*;
use crate::game::pathfinding::graph::HierarchicalGraph;
use crate::game::pathfinding::cluster::Cluster;
use crate::game::pathfinding::components::ConnectedComponents;
use crate::game::pathfinding::types::CLUSTER_SIZE;
use crate::game::pathfinding::astar::find_path_astar_local;
use crate::game::pathfinding::cluster_flow::generate_local_flow_field;
use std::collections::BTreeMap;
use crate::game::fixed_math::FixedNum;
use peregrine_macros::profile;

/// Connect all portals within a single cluster using A* pathfinding
/// 
/// This creates edges between every pair of portals in the cluster if a path exists.
/// Used during graph build to establish intra-cluster connectivity.
pub(crate) fn connect_intra_cluster(
    graph: &mut HierarchicalGraph,
    flow_field: &crate::game::structures::FlowField,
    key: (usize, usize),
) {
    let portals = &graph.clusters[&key].portals;
    let portal_count = portals.len();
    let (cx, cy) = key;
    
    let min_x = cx * CLUSTER_SIZE;
    let max_x = ((cx + 1) * CLUSTER_SIZE).min(flow_field.width) - 1;
    let min_y = cy * CLUSTER_SIZE;
    let max_y = ((cy + 1) * CLUSTER_SIZE).min(flow_field.height) - 1;

    for i in 0..portal_count {
        for j in i+1..portal_count {
            let id1 = portals[i];
            let id2 = portals[j];
            let node1 = graph.nodes[id1].node;
            let node2 = graph.nodes[id2].node;

            if let Some(path) = find_path_astar_local(node1, node2, flow_field, min_x, max_x, min_y, max_y) {
                let cost = FixedNum::from_num(path.len() as usize as f64);
                graph.edges.entry(id1).or_default().push((id2, cost));
                graph.edges.entry(id2).or_default().push((id1, cost));
            }
        }
    }
}

/// Precompute flow fields for all portals in a cluster
/// 
/// For each portal, generates a local flow field showing optimal paths from any point
/// in the cluster to that portal. These cached flow fields are used during pathfinding
/// to quickly navigate units within clusters.
pub(crate) fn precompute_flow_fields_for_cluster(
    graph: &mut HierarchicalGraph,
    flow_field: &crate::game::structures::FlowField,
    key: (usize, usize),
) {
    // Check if cluster exists before trying to access it
    let Some(cluster) = graph.clusters.get(&key) else {
        // Cluster doesn't exist - this can happen for edge areas or uninitialized regions
        return;
    };
    
    let portal_ids: Vec<usize> = cluster.portals.iter().copied().collect();
    for portal_id in portal_ids {
        if let Some(portal) = graph.nodes.get(portal_id) {
            let field = generate_local_flow_field(key, &portal, flow_field);
            if let Some(cluster) = graph.clusters.get_mut(&key) {
                cluster.flow_field_cache.insert(portal_id, field);
            }
        }
    }
}

/// Regenerate flow fields for a specific cluster after obstacles are added.
/// 
/// This is called by apply_new_obstacles after clearing cluster cache.
/// It rebuilds the flow field cache so units can navigate around new obstacles.
pub fn regenerate_cluster_flow_fields(
    graph: &mut HierarchicalGraph,
    flow_field: &crate::game::structures::FlowField,
    cluster_key: (usize, usize),
) {
    precompute_flow_fields_for_cluster(graph, flow_field, cluster_key);
}

/// Initialize cluster data structures for the entire map
/// 
/// Creates empty Cluster objects for each grid position in the cluster grid.
/// These will be populated with portals and flow fields in later build steps.
#[profile(1)]
pub(super) fn initialize_clusters(
    graph: &mut HierarchicalGraph,
    width_clusters: usize,
    height_clusters: usize,
) {
    for cy in 0..height_clusters {
        for cx in 0..width_clusters {
            graph.clusters.insert((cx, cy), Cluster {
                id: (cx, cy),
                portals: Vec::new(),
                flow_field_cache: BTreeMap::new(),
            });
        }
    }
    info!("Initialized {} clusters", width_clusters * height_clusters);
}

/// Build connected components from the hierarchical graph
/// 
/// Analyzes graph connectivity to identify unreachable regions. This helps detect
/// map design issues where areas are completely isolated by obstacles.
#[profile(1)]
pub(super) fn build_connected_components(
    components: &mut ConnectedComponents,
    graph: &HierarchicalGraph,
) {
    components.build_from_graph(graph);
    info!("Connected components built");
}
