#[path = "graph_build_helpers.rs"]
mod graph_build_helpers;

use bevy::prelude::*;
use crate::game::GameState;
use crate::game::loading::{LoadingProgress, TargetGameState};
use crate::game::simulation::MapFlowField;
use crate::game::config::InitialConfig;
use super::graph::HierarchicalGraph;
use super::components::ConnectedComponents;
use super::types::CLUSTER_SIZE;
use graph_build_helpers::{
    initialize_clusters, 
    build_connected_components, 
};
pub(super) use graph_build_helpers::{connect_intra_cluster, precompute_flow_fields_for_cluster};

// Re-export regenerate_cluster_flow_fields for use by simulation module
pub use graph_build_helpers::regenerate_cluster_flow_fields;

#[derive(Resource, Default)]
pub struct GraphBuildState {
    pub step: GraphBuildStep,
    pub cx: usize,
    pub cy: usize,
    pub cluster_keys: Vec<(usize, usize)>,
    pub current_cluster_idx: usize,
    // Routing table build state
    pub routing_source_clusters: Vec<(usize, usize)>,
    pub routing_current_source_idx: usize,
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
            initialize_clusters(&mut graph, width_clusters, height_clusters);
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
            if end_idx > start_idx && end_idx % 50 == 0 {
                info!("  Connected {}/{} clusters - batch of {}", 
                      end_idx, build_state.cluster_keys.len(), end_idx - start_idx);
            }
            loading_progress.progress = 0.40 + 0.20 * (build_state.current_cluster_idx as f32 / build_state.cluster_keys.len() as f32);
        }
        GraphBuildStep::PrecomputingFlowFields => {
            loading_progress.task = "Precomputing Flow Fields...".to_string();
            let batch_size = initial_config.pathfinding_build_batch_size;
            let start_idx = build_state.current_cluster_idx;
            if start_idx == 0 {
                info!("Precomputing flow fields (batch size: {})...", batch_size);
            }
            for _ in 0..batch_size {
                if build_state.current_cluster_idx < build_state.cluster_keys.len() {
                    let key = build_state.cluster_keys[build_state.current_cluster_idx];
                    precompute_flow_fields_for_cluster(&mut graph, flow_field, key);
                    build_state.current_cluster_idx += 1;
                } else {
                    graph.initialized = true;
                    
                    // Build connected components to detect unreachable regions
                    info!("Building connected components...");
                    build_connected_components(&mut components, &graph);
                    
                    // Move to routing table build step (starts at 80%)
                    build_state.step = GraphBuildStep::BuildingRoutingTable;
                    loading_progress.progress = 0.80;
                    break;
                }
            }
            let end_idx = build_state.current_cluster_idx;
            if end_idx > start_idx && end_idx % 50 == 0 {
                info!("  Precomputed flow fields for {}/{} clusters - batch of {}", 
                      end_idx, build_state.cluster_keys.len(), end_idx - start_idx);
            }
            loading_progress.progress = 0.60 + 0.20 * (build_state.current_cluster_idx as f32 / build_state.cluster_keys.len() as f32);
        }
        GraphBuildStep::BuildingRoutingTable => {
            // Build cluster routing table incrementally to avoid blocking
            loading_progress.task = "Building Routing Table...".to_string();
            
            // Initialize routing table source cluster list on first entry
            if build_state.routing_source_clusters.is_empty() {
                build_state.routing_source_clusters = graph.clusters.keys().cloned().collect();
                build_state.routing_current_source_idx = 0;
                graph.cluster_routing_table.clear();
                info!("Building cluster routing table for {} clusters...", build_state.routing_source_clusters.len());
            }
            
            // Process routing table in batches to stay responsive
            let batch_size = initial_config.pathfinding_build_batch_size;
            let start_idx = build_state.routing_current_source_idx;
            
            for _ in 0..batch_size {
                if build_state.routing_current_source_idx < build_state.routing_source_clusters.len() {
                    let source_cluster = build_state.routing_source_clusters[build_state.routing_current_source_idx];
                    graph.build_routing_table_for_source(source_cluster);
                    build_state.routing_current_source_idx += 1;
                } else {
                    // Routing table complete!
                    build_state.step = GraphBuildStep::Done;
                    loading_progress.progress = 1.0;
                    loading_progress.task = "Done".to_string();
                    
                    let total_cached = graph.clusters.values().map(|c| c.flow_field_cache.len()).sum::<usize>();
                    let total_routes: usize = graph.cluster_routing_table.values().map(|m| m.len()).sum();
                    info!("=== GRAPH BUILD COMPLETE ===");
                    info!("incremental_build_graph: Graph build COMPLETE! ({} cached flow fields, {} routing table entries)", total_cached, total_routes);
                    break;
                }
            }
            
            let end_idx = build_state.routing_current_source_idx;
            if end_idx > start_idx && end_idx % 50 == 0 {
                info!("  Built routing table for {}/{} clusters - batch of {}", 
                      end_idx, build_state.routing_source_clusters.len(), end_idx - start_idx);
            }
            
            // Progress: 80% to 100% during routing table build (20% of total)
            loading_progress.progress = 0.80 + 0.20 * (build_state.routing_current_source_idx as f32 / build_state.routing_source_clusters.len() as f32);
        }
        GraphBuildStep::Done => {}
    }
}
