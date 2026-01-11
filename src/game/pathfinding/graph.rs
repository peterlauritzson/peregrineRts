use bevy::prelude::*;
use serde::{Serialize, Deserialize};
use std::collections::{BTreeMap, BTreeSet};
use crate::game::fixed_math::FixedNum;
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
    /// Precomputed routing table: cluster_routing_table[start_cluster_id][goal_cluster_id] = first_portal_id
    /// Maps every cluster pair to the optimal first portal to take from start toward goal.
    /// Memory: ~90MB for 2048x2048 map (6724^2 * 2 bytes). Eliminates A* between clusters.
    pub cluster_routing_table: BTreeMap<(usize, usize), BTreeMap<(usize, usize), usize>>,
}

impl HierarchicalGraph {
    pub fn reset(&mut self) {
        self.nodes.clear();
        self.edges.clear();
        self.clusters.clear();
        self.cluster_routing_table.clear();
        self.initialized = false;
    }

    pub fn clear_cluster_cache(&mut self, cluster_id: (usize, usize)) {
        if let Some(cluster) = self.clusters.get_mut(&cluster_id) {
            cluster.clear_cache();
        }
    }

    /// Synchronous graph build for testing. In production, use the incremental build system.
    pub fn build_graph_sync(&mut self, flow_field: &crate::game::structures::FlowField) {
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

        // Build cluster routing table
        self.build_routing_table();

        self.initialized = true;
    }

    /// Build cluster-to-cluster routing table for a single source cluster.
    /// Called incrementally during graph build to avoid blocking the UI.
    /// For incremental building, call this once per source cluster across multiple frames.
    pub fn build_routing_table_for_source(&mut self, source_cluster: (usize, usize)) {
        use std::collections::BinaryHeap;
        
        if self.clusters.is_empty() {
            return;
        }
        
        // Run Dijkstra from this source cluster to all other clusters
        let mut distances: BTreeMap<(usize, usize), FixedNum> = BTreeMap::new();
        let mut first_portal: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        let mut open_set = BinaryHeap::new();
        
        distances.insert(source_cluster, FixedNum::ZERO);
            
            // Add all portals from source cluster to open set
            if let Some(cluster) = self.clusters.get(&source_cluster) {
                for &portal_id in &cluster.portals {
                    open_set.push(super::types::GraphState {
                        cost: FixedNum::ZERO,
                        portal_id,
                    });
                    // Mark this portal as the first step to reach its own cluster
                    first_portal.insert(source_cluster, portal_id);
                }
            }
            
            let mut visited_portals = BTreeSet::new();
            
            while let Some(super::types::GraphState { cost, portal_id }) = open_set.pop() {
                if visited_portals.contains(&portal_id) {
                    continue;
                }
                visited_portals.insert(portal_id);
                
                let current_portal = &self.nodes[portal_id];
                let current_cluster = current_portal.cluster;
                
                // Update distance to current cluster
                if cost < *distances.get(&current_cluster).unwrap_or(&FixedNum::MAX) {
                    distances.insert(current_cluster, cost);
                    // Record first portal if not already set for this cluster
                    if !first_portal.contains_key(&current_cluster) {
                        // Trace back to find the first portal from source
                        // For now, just use the current portal - it will be overwritten by earlier ones
                        first_portal.insert(current_cluster, portal_id);
                    }
                }
                
                // Expand to neighboring portals via edges
                if let Some(neighbors) = self.edges.get(&portal_id) {
                    for &(neighbor_id, edge_cost) in neighbors {
                        if visited_portals.contains(&neighbor_id) {
                            continue;
                        }
                        
                        let new_cost = cost + edge_cost;
                        let neighbor_cluster = self.nodes[neighbor_id].cluster;
                        
                        // Track the first portal used to reach this neighbor's cluster
                        if !first_portal.contains_key(&neighbor_cluster) {
                            // Find which portal we used to leave the source cluster
                            if let Some(cluster) = self.clusters.get(&source_cluster) {
                                if cluster.portals.contains(&portal_id) {
                                    // This portal is in source cluster, so it's the first hop
                                    first_portal.insert(neighbor_cluster, portal_id);
                                } else if cluster.portals.contains(&neighbor_id) {
                                    // Neighbor is in source cluster (rare edge case)
                                    first_portal.insert(neighbor_cluster, neighbor_id);
                                } else {
                                    // Neither is in source - use existing first portal for current cluster
                                    if let Some(&fp) = first_portal.get(&current_cluster) {
                                        first_portal.insert(neighbor_cluster, fp);
                                    }
                                }
                            }
                        }
                        
                        open_set.push(super::types::GraphState {
                            cost: new_cost,
                            portal_id: neighbor_id,
                        });
                    }
                }
            }
            
        // Store routing decisions for this source cluster
        let mut route_map = BTreeMap::new();
        for (dest_cluster, portal) in first_portal {
            if dest_cluster != source_cluster {
                route_map.insert(dest_cluster, portal);
            }
        }
        self.cluster_routing_table.insert(source_cluster, route_map);
    }
    
    /// Build cluster-to-cluster routing table using all-pairs shortest path.
    /// For each cluster pair, stores the first portal to take from start toward goal.
    /// This eliminates A* search between clusters at pathfinding time.
    /// 
    /// NOTE: This is the synchronous version - only use for tests or small maps.
    /// For loading screens, use build_routing_table_for_source() incrementally.
    pub fn build_routing_table(&mut self) {
        self.cluster_routing_table.clear();
        
        if self.clusters.is_empty() {
            return;
        }
        
        info!("[ROUTING TABLE] Building cluster routing table for {} clusters...", self.clusters.len());
        let start_time = std::time::Instant::now();
        
        // Collect cluster keys to avoid borrow checker issues
        let cluster_keys: Vec<_> = self.clusters.keys().cloned().collect();
        
        // For each source cluster, run Dijkstra to all other clusters
        for &source_cluster in &cluster_keys {
            self.build_routing_table_for_source(source_cluster);
        }
        
        let total_entries: usize = self.cluster_routing_table.values().map(|m| m.len()).sum();
        info!(
            "[ROUTING TABLE] Built routing table in {:?}: {} cluster pairs, ~{} KB memory",
            start_time.elapsed(),
            total_entries,
            (total_entries * std::mem::size_of::<usize>() * 3) / 1024
        );
    }
    
    /// Lookup next portal to take from current cluster toward goal cluster.
    /// Uses precomputed routing table for O(log n) lookup.
    /// Returns None if no path exists between clusters.
    pub fn get_next_portal(&self, current_cluster: (usize, usize), goal_cluster: (usize, usize)) -> Option<usize> {
        if current_cluster == goal_cluster {
            return None;
        }
        
        self.cluster_routing_table
            .get(&current_cluster)?
            .get(&goal_cluster)
            .copied()
    }
}
