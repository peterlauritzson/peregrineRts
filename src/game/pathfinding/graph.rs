use bevy::prelude::*;
use serde::{Serialize, Deserialize};
use std::collections::BTreeMap;
use crate::game::math::FixedNum;
use super::types::{CLUSTER_SIZE, Portal};
use super::cluster::Cluster;

/// Hierarchical pathfinding graph for large-scale RTS navigation.
///
/// Divides the map into clusters connected by portals, enabling efficient
/// pathfinding across large maps with thousands of entities.
///
/// # Algorithm
///
/// 1. **Clustering:** Divide map into CLUSTER_SIZE × CLUSTER_SIZE grids
/// 2. **Portals:** Find walkable transitions between adjacent clusters
/// 3. **Inter-cluster:** A* on abstract portal graph (high-level path)
/// 4. **Intra-cluster:** Flow fields guide units within clusters (low-level)
///
/// # Benefits
///
/// - **Scalability:** Graph size = O(portals), not O(cells)
/// - **Caching:** Precompute intra-cluster flow fields once
/// - **Dynamic updates:** Only invalidate affected cluster caches
///
/// # Example Workflow
///
/// ```rust,ignore
/// // 1. Build graph (once, or incrementally during loading)
/// let graph = build_graph_incrementally(&flow_field);
///
/// // 2. Find path
/// if let Some(path) = find_path_hierarchical(start, goal, &flow_field, &graph, &components) {
///     commands.entity(unit).insert(path);
/// }
///
/// // 3. Follow path (see follow_path system in simulation.rs)
/// // Units automatically navigate through portals using cached flow fields
/// ```
///
/// # Performance
///
/// - **Graph build:** O(clusters × portals²) - done once or incrementally
/// - **Pathfinding:** O(portals × log portals) - A* on abstract graph
/// - **Memory:** ~500MB for 2048×2048 map (cached flow fields)
///
/// # See Also
///
/// - [PATHFINDING.md](documents/Design%20docs/PATHFINDING.md) - Detailed design doc
/// - `incremental_build_graph()` - Non-blocking graph construction
/// - Memory budget analysis in CURRENT_IMPROVEMENTS.md Issue #9
#[derive(Resource, Default, Serialize, Deserialize, Clone)]
pub struct HierarchicalGraph {
    pub nodes: Vec<Portal>,
    pub edges: BTreeMap<usize, Vec<(usize, FixedNum)>>, // PortalId -> [(TargetPortalId, Cost)]
    pub clusters: BTreeMap<(usize, usize), Cluster>,
    pub initialized: bool,
}

impl HierarchicalGraph {
    pub fn reset(&mut self) {
        self.nodes.clear();
        self.edges.clear();
        self.clusters.clear();
        self.initialized = false;
    }

    pub fn clear_cluster_cache(&mut self, cluster_id: (usize, usize)) {
        if let Some(cluster) = self.clusters.get_mut(&cluster_id) {
            cluster.clear_cache();
        }
    }

    /// Synchronous graph build for testing. In production, use the incremental build system.
    pub fn build_graph_sync(&mut self, flow_field: &crate::game::flow_field::FlowField) {
        self.reset();
        
        if flow_field.width == 0 || flow_field.height == 0 {
            return;
        }

        let width_clusters = (flow_field.width + CLUSTER_SIZE - 1) / CLUSTER_SIZE;
        let height_clusters = (flow_field.height + CLUSTER_SIZE - 1) / CLUSTER_SIZE;

        // Create clusters
        for cy in 0..height_clusters {
            for cx in 0..width_clusters {
                self.clusters.insert((cx, cy), Cluster {
                    id: (cx, cy),
                    portals: Vec::new(),
                    flow_field_cache: BTreeMap::new(),
                });
            }
        }

        // Vertical portals
        for cy in 0..height_clusters {
            for cx in 0..width_clusters.saturating_sub(1) {
                let min_y = cy * CLUSTER_SIZE;
                let max_y = ((cy + 1) * CLUSTER_SIZE).min(flow_field.height);
                let x1 = (cx + 1) * CLUSTER_SIZE - 1;
                let x2 = (cx + 1) * CLUSTER_SIZE;
                
                if x2 >= flow_field.width { continue; }
                
                let mut start_segment = None;
                for y in min_y..max_y {
                    let idx1 = flow_field.get_index(x1, y);
                    let idx2 = flow_field.get_index(x2, y);
                    let walkable = flow_field.cost_field[idx1] != 255 && flow_field.cost_field[idx2] != 255;

                    if walkable {
                        if start_segment.is_none() {
                            start_segment = Some(y);
                        }
                    } else {
                        if let Some(sy) = start_segment {
                            super::cluster::create_portal_vertical(self, x1, x2, sy, y - 1, cx, cy, cx + 1, cy);
                            start_segment = None;
                        }
                    }
                }
                if let Some(sy) = start_segment {
                    super::cluster::create_portal_vertical(self, x1, x2, sy, max_y - 1, cx, cy, cx + 1, cy);
                }
            }
        }

        // Horizontal portals
        for cx in 0..width_clusters {
            for cy in 0..height_clusters.saturating_sub(1) {
                let min_x = cx * CLUSTER_SIZE;
                let max_x = ((cx + 1) * CLUSTER_SIZE).min(flow_field.width);
                let y1 = (cy + 1) * CLUSTER_SIZE - 1;
                let y2 = (cy + 1) * CLUSTER_SIZE;
                
                if y2 >= flow_field.height { continue; }
                
                let mut start_segment = None;
                for x in min_x..max_x {
                    let idx1 = flow_field.get_index(x, y1);
                    let idx2 = flow_field.get_index(x, y2);
                    let walkable = flow_field.cost_field[idx1] != 255 && flow_field.cost_field[idx2] != 255;

                    if walkable {
                        if start_segment.is_none() {
                            start_segment = Some(x);
                        }
                    } else {
                        if let Some(sx) = start_segment {
                            super::cluster::create_portal_horizontal(self, sx, x - 1, y1, y2, cx, cy, cx, cy + 1);
                            start_segment = None;
                        }
                    }
                }
                if let Some(sx) = start_segment {
                    super::cluster::create_portal_horizontal(self, sx, max_x - 1, y1, y2, cx, cy, cx, cy + 1);
                }
            }
        }

        // Connect intra-cluster
        let cluster_keys: Vec<_> = self.clusters.keys().cloned().collect();
        for key in &cluster_keys {
            super::graph_build::connect_intra_cluster(self, flow_field, *key);
        }

        // Precompute flow fields
        for key in &cluster_keys {
            super::graph_build::precompute_flow_fields_for_cluster(self, flow_field, *key);
        }

        self.initialized = true;
    }
}
