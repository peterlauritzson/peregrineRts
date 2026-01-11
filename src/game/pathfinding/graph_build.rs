use bevy::prelude::*;
use crate::game::GameState;
use crate::game::loading::{LoadingProgress, TargetGameState};
use crate::game::simulation::MapFlowField;
use crate::game::config::InitialConfig;
use super::graph::HierarchicalGraph;
use super::cluster::Cluster;
use super::components::ConnectedComponents;
use super::types::CLUSTER_SIZE;
use super::astar::find_path_astar_local;
use super::cluster_flow::generate_local_flow_field;
use std::collections::BTreeMap;
use crate::game::fixed_math::FixedNum;

#[derive(Resource, Default)]
pub struct GraphBuildState {
    pub step: GraphBuildStep,
    pub cx: usize,
    pub cy: usize,
    pub cluster_keys: Vec<(usize, usize)>,
    pub current_cluster_idx: usize,
}

#[derive(Default, PartialEq, Eq, Debug, Clone, Copy)]
pub enum GraphBuildStep {
    #[default]
    Done,
    NotStarted,
    InitializingClusters,
    FindingVerticalPortals,
    FindingHorizontalPortals,
    ConnectingIntraCluster,
    PrecomputingFlowFields,
    BuildingRoutingTable,
}

pub(super) fn start_graph_build(
    mut build_state: ResMut<GraphBuildState>,
    graph: Res<HierarchicalGraph>,
    mut loading_progress: ResMut<LoadingProgress>,
    target_state: Option<Res<TargetGameState>>,
    pending_gen: Option<Res<crate::game::editor::PendingMapGeneration>>,
) {
    // If we are going to the editor, don't build the graph automatically
    if let Some(target) = target_state {
        if target.0 == GameState::Editor {
            loading_progress.progress = 1.0;
            loading_progress.task = "Done".to_string();
            build_state.step = GraphBuildStep::Done;
            return;
        }
    }

    // If we have a pending map generation, don't auto-complete loading
    // The handle_pending_map_generation system will reset the graph and trigger a new build
    if pending_gen.is_some() {
        info!("Pending map generation detected - skipping auto-complete, will rebuild graph after generation");
        build_state.step = GraphBuildStep::NotStarted;
        return;
    }

    if !graph.initialized {
        build_state.step = GraphBuildStep::NotStarted;
    } else {
        loading_progress.progress = 1.0;
        loading_progress.task = "Done".to_string();
    }
}

pub(super) fn incremental_build_graph(
    mut graph: ResMut<HierarchicalGraph>,
    map_flow_field: Res<MapFlowField>,
    mut build_state: ResMut<GraphBuildState>,
    mut loading_progress: ResMut<LoadingProgress>,
    config_handle: Res<crate::game::config::GameConfigHandle>,
    game_configs: Res<Assets<crate::game::config::GameConfig>>,
    initial_config: Res<InitialConfig>,
    mut components: ResMut<ConnectedComponents>,
) {
    let Some(_config) = game_configs.get(&config_handle.0) else { return; };
    let flow_field = &map_flow_field.0;
    if flow_field.width == 0 { return; }

    let width_clusters = (flow_field.width + CLUSTER_SIZE - 1) / CLUSTER_SIZE;
    let height_clusters = (flow_field.height + CLUSTER_SIZE - 1) / CLUSTER_SIZE;

    match build_state.step {
        GraphBuildStep::NotStarted => {
            let flow_field = &map_flow_field.0;
            let total_cells = flow_field.width * flow_field.height;
            let total_clusters = width_clusters * height_clusters;
            info!("=== GRAPH BUILD START ===");
            info!("incremental_build_graph: Starting graph build");
            info!("  Map: {} x {} cells ({} total)", flow_field.width, flow_field.height, total_cells);
            info!("  Clusters: {} x {} ({} total, {}x{} cells each)", width_clusters, height_clusters, total_clusters, CLUSTER_SIZE, CLUSTER_SIZE);
            loading_progress.task = "Initializing Graph...".to_string();
            loading_progress.progress = 0.0;
            build_state.step = GraphBuildStep::InitializingClusters;
        }
        GraphBuildStep::InitializingClusters => {
            let init_start = std::time::Instant::now();
            for cy in 0..height_clusters {
                for cx in 0..width_clusters {
                    graph.clusters.insert((cx, cy), Cluster {
                        id: (cx, cy),
                        portals: Vec::new(),
                        flow_field_cache: BTreeMap::new(),
                    });
                }
            }
            info!("Initialized {} clusters in {:?}", width_clusters * height_clusters, init_start.elapsed());
            build_state.cx = 0;
            build_state.cy = 0;
            build_state.step = GraphBuildStep::FindingVerticalPortals;
            loading_progress.progress = 0.1;
        }
        GraphBuildStep::FindingVerticalPortals => {
            loading_progress.task = "Finding Vertical Portals...".to_string();
            let cx = build_state.cx;
            if cx == 0 {
                info!("Finding vertical portals...");
            }
            if cx < width_clusters - 1 {
                for cy in 0..height_clusters {
                    let x1 = (cx + 1) * CLUSTER_SIZE - 1;
                    let x2 = x1 + 1;
                    
                    if x2 >= flow_field.width { continue; }

                    let start_y = cy * CLUSTER_SIZE;
                    let end_y = ((cy + 1) * CLUSTER_SIZE).min(flow_field.height);
                    
                    let mut start_segment = None;
                    
                    for y in start_y..end_y {
                        let idx1 = flow_field.get_index(x1, y);
                        let idx2 = flow_field.get_index(x2, y);
                        let walkable = flow_field.cost_field[idx1] != 255 && flow_field.cost_field[idx2] != 255;
                        
                        if walkable {
                            if start_segment.is_none() {
                                start_segment = Some(y);
                            }
                        } else {
                            if let Some(sy) = start_segment {
                                super::cluster::create_portal_vertical(&mut graph, x1, x2, sy, y - 1, cx, cy, cx + 1, cy);
                                start_segment = None;
                            }
                        }
                    }
                    if let Some(sy) = start_segment {
                         super::cluster::create_portal_vertical(&mut graph, x1, x2, sy, end_y - 1, cx, cy, cx + 1, cy);
                    }
                }
                build_state.cx += 1;
                loading_progress.progress = 0.1 + 0.2 * (cx as f32 / width_clusters as f32);
            } else {
                info!("Found {} total portals (vertical phase complete)", graph.nodes.len());
                build_state.cx = 0;
                build_state.cy = 0;
                build_state.step = GraphBuildStep::FindingHorizontalPortals;
            }
        }
        GraphBuildStep::FindingHorizontalPortals => {
            loading_progress.task = "Finding Horizontal Portals...".to_string();
            let cx = build_state.cx;
            if cx < width_clusters {
                for cy in 0..height_clusters - 1 {
                    let y1 = (cy + 1) * CLUSTER_SIZE - 1;
                    let y2 = y1 + 1;

                    if y2 >= flow_field.height { continue; }

                    let start_x = cx * CLUSTER_SIZE;
                    let end_x = ((cx + 1) * CLUSTER_SIZE).min(flow_field.width);

                    let mut start_segment = None;

                    for x in start_x..end_x {
                        let idx1 = flow_field.get_index(x, y1);
                        let idx2 = flow_field.get_index(x, y2);
                        let walkable = flow_field.cost_field[idx1] != 255 && flow_field.cost_field[idx2] != 255;

                        if walkable {
                            if start_segment.is_none() {
                                start_segment = Some(x);
                            }
                        } else {
                            if let Some(sx) = start_segment {
                                super::cluster::create_portal_horizontal(&mut graph, sx, x - 1, y1, y2, cx, cy, cx, cy + 1);
                                start_segment = None;
                            }
                        }
                    }
                    if let Some(sx) = start_segment {
                        super::cluster::create_portal_horizontal(&mut graph, sx, end_x - 1, y1, y2, cx, cy, cx, cy + 1);
                    }
                }
                build_state.cx += 1;
                loading_progress.progress = 0.3 + 0.2 * (cx as f32 / width_clusters as f32);
            } else {
                let total_portals = graph.nodes.len();
                info!("Found {} total portals (horizontal phase complete)", total_portals);
                build_state.cluster_keys = graph.clusters.keys().cloned().collect();
                build_state.current_cluster_idx = 0;
                build_state.step = GraphBuildStep::ConnectingIntraCluster;
            }
        }
        GraphBuildStep::ConnectingIntraCluster => {
            loading_progress.task = "Connecting Intra-Cluster...".to_string();
            let batch_size = initial_config.pathfinding_build_batch_size;
            let start_idx = build_state.current_cluster_idx;
            if start_idx == 0 {
                info!("Connecting intra-cluster portals (batch size: {})...", batch_size);
            }
            let batch_start = std::time::Instant::now();
            for _ in 0..batch_size {
                if build_state.current_cluster_idx < build_state.cluster_keys.len() {
                    let key = build_state.cluster_keys[build_state.current_cluster_idx];
                    connect_intra_cluster(&mut graph, flow_field, key);
                    build_state.current_cluster_idx += 1;
                } else {
                    let total_edges = graph.edges.values().map(|v| v.len()).sum::<usize>();
                    info!("incremental_build_graph: Finished intra-cluster connections ({} total edges)", total_edges);
                    build_state.current_cluster_idx = 0;
                    build_state.step = GraphBuildStep::PrecomputingFlowFields;
                    break;
                }
            }
            let end_idx = build_state.current_cluster_idx;
            let batch_duration = batch_start.elapsed();
            if end_idx > start_idx && end_idx % 50 == 0 {
                info!("  Connected {}/{} clusters - batch of {} took {:?}", 
                      end_idx, build_state.cluster_keys.len(), end_idx - start_idx, batch_duration);
            }
            loading_progress.progress = 0.5 + 0.25 * (build_state.current_cluster_idx as f32 / build_state.cluster_keys.len() as f32);
        }
        GraphBuildStep::PrecomputingFlowFields => {
            loading_progress.task = "Precomputing Flow Fields...".to_string();
            let batch_size = initial_config.pathfinding_build_batch_size;
            let start_idx = build_state.current_cluster_idx;
            if start_idx == 0 {
                info!("Precomputing flow fields (batch size: {})...", batch_size);
            }
            let batch_start = std::time::Instant::now();
            for _ in 0..batch_size {
                if build_state.current_cluster_idx < build_state.cluster_keys.len() {
                    let key = build_state.cluster_keys[build_state.current_cluster_idx];
                    precompute_flow_fields_for_cluster(&mut graph, flow_field, key);
                    build_state.current_cluster_idx += 1;
                } else {
                    graph.initialized = true;
                    
                    // Build connected components to detect unreachable regions
                    info!("Building connected components...");
                    let conn_start = std::time::Instant::now();
                    components.build_from_graph(&graph);
                    info!("Connected components built in {:?}", conn_start.elapsed());
                    
                    // Move to routing table build step
                    build_state.step = GraphBuildStep::BuildingRoutingTable;
                    loading_progress.progress = 0.95;
                    break;
                }
            }
            let end_idx = build_state.current_cluster_idx;
            let batch_duration = batch_start.elapsed();
            if end_idx > start_idx && end_idx % 50 == 0 {
                info!("  Precomputed flow fields for {}/{} clusters - batch of {} took {:?}", 
                      end_idx, build_state.cluster_keys.len(), end_idx - start_idx, batch_duration);
            }
            loading_progress.progress = 0.75 + 0.20 * (build_state.current_cluster_idx as f32 / build_state.cluster_keys.len() as f32);
        }
        GraphBuildStep::BuildingRoutingTable => {
            // Build cluster routing table for O(1) pathfinding between clusters
            loading_progress.task = "Building Routing Table...".to_string();
            info!("Building cluster routing table...");
            graph.build_routing_table();
            
            build_state.step = GraphBuildStep::Done;
            loading_progress.progress = 1.0;
            loading_progress.task = "Done".to_string();
            let total_cached = graph.clusters.values().map(|c| c.flow_field_cache.len()).sum::<usize>();
            info!("=== GRAPH BUILD COMPLETE ===");
            info!("incremental_build_graph: Graph build COMPLETE! ({} cached flow fields)", total_cached);
        }
        GraphBuildStep::Done => {}
    }
}

pub(super) fn connect_intra_cluster(
    graph: &mut HierarchicalGraph,
    flow_field: &crate::game::structures::FlowField,
    key: (usize, usize),
) {
    let portals = graph.clusters[&key].portals.clone();
    let (cx, cy) = key;
    
    let min_x = cx * CLUSTER_SIZE;
    let max_x = ((cx + 1) * CLUSTER_SIZE).min(flow_field.width) - 1;
    let min_y = cy * CLUSTER_SIZE;
    let max_y = ((cy + 1) * CLUSTER_SIZE).min(flow_field.height) - 1;

    for i in 0..portals.len() {
        for j in i+1..portals.len() {
            let id1 = portals[i];
            let id2 = portals[j];
            let node1 = graph.nodes[id1].node;
            let node2 = graph.nodes[id2].node;

            if let Some(path) = find_path_astar_local(node1, node2, flow_field, min_x, max_x, min_y, max_y) {
                let cost = FixedNum::from_num(path.len() as f64);
                graph.edges.entry(id1).or_default().push((id2, cost));
                graph.edges.entry(id2).or_default().push((id1, cost));
            }
        }
    }
}

pub(super) fn precompute_flow_fields_for_cluster(
    graph: &mut HierarchicalGraph,
    flow_field: &crate::game::structures::FlowField,
    key: (usize, usize),
) {
    // Check if cluster exists before trying to access it
    let Some(cluster) = graph.clusters.get(&key) else {
        // Cluster doesn't exist - this can happen for edge areas or uninitialized regions
        return;
    };
    
    let portals = cluster.portals.clone();
    for portal_id in portals {
        if let Some(portal) = graph.nodes.get(portal_id).cloned() {
            let field = generate_local_flow_field(key, &portal, flow_field);
            if let Some(cluster) = graph.clusters.get_mut(&key) {
                cluster.flow_field_cache.insert(portal_id, field);
            }
        }
    }
}

/// Regenerate flow fields for a specific cluster after obstacles are added.
/// This is called by apply_new_obstacles after clearing cluster cache.
pub fn regenerate_cluster_flow_fields(
    graph: &mut HierarchicalGraph,
    flow_field: &crate::game::structures::FlowField,
    cluster_key: (usize, usize),
) {
    precompute_flow_fields_for_cluster(graph, flow_field, cluster_key);
}
