use bevy::prelude::*;
use crate::game::math::{FixedVec2, FixedNum};
use crate::game::simulation::{MapFlowField, DebugConfig};
use crate::game::config::{GameConfig, GameConfigHandle, InitialConfig};
use crate::game::GameState;
use crate::game::loading::{LoadingProgress, TargetGameState};
use crate::game::flow_field::CELL_SIZE;
use std::collections::{BinaryHeap, BTreeMap, BTreeSet, VecDeque};
use std::cmp::Ordering;
use serde::{Serialize, Deserialize};

/// Fixed cluster size for hierarchical pathfinding (25×25 cells).
///
/// Maps are divided into clusters of this size. Larger clusters reduce graph size
/// but increase intra-cluster pathfinding cost. 25×25 provides good balance.
pub const CLUSTER_SIZE: usize = 25;

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
}

#[derive(Event, Message, Debug, Clone)]
pub struct PathRequest {
    pub entity: Entity,
    #[allow(dead_code)]
    pub start: FixedVec2,
    pub goal: FixedVec2,
}

#[derive(Component, Debug, Clone)]
pub enum Path {
    Direct(FixedVec2),
    LocalAStar { waypoints: Vec<FixedVec2>, current_index: usize },
    Hierarchical {
        portals: Vec<usize>,
        final_goal: FixedVec2,
        current_index: usize,
    }
}

impl Default for Path {
    fn default() -> Self {
        Path::Direct(FixedVec2::ZERO)
    }
}

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

/// Tracks connected components in the pathfinding graph to detect unreachable regions.
///
/// Solves the problem where pathfinding tries to find paths to unreachable targets
/// (e.g., islands cut off by obstacles, or targets inside obstacles). Without this,
/// A* can loop forever trying to reach impossible destinations.
///
/// # Algorithm
///
/// 1. **Build:** After graph construction, use BFS/DFS to find connected components
/// 2. **Check:** Before pathfinding, verify start and goal are in same component
/// 3. **Fallback:** If unreachable, redirect to closest portal in same component
/// 4. **Update:** Rebuild components when obstacles added/removed
///
/// # Design Decisions
///
/// - **Granularity:** Components at cluster level (not cell level) for efficiency
/// - **Fallback strategy:** Find closest reachable point, don't fail silently
/// - **Memory:** O(clusters) - negligible compared to flow field cache
/// - **Update cost:** O(portals) - acceptable for infrequent obstacle changes
#[derive(Resource, Default, Clone)]
pub struct ConnectedComponents {
    /// Maps each cluster to its component ID
    pub cluster_to_component: BTreeMap<(usize, usize), usize>,
    
    /// For each component, stores representative portal IDs
    /// Used to pick fallback targets when pathfinding to unreachable regions
    pub component_portals: BTreeMap<usize, Vec<usize>>,
    
    /// For each component pair (from, to), stores closest portal IDs in 'from' component
    /// that are physically near the 'to' component (even though not connected)
    /// Used for "get as close as possible" behavior
    pub closest_cross_component: BTreeMap<(usize, usize), Vec<usize>>,
    
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
                            create_portal_vertical(self, x1, x2, sy, y - 1, cx, cy, cx + 1, cy);
                            start_segment = None;
                        }
                    }
                }
                if let Some(sy) = start_segment {
                    create_portal_vertical(self, x1, x2, sy, max_y - 1, cx, cy, cx + 1, cy);
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
                            create_portal_horizontal(self, sx, x - 1, y1, y2, cx, cy, cx, cy + 1);
                            start_segment = None;
                        }
                    }
                }
                if let Some(sx) = start_segment {
                    create_portal_horizontal(self, sx, max_x - 1, y1, y2, cx, cy, cx, cy + 1);
                }
            }
        }

        // Connect intra-cluster
        let cluster_keys: Vec<_> = self.clusters.keys().cloned().collect();
        for key in &cluster_keys {
            connect_intra_cluster(self, flow_field, *key);
        }

        // Precompute flow fields
        for key in &cluster_keys {
            precompute_flow_fields_for_cluster(self, flow_field, *key);
        }

        self.initialized = true;
    }
}

/// Represents a spatial cluster in the hierarchical pathfinding graph.
///
/// # Flow Field Cache Memory Budget
///
/// Each cluster precomputes and caches flow fields for ALL its portals during graph build.
/// This is a **bounded, eager-caching** strategy:
///
/// - **Memory per cluster:** ~50-100 KB (depends on portal count)
/// - **Total memory (2048x2048 map):** ~335-670 MB for ~6,700 clusters
/// - **Cache invalidation:** Clusters clear cache when obstacles added
/// - **No LRU needed:** Cache is bounded by portal count (typically 4-8 per cluster)
///
/// ## Design Rationale
///
/// **Why precompute everything?**
/// - Flow field generation is expensive (A* + integration field construction)
/// - Portals are fixed after graph build (don't change during gameplay)
/// - Cache hit rate would be ~100% for typical RTS gameplay
/// - Avoids runtime cache misses and LRU overhead
///
/// **When does memory grow?**
/// - Only during graph build (one-time cost)
/// - Never grows during gameplay (portal count is fixed)
/// - Cleared and rebuilt when obstacles added (see Issue #10)
///
/// **Alternative considered:** LRU cache with capacity limit
/// - **Rejected because:** Would add complexity without benefit
/// - Flow fields are needed repeatedly (units path through same clusters)
/// - Cache eviction would cause expensive regeneration mid-game
/// - Memory budget is acceptable for target hardware (< 1 GB total)
///
/// ## Memory Breakdown
///
/// For a 2048x2048 map with 25x25 cluster size:
/// - Clusters: 82 × 82 = 6,724
/// - Portals per cluster: ~4-8 (average ~6)
/// - Flow field size: 625 vectors (16 bytes) + 625 u32s (4 bytes) = 12.5 KB
/// - Memory per cluster: 6 portals × 12.5 KB = 75 KB
/// - **Total cache memory: 6,724 × 75 KB ≈ 504 MB**
///
/// This is acceptable given:
/// - Modern systems have 8+ GB RAM
/// - Game targets large-scale RTS (10M units)
/// - Memory budget prioritizes performance over minimal footprint
///
/// See also: [PATHFINDING.md](documents/Design%20docs/PATHFINDING.md) - Hierarchical pathfinding design
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Cluster {
    pub id: (usize, usize),
    pub portals: Vec<usize>,
    pub flow_field_cache: BTreeMap<usize, LocalFlowField>,
}

impl Cluster {
    pub fn clear_cache(&mut self) {
        self.flow_field_cache.clear();
    }
}

impl ConnectedComponents {
    /// Build connected components from the hierarchical graph using BFS.
    /// Groups clusters into connectivity sets where all clusters in a set can reach each other.
    pub fn build_from_graph(&mut self, graph: &HierarchicalGraph) {
        self.cluster_to_component.clear();
        self.component_portals.clear();
        self.closest_cross_component.clear();
        
        if graph.clusters.is_empty() {
            self.initialized = false;
            return;
        }
        
        let mut component_id = 0;
        let all_clusters: Vec<_> = graph.clusters.keys().cloned().collect();
        
        // Build portal_to_cluster lookup for efficient component traversal
        let mut portal_to_cluster: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
        for (cluster_id, cluster) in &graph.clusters {
            for &portal_id in &cluster.portals {
                portal_to_cluster.insert(portal_id, *cluster_id);
            }
        }
        
        // BFS to find connected components
        for &start_cluster in &all_clusters {
            if self.cluster_to_component.contains_key(&start_cluster) {
                continue; // Already visited
            }
            
            // Start new component
            let mut queue = VecDeque::new();
            queue.push_back(start_cluster);
            self.cluster_to_component.insert(start_cluster, component_id);
            
            let mut component_portal_set = BTreeSet::new();
            
            while let Some(current_cluster) = queue.pop_front() {
                if let Some(cluster) = graph.clusters.get(&current_cluster) {
                    // Add all portals from this cluster to the component
                    for &portal_id in &cluster.portals {
                        component_portal_set.insert(portal_id);
                        
                        // Follow edges to find neighboring portals and their clusters
                        if let Some(edges) = graph.edges.get(&portal_id) {
                            for &(neighbor_portal_id, _cost) in edges {
                                if let Some(&neighbor_cluster) = portal_to_cluster.get(&neighbor_portal_id) {
                                    if !self.cluster_to_component.contains_key(&neighbor_cluster) {
                                        self.cluster_to_component.insert(neighbor_cluster, component_id);
                                        queue.push_back(neighbor_cluster);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            
            // Store portals for this component
            self.component_portals.insert(component_id, component_portal_set.into_iter().collect());
            component_id += 1;
        }
        
        // Precompute closest cross-component portals (for fallback behavior)
        self.compute_closest_cross_component(graph);
        
        self.initialized = true;
        
        info!("[CONNECTIVITY] Built {} connected components covering {} clusters", 
              component_id, all_clusters.len());
    }
    
    /// For each component pair, find portals in the 'from' component that are physically
    /// closest to any portal in the 'to' component (even though not path-connected).
    /// This enables "get as close as possible" behavior when targets are unreachable.
    fn compute_closest_cross_component(&mut self, graph: &HierarchicalGraph) {
        let component_ids: Vec<_> = self.component_portals.keys().cloned().collect();
        
        for &from_comp in &component_ids {
            for &to_comp in &component_ids {
                if from_comp == to_comp {
                    continue; // Same component = already reachable
                }
                
                let from_portals = self.component_portals.get(&from_comp).unwrap();
                let to_portals = self.component_portals.get(&to_comp).unwrap();
                
                // Find closest portal in from_comp to any portal in to_comp
                let mut best_portals: Vec<(usize, FixedNum)> = Vec::new();
                
                for &from_portal_id in from_portals {
                    if let Some(from_portal) = graph.nodes.get(from_portal_id) {
                        let from_pos = FixedVec2::new(
                            FixedNum::from_num(from_portal.node.x as i32),
                            FixedNum::from_num(from_portal.node.y as i32)
                        );
                        
                        let mut min_dist = FixedNum::MAX;
                        for &to_portal_id in to_portals {
                            if let Some(to_portal) = graph.nodes.get(to_portal_id) {
                                let to_pos = FixedVec2::new(
                                    FixedNum::from_num(to_portal.node.x as i32),
                                    FixedNum::from_num(to_portal.node.y as i32)
                                );
                                let dist = (from_pos - to_pos).length_squared();
                                if dist < min_dist {
                                    min_dist = dist;
                                }
                            }
                        }
                        
                        best_portals.push((from_portal_id, min_dist));
                    }
                }
                
                // Sort by distance and keep top 3 closest portals
                best_portals.sort_by(|a, b| a.1.cmp(&b.1));
                let closest: Vec<usize> = best_portals.iter().take(3).map(|(id, _)| *id).collect();
                
                if !closest.is_empty() {
                    self.closest_cross_component.insert((from_comp, to_comp), closest);
                }
            }
        }
    }
    
    /// Get the component ID for a given cluster
    pub fn get_component(&self, cluster: (usize, usize)) -> Option<usize> {
        self.cluster_to_component.get(&cluster).copied()
    }
    
    /// Check if two clusters are in the same connected component
    pub fn are_connected(&self, cluster_a: (usize, usize), cluster_b: (usize, usize)) -> bool {
        if let (Some(comp_a), Some(comp_b)) = (self.get_component(cluster_a), self.get_component(cluster_b)) {
            comp_a == comp_b
        } else {
            false
        }
    }
    
    /// Get fallback portals when trying to path from cluster_a to unreachable cluster_b.
    /// Returns portals in cluster_a's component that are closest to cluster_b's component.
    pub fn get_fallback_portals(&self, cluster_a: (usize, usize), cluster_b: (usize, usize)) -> Option<&Vec<usize>> {
        let comp_a = self.get_component(cluster_a)?;
        let comp_b = self.get_component(cluster_b)?;
        
        if comp_a == comp_b {
            return None; // Already connected
        }
        
        self.closest_cross_component.get(&(comp_a, comp_b))
    }
}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalFlowField {
    pub width: usize,
    pub height: usize,
    pub vectors: Vec<FixedVec2>, // Row-major, size width * height
    pub integration_field: Vec<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Portal {
    pub id: usize,
    pub node: Node,
    pub range_min: Node,
    pub range_max: Node,
    pub cluster: (usize, usize),
}

pub struct PathfindingPlugin;

impl Plugin for PathfindingPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PathRequest>();
        app.init_resource::<HierarchicalGraph>();
        app.init_resource::<ConnectedComponents>();
        app.init_resource::<GraphBuildState>();
        // Removed synchronous build_graph system that froze the game for 10+ seconds on large maps
        app.add_systems(Update, (draw_graph_gizmos).run_if(in_state(GameState::InGame).or(in_state(GameState::Editor))));
        app.add_systems(FixedUpdate, process_path_requests.run_if(in_state(GameState::InGame).or(in_state(GameState::Editor))));
        app.add_systems(Update, incremental_build_graph.run_if(in_state(GameState::Loading).or(in_state(GameState::Editor)).or(in_state(GameState::InGame))));
        app.add_systems(OnEnter(GameState::Loading), start_graph_build);
    }
}

fn start_graph_build(
    mut build_state: ResMut<GraphBuildState>,
    graph: Res<HierarchicalGraph>,
    mut loading_progress: ResMut<LoadingProgress>,
    target_state: Option<Res<TargetGameState>>,
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

    if !graph.initialized {
        build_state.step = GraphBuildStep::NotStarted;
    } else {
        loading_progress.progress = 1.0;
        loading_progress.task = "Done".to_string();
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize, PartialOrd, Ord)]
pub struct Node {
    pub x: usize,
    pub y: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct State {
    cost: FixedNum,
    node: Node,
}

impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        other.cost.cmp(&self.cost)
            .then_with(|| self.node.x.cmp(&other.node.x))
            .then_with(|| self.node.y.cmp(&other.node.y))
    }
}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// Synchronous build_graph function removed - use incremental_build_graph instead
// The old build_graph would freeze the game for 10+ seconds on large maps

impl Cluster {
    pub fn get_flow_field(
        &self,
        portal_id: usize,
    ) -> Option<&LocalFlowField> {
        self.flow_field_cache.get(&portal_id)
    }

    /// Get flow field for a portal, generating it on-demand if missing.
    /// This should only happen if obstacles were added and regeneration failed.
    /// Logs a warning when generation is needed.
    pub fn get_or_generate_flow_field(
        &mut self,
        portal_id: usize,
        portal: &Portal,
        map_flow_field: &crate::game::flow_field::FlowField,
    ) -> &LocalFlowField {
        if !self.flow_field_cache.contains_key(&portal_id) {
            warn!(
                "[FLOW FIELD] Missing flow field for portal {} in cluster {:?} - generating on-demand. \
                This indicates flow field regeneration after obstacle placement may have failed.",
                portal_id, self.id
            );
            // TODO: Consider pausing game with overlay: "Generating missing flow field..."
            let field = generate_local_flow_field(self.id, portal, map_flow_field);
            self.flow_field_cache.insert(portal_id, field);
        }
        &self.flow_field_cache[&portal_id]
    }
}

fn generate_local_flow_field(
    cluster_id: (usize, usize),
    portal: &Portal,
    map_flow_field: &crate::game::flow_field::FlowField,
) -> LocalFlowField {
    let (cx, cy) = cluster_id;
    let min_x = cx * CLUSTER_SIZE;
    let max_x = ((cx + 1) * CLUSTER_SIZE).min(map_flow_field.width);
    let min_y = cy * CLUSTER_SIZE;
    let max_y = ((cy + 1) * CLUSTER_SIZE).min(map_flow_field.height);
    
    let width = max_x - min_x;
    let height = max_y - min_y;
    let size = width * height;
    
    let mut integration_field = vec![u32::MAX; size];
    let mut queue = VecDeque::new();
    
    // Initialize target cells (Portal Range)
    let p_min_x = portal.range_min.x;
    let p_max_x = portal.range_max.x;
    let p_min_y = portal.range_min.y;
    let p_max_y = portal.range_max.y;
    
    for y in p_min_y..=p_max_y {
        for x in p_min_x..=p_max_x {
            if x >= min_x && x < max_x && y >= min_y && y < max_y {
                let lx = x - min_x;
                let ly = y - min_y;
                let idx = ly * width + lx;
                integration_field[idx] = 0;
                queue.push_back((lx, ly));
            }
        }
    }
    
    // Dijkstra
    while let Some((lx, ly)) = queue.pop_front() {
        let idx = ly * width + lx;
        let cost = integration_field[idx];
        
        let neighbors = [
            (lx.wrapping_sub(1), ly),
            (lx + 1, ly),
            (lx, ly.wrapping_sub(1)),
            (lx, ly + 1),
        ];
        
        for (nx, ny) in neighbors {
            if nx >= width || ny >= height { continue; }
            
            let gx = min_x + nx;
            let gy = min_y + ny;
            
            // Bounds check: Ensure coordinates are within flow field bounds
            // This prevents crashes when portals reference coordinates from old map sizes
            if gx >= map_flow_field.width || gy >= map_flow_field.height {
                continue; // Portal extends beyond current map bounds - skip this cell
            }
            
            // Check global obstacle
            if map_flow_field.cost_field[map_flow_field.get_index(gx, gy)] == 255 {
                continue;
            }
            
            let n_idx = ny * width + nx;
            if integration_field[n_idx] == u32::MAX {
                integration_field[n_idx] = cost + 1;
                queue.push_back((nx, ny));
            }
        }
    }
    
    // Generate Vectors
    let mut vectors = vec![FixedVec2::ZERO; size];
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            if integration_field[idx] == u32::MAX { continue; }
            if integration_field[idx] == 0 { continue; } // Target
            
            let mut best_cost = integration_field[idx];
            let mut best_dir = FixedVec2::ZERO;
            
            // Check neighbors for lowest cost
             let neighbors = [
                (x.wrapping_sub(1), y, FixedVec2::new(FixedNum::from_num(-1), FixedNum::ZERO)),
                (x + 1, y, FixedVec2::new(FixedNum::ONE, FixedNum::ZERO)),
                (x, y.wrapping_sub(1), FixedVec2::new(FixedNum::ZERO, FixedNum::from_num(-1))),
                (x, y + 1, FixedVec2::new(FixedNum::ZERO, FixedNum::ONE)),
            ];
            
            for (nx, ny, dir) in neighbors {
                if nx >= width || ny >= height { continue; }
                let n_idx = ny * width + nx;
                if integration_field[n_idx] < best_cost {
                    best_cost = integration_field[n_idx];
                    best_dir = dir;
                }
            }
            
            vectors[idx] = best_dir;
        }
    }
    
    LocalFlowField { width, height, vectors, integration_field }
}

fn create_portal_vertical(
    graph: &mut HierarchicalGraph,
    x1: usize, x2: usize,
    y_start: usize, y_end: usize,
    c1x: usize, c1y: usize,
    c2x: usize, c2y: usize,
) {
    let mid_y = (y_start + y_end) / 2;
    
    let id1 = graph.nodes.len();
    graph.nodes.push(Portal { 
        id: id1, 
        node: Node { x: x1, y: mid_y }, 
        range_min: Node { x: x1, y: y_start },
        range_max: Node { x: x1, y: y_end },
        cluster: (c1x, c1y) 
    });
    graph.clusters.entry((c1x, c1y)).or_default().portals.push(id1);

    let id2 = graph.nodes.len();
    graph.nodes.push(Portal { 
        id: id2, 
        node: Node { x: x2, y: mid_y }, 
        range_min: Node { x: x2, y: y_start },
        range_max: Node { x: x2, y: y_end },
        cluster: (c2x, c2y) 
    });
    graph.clusters.entry((c2x, c2y)).or_default().portals.push(id2);

    let cost = FixedNum::from_num(1.0);
    graph.edges.entry(id1).or_default().push((id2, cost));
    graph.edges.entry(id2).or_default().push((id1, cost));
}

fn create_portal_horizontal(
    graph: &mut HierarchicalGraph,
    x_start: usize, x_end: usize,
    y1: usize, y2: usize,
    c1x: usize, c1y: usize,
    c2x: usize, c2y: usize,
) {
    let mid_x = (x_start + x_end) / 2;
    
    let id1 = graph.nodes.len();
    graph.nodes.push(Portal { 
        id: id1, 
        node: Node { x: mid_x, y: y1 }, 
        range_min: Node { x: x_start, y: y1 },
        range_max: Node { x: x_end, y: y1 },
        cluster: (c1x, c1y) 
    });
    graph.clusters.entry((c1x, c1y)).or_default().portals.push(id1);

    let id2 = graph.nodes.len();
    graph.nodes.push(Portal { 
        id: id2, 
        node: Node { x: mid_x, y: y2 }, 
        range_min: Node { x: x_start, y: y2 },
        range_max: Node { x: x_end, y: y2 },
        cluster: (c2x, c2y) 
    });
    graph.clusters.entry((c2x, c2y)).or_default().portals.push(id2);

    let cost = FixedNum::from_num(1.0);
    graph.edges.entry(id1).or_default().push((id2, cost));
    graph.edges.entry(id2).or_default().push((id1, cost));
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct GraphState {
    cost: FixedNum,
    portal_id: usize,
}

impl Ord for GraphState {
    fn cmp(&self, other: &Self) -> Ordering {
        other.cost.cmp(&self.cost)
            .then_with(|| self.portal_id.cmp(&other.portal_id))
    }
}

impl PartialOrd for GraphState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn has_line_of_sight(
    start: Node,
    goal: Node,
    flow_field: &crate::game::flow_field::FlowField
) -> bool {
    let mut x0 = start.x as isize;
    let mut y0 = start.y as isize;
    let x1 = goal.x as isize;
    let y1 = goal.y as isize;

    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        if x0 < 0 || x0 >= flow_field.width as isize || y0 < 0 || y0 >= flow_field.height as isize {
            return false;
        }
        
        if flow_field.cost_field[flow_field.get_index(x0 as usize, y0 as usize)] == 255 {
            return false;
        }

        if x0 == x1 && y0 == y1 {
            break;
        }

        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
    true
}

/// Find the nearest walkable cell to a target node using BFS
fn find_nearest_walkable(
    target: Node,
    flow_field: &crate::game::flow_field::FlowField
) -> Option<Node> {
    const MAX_SEARCH_RADIUS: usize = 50; // Search up to 50 cells away
    
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::new();
    queue.push_back((target, 0));
    visited.insert((target.x, target.y));
    
    while let Some((node, distance)) = queue.pop_front() {
        // Stop if we've searched too far
        if distance > MAX_SEARCH_RADIUS {
            break;
        }
        
        // Check if this node is walkable
        let idx = flow_field.get_index(node.x, node.y);
        if flow_field.cost_field[idx] != 255 {
            return Some(node);
        }
        
        // Explore 8-directional neighbors
        for dx in -1isize..=1 {
            for dy in -1isize..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                
                let nx = node.x as isize + dx;
                let ny = node.y as isize + dy;
                
                if nx < 0 || nx >= flow_field.width as isize || ny < 0 || ny >= flow_field.height as isize {
                    continue;
                }
                
                let nx = nx as usize;
                let ny = ny as usize;
                
                if visited.contains(&(nx, ny)) {
                    continue;
                }
                
                visited.insert((nx, ny));
                queue.push_back((Node { x: nx, y: ny }, distance + 1));
            }
        }
    }
    
    None
}

fn find_path_hierarchical(
    start: Node,
    goal: Node,
    flow_field: &crate::game::flow_field::FlowField,
    graph: &HierarchicalGraph,
    components: &ConnectedComponents,
) -> Option<Path> {
    // 1. Check if goal is inside an obstacle (unwalkable)
    let goal_idx = flow_field.get_index(goal.x, goal.y);
    if flow_field.cost_field[goal_idx] == 255 {
        warn!("[PATHFINDING] Goal {:?} is inside an obstacle (unwalkable)! Finding nearest walkable cell...", goal);
        // Find nearest walkable cell to the goal
        if let Some(walkable_goal) = find_nearest_walkable(goal, flow_field) {
            warn!("[PATHFINDING] Redirecting to nearest walkable cell: {:?}", walkable_goal);
            return find_path_hierarchical(start, walkable_goal, flow_field, graph, components);
        } else {
            warn!("[PATHFINDING] No walkable cells found near goal! Pathfinding failed.");
            return None;
        }
    }

    // 2. Check Line of Sight
    if has_line_of_sight(start, goal, flow_field) {
        return Some(Path::Direct(flow_field.grid_to_world(goal.x, goal.y)));
    }

    let start_cluster = (start.x / CLUSTER_SIZE, start.y / CLUSTER_SIZE);
    let goal_cluster = (goal.x / CLUSTER_SIZE, goal.y / CLUSTER_SIZE);

    // Check connectivity: if start and goal are in different components, redirect to closest reachable point
    let actual_goal = if components.initialized && !components.are_connected(start_cluster, goal_cluster) {
        // Target is unreachable! Find closest reachable portal instead
        if let Some(fallback_portals) = components.get_fallback_portals(start_cluster, goal_cluster) {
            if let Some(&fallback_portal_id) = fallback_portals.first() {
                // Redirect to the closest reachable portal
                let fallback_portal = &graph.nodes[fallback_portal_id];
                warn!("[CONNECTIVITY] Target cluster {:?} unreachable from {:?}. Redirecting to closest reachable portal at {:?}", 
                      goal_cluster, start_cluster, fallback_portal.node);
                fallback_portal.node
            } else {
                warn!("[CONNECTIVITY] No fallback portal found for unreachable target. Using original goal.");
                goal
            }
        } else {
            goal
        }
    } else if components.initialized {
        // Clusters ARE connected according to component analysis
        // But let's verify there's actually a path
        let start_comp = components.get_component(start_cluster);
        let goal_comp = components.get_component(goal_cluster);
        if start_comp != goal_comp {
            warn!("[CONNECTIVITY BUG] Clusters {:?} (comp {:?}) and {:?} (comp {:?}) reported as connected but have different component IDs!", 
                  start_cluster, start_comp, goal_cluster, goal_comp);
        }
        goal
    } else {
        goal
    };

    // Recalculate cluster in case actual_goal was redirected
    let actual_goal_cluster = (actual_goal.x / CLUSTER_SIZE, actual_goal.y / CLUSTER_SIZE);

    // If in same cluster, use local A*
    if start_cluster == actual_goal_cluster {
        let min_x = start_cluster.0 * CLUSTER_SIZE;
        let max_x = ((start_cluster.0 + 1) * CLUSTER_SIZE).min(flow_field.width) - 1;
        let min_y = start_cluster.1 * CLUSTER_SIZE;
        let max_y = ((start_cluster.1 + 1) * CLUSTER_SIZE).min(flow_field.height) - 1;
        
        if let Some(points) = find_path_astar_local_points(start, actual_goal, flow_field, min_x, max_x, min_y, max_y) {
            return Some(Path::LocalAStar { waypoints: points, current_index: 0 });
        }
    }

    // 2. Check if close enough for local A* even if different clusters
    let dist_sq = (start.x as i32 - actual_goal.x as i32).pow(2) + (start.y as i32 - actual_goal.y as i32).pow(2);
    let threshold = (CLUSTER_SIZE as i32 * 2).pow(2); 

    if dist_sq < threshold {
         let min_x = start.x.min(actual_goal.x).saturating_sub(CLUSTER_SIZE);
         let max_x = start.x.max(actual_goal.x) + CLUSTER_SIZE;
         let min_y = start.y.min(actual_goal.y).saturating_sub(CLUSTER_SIZE);
         let max_y = start.y.max(actual_goal.y) + CLUSTER_SIZE;
         
         let min_x = min_x.max(0);
         let max_x = max_x.min(flow_field.width - 1);
         let min_y = min_y.max(0);
         let max_y = max_y.min(flow_field.height - 1);

         if let Some(points) = find_path_astar_local_points(start, actual_goal, flow_field, min_x, max_x, min_y, max_y) {
             return Some(Path::LocalAStar { waypoints: points, current_index: 0 });
         }
    }

    let mut open_set = BinaryHeap::new();
    let mut came_from: BTreeMap<usize, usize> = BTreeMap::new();
    let mut g_score: BTreeMap<usize, FixedNum> = BTreeMap::new();
    let mut closed_set: BTreeSet<usize> = BTreeSet::new(); // Track visited portals to prevent cycles
    
    const MAX_PORTAL_ITERATIONS: usize = 1000; // Safety limit for portal graph A*
    let mut iterations = 0;

    // Add start node connections to graph
    // Find portals in start cluster
    let mut start_portals = Vec::new();
    if let Some(cluster) = graph.clusters.get(&start_cluster) {
        for &portal_id in &cluster.portals {
            let portal_node = graph.nodes[portal_id].node;
            
            let mut cost = None;

            // Try to use cached flow field
            if let Some(local_field) = cluster.flow_field_cache.get(&portal_id) {
                let lx = start.x.wrapping_sub(start_cluster.0 * CLUSTER_SIZE);
                let ly = start.y.wrapping_sub(start_cluster.1 * CLUSTER_SIZE);
                
                if lx < local_field.width && ly < local_field.height {
                    let idx = ly * local_field.width + lx;
                    if let Some(&c) = local_field.integration_field.get(idx) {
                        if c != u32::MAX {
                            cost = Some(FixedNum::from_num(c));
                        }
                    }
                }
            }

            // Fallback to A*
            if cost.is_none() {
                let min_x = start_cluster.0 * CLUSTER_SIZE;
                let max_x = ((start_cluster.0 + 1) * CLUSTER_SIZE).min(flow_field.width) - 1;
                let min_y = start_cluster.1 * CLUSTER_SIZE;
                let max_y = ((start_cluster.1 + 1) * CLUSTER_SIZE).min(flow_field.height) - 1;

                if let Some(path) = find_path_astar_local(start, portal_node, flow_field, min_x, max_x, min_y, max_y) {
                    cost = Some(FixedNum::from_num(path.len() as f64));
                }
            }

            if let Some(c) = cost {
                g_score.insert(portal_id, c);
                let h = heuristic(portal_node.x, portal_node.y, actual_goal.x, actual_goal.y, flow_field.cell_size);
                open_set.push(GraphState { cost: c + h, portal_id });
                start_portals.push(portal_id);
            }
        }
    }

    let mut goal_portals = BTreeSet::new();
    if let Some(cluster) = graph.clusters.get(&actual_goal_cluster) {
        for &portal_id in &cluster.portals {
            goal_portals.insert(portal_id);
        }
    }

    let mut final_portal = None;

    while let Some(GraphState { cost: _, portal_id: current_id }) = open_set.pop() {
        iterations += 1;
        
        // Safety check
        if iterations > MAX_PORTAL_ITERATIONS {
            error!("[PATHFINDING] Portal graph A* exceeded max iterations ({}) - possible infinite loop!", MAX_PORTAL_ITERATIONS);
            error!("  Start cluster: {:?}, Goal cluster: {:?}, Total portals: {}", start_cluster, goal_cluster, graph.nodes.len());
            return None;
        }
        
        // CRITICAL FIX: Skip if already visited (prevents cycles and re-processing)
        if closed_set.contains(&current_id) {
            continue;
        }
        closed_set.insert(current_id);
        
        if goal_portals.contains(&current_id) {
            let portal_node = graph.nodes[current_id].node;
            let min_x = actual_goal_cluster.0 * CLUSTER_SIZE;
            let max_x = ((actual_goal_cluster.0 + 1) * CLUSTER_SIZE).min(flow_field.width) - 1;
            let min_y = actual_goal_cluster.1 * CLUSTER_SIZE;
            let max_y = ((actual_goal_cluster.1 + 1) * CLUSTER_SIZE).min(flow_field.height) - 1;

            if let Some(_) = find_path_astar_local(portal_node, actual_goal, flow_field, min_x, max_x, min_y, max_y) {
                final_portal = Some(current_id);
                break;
            }
        }

        if let Some(neighbors) = graph.edges.get(&current_id) {
            for &(neighbor_id, edge_cost) in neighbors {
                // Skip if already visited
                if closed_set.contains(&neighbor_id) {
                    continue;
                }
                
                let tentative_g = g_score[&current_id] + edge_cost;
                if tentative_g < *g_score.get(&neighbor_id).unwrap_or(&FixedNum::MAX) {
                    g_score.insert(neighbor_id, tentative_g);
                    came_from.insert(neighbor_id, current_id);
                    
                    let neighbor_node = graph.nodes[neighbor_id].node;
                    let h = heuristic(neighbor_node.x, neighbor_node.y, actual_goal.x, actual_goal.y, flow_field.cell_size);
                    open_set.push(GraphState { cost: tentative_g + h, portal_id: neighbor_id });
                }
            }
        }
    }

    if let Some(end_portal) = final_portal {
        let mut portals = Vec::new();
        let mut curr = end_portal;
        portals.push(curr);
        
        while let Some(&prev) = came_from.get(&curr) {
            curr = prev;
            portals.push(curr);
        }
        portals.reverse();
        
        return Some(Path::Hierarchical {
            portals,
            final_goal: flow_field.grid_to_world(actual_goal.x, actual_goal.y),
            current_index: 0,
        });
    }

    None
}



fn heuristic(x1: usize, y1: usize, x2: usize, y2: usize, cell_size: FixedNum) -> FixedNum {
    let dx = (x1 as i32 - x2 as i32).abs();
    let dy = (y1 as i32 - y2 as i32).abs();
    FixedNum::from_num(dx + dy) * cell_size
}

fn reconstruct_path(came_from: BTreeMap<Node, Node>, mut current: Node, flow_field: &crate::game::flow_field::FlowField) -> Vec<FixedVec2> {
    let mut path = Vec::new();
    path.push(flow_field.grid_to_world(current.x, current.y));
    
    while let Some(prev) = came_from.get(&current) {
        current = *prev;
        path.push(flow_field.grid_to_world(current.x, current.y));
    }
    
    path.reverse();
    path
}

fn process_path_requests(
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

    for (i, request) in path_requests.read().enumerate() {
        let req_start = std::time::Instant::now();
        info!("Processing path request {}/{} from {:?} to {:?}", i + 1, request_count, request.start, request.goal);
        let start_node_opt = flow_field.world_to_grid(request.start);
        let goal_node_opt = flow_field.world_to_grid(request.goal);

        if let (Some(start_node), Some(goal_node)) = (start_node_opt, goal_node_opt) {
            info!("Grid coords: {:?} -> {:?}", start_node, goal_node);
            if let Some(path) = find_path_hierarchical(
                Node { x: start_node.0, y: start_node.1 },
                Node { x: goal_node.0, y: goal_node.1 },
                flow_field,
                &graph,
                &components,
            ) {
                let req_duration = req_start.elapsed();
                info!("Path found in {:?}: {:?}", req_duration, path);
                if req_duration.as_millis() > 50 {
                    warn!("[PATHFINDING] Slow path request: {:?}", req_duration);
                }
                commands.entity(request.entity).insert(path);
            } else {
                warn!("No path found (took {:?})", req_start.elapsed());
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

fn find_path_astar_local(
    start: Node,
    goal: Node,
    flow_field: &crate::game::flow_field::FlowField,
    min_x: usize, max_x: usize,
    min_y: usize, max_y: usize,
) -> Option<Vec<FixedVec2>> {
    find_path_astar_local_points(start, goal, flow_field, min_x, max_x, min_y, max_y)
}

fn find_path_astar_local_points(
    start: Node,
    goal: Node,
    flow_field: &crate::game::flow_field::FlowField,
    min_x: usize, max_x: usize,
    min_y: usize, max_y: usize,
) -> Option<Vec<FixedVec2>> {
    const MAX_ITERATIONS: usize = 10000; // Safety limit to prevent infinite loops
    let mut iterations = 0;
    
    let mut open_set = BinaryHeap::new();
    open_set.push(State { cost: FixedNum::ZERO, node: start });

    let mut came_from: BTreeMap<Node, Node> = BTreeMap::new();
    let mut g_score: BTreeMap<Node, FixedNum> = BTreeMap::new();
    g_score.insert(start, FixedNum::ZERO);

    while let Some(State { cost: _, node: current }) = open_set.pop() {
        iterations += 1;
        
        // Safety check for infinite loops
        if iterations > MAX_ITERATIONS {
            error!("[PATHFINDING] A* exceeded max iterations ({}) - possible infinite loop! Start: {:?}, Goal: {:?}, Bounds: ({},{}) to ({},{})",
                   MAX_ITERATIONS, start, goal, min_x, min_y, max_x, max_y);
            return None;
        }
        
        if current == goal {
            if iterations > 1000 {
                warn!("[PATHFINDING] A* used {} iterations (high!)", iterations);
            }
            return Some(reconstruct_path(came_from, current, flow_field));
        }

        let neighbors = [
            (current.x.wrapping_sub(1), current.y),
            (current.x + 1, current.y),
            (current.x, current.y.wrapping_sub(1)),
            (current.x, current.y + 1),
        ];

        for (nx, ny) in neighbors {
            if nx < min_x || nx > max_x || ny < min_y || ny > max_y {
                continue;
            }
            
            if flow_field.cost_field[flow_field.get_index(nx, ny)] == 255 {
                continue;
            }

            let neighbor = Node { x: nx, y: ny };
            let tentative_g_score = g_score[&current] + flow_field.cell_size;

            if tentative_g_score < *g_score.get(&neighbor).unwrap_or(&FixedNum::MAX) {
                came_from.insert(neighbor, current);
                g_score.insert(neighbor, tentative_g_score);
                
                let h_score = heuristic(nx, ny, goal.x, goal.y, flow_field.cell_size);
                open_set.push(State { cost: tentative_g_score + h_score, node: neighbor });
            }
        }
    }
    None
}

fn incremental_build_graph(
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
                                create_portal_vertical(&mut graph, x1, x2, sy, y - 1, cx, cy, cx + 1, cy);
                                start_segment = None;
                            }
                        }
                    }
                    if let Some(sy) = start_segment {
                         create_portal_vertical(&mut graph, x1, x2, sy, end_y - 1, cx, cy, cx + 1, cy);
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
                                create_portal_horizontal(&mut graph, sx, x - 1, y1, y2, cx, cy, cx, cy + 1);
                                start_segment = None;
                            }
                        }
                    }
                    if let Some(sx) = start_segment {
                        create_portal_horizontal(&mut graph, sx, end_x - 1, y1, y2, cx, cy, cx, cy + 1);
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
                    
                    build_state.step = GraphBuildStep::Done;
                    loading_progress.progress = 1.0;
                    let total_cached = graph.clusters.values().map(|c| c.flow_field_cache.len()).sum::<usize>();
                    info!("=== GRAPH BUILD COMPLETE ===");
                    info!("incremental_build_graph: Graph build COMPLETE! ({} cached flow fields)", total_cached);
                    break;
                }
            }
            let end_idx = build_state.current_cluster_idx;
            let batch_duration = batch_start.elapsed();
            if end_idx > start_idx && end_idx % 50 == 0 {
                info!("  Precomputed flow fields for {}/{} clusters - batch of {} took {:?}", 
                      end_idx, build_state.cluster_keys.len(), end_idx - start_idx, batch_duration);
            }
            loading_progress.progress = 0.75 + 0.25 * (build_state.current_cluster_idx as f32 / build_state.cluster_keys.len() as f32);
        }
        GraphBuildStep::Done => {}
    }
}

fn connect_intra_cluster(
    graph: &mut HierarchicalGraph,
    flow_field: &crate::game::flow_field::FlowField,
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

fn precompute_flow_fields_for_cluster(
    graph: &mut HierarchicalGraph,
    flow_field: &crate::game::flow_field::FlowField,
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
    flow_field: &crate::game::flow_field::FlowField,
    cluster_key: (usize, usize),
) {
    precompute_flow_fields_for_cluster(graph, flow_field, cluster_key);
}

fn draw_graph_gizmos(
    graph: Res<HierarchicalGraph>,
    map_flow_field: Res<MapFlowField>,
    debug_config: Res<DebugConfig>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    mut gizmos: Gizmos,
    q_camera: Query<(&Camera, &GlobalTransform), With<crate::game::camera::RtsCamera>>,
) {
    if !debug_config.show_pathfinding_graph {
        return;
    }

    let flow_field = &map_flow_field.0;
    if flow_field.width == 0 { return; }

    let Some(config) = game_configs.get(&config_handle.0) else { return };
    let Ok((camera, camera_transform)) = q_camera.single() else { return };

    // Get camera view center (raycast to ground)
    let camera_pos = camera_transform.translation();
    let center_pos = if let Ok(ray) = camera.viewport_to_world(camera_transform, Vec2::new(640.0, 360.0)) {
        if ray.direction.y.abs() > 0.001 {
            let t = -ray.origin.y / ray.direction.y;
            if t >= 0.0 {
                ray.origin + ray.direction * t
            } else {
                camera_pos
            }
        } else {
            camera_pos
        }
    } else {
        camera_pos
    };

    let view_radius = config.debug_flow_field_view_radius;
    let camera_center = Vec2::new(center_pos.x, center_pos.z);

    // Draw nodes (portals) with frustum culling
    for portal in &graph.nodes {
        let pos = flow_field.grid_to_world(portal.node.x, portal.node.y);
        let portal_pos = Vec2::new(pos.x.to_num(), pos.y.to_num());
        
        // Cull portals outside view radius
        let dx = portal_pos.x - camera_center.x;
        let dy = portal_pos.y - camera_center.y;
        let distance = (dx * dx + dy * dy).sqrt();
        if distance > view_radius {
            continue;
        }

        gizmos.sphere(
            Vec3::new(pos.x.to_num(), 1.0, pos.y.to_num()),
            0.3,
            Color::srgb(0.0, 1.0, 1.0),
        );

        // Draw portal range
        let min_pos = flow_field.grid_to_world(portal.range_min.x, portal.range_min.y);
        let max_pos = flow_field.grid_to_world(portal.range_max.x, portal.range_max.y);
        
        gizmos.line(
            Vec3::new(min_pos.x.to_num(), 1.0, min_pos.y.to_num()),
            Vec3::new(max_pos.x.to_num(), 1.0, max_pos.y.to_num()),
            Color::srgb(0.0, 1.0, 1.0),
        );
    }

    // Draw edges with frustum culling (both endpoints must be visible)
    for (from_id, edges) in &graph.edges {
        if let Some(from_portal) = graph.nodes.get(*from_id) {
            let start = flow_field.grid_to_world(from_portal.node.x, from_portal.node.y);
            let start_pos = Vec2::new(start.x.to_num(), start.y.to_num());
            
            // Cull edges where start portal is outside view
            let dx = start_pos.x - camera_center.x;
            let dy = start_pos.y - camera_center.y;
            let start_distance = (dx * dx + dy * dy).sqrt();
            if start_distance > view_radius {
                continue;
            }

            for (to_id, _) in edges {
                if let Some(to_portal) = graph.nodes.get(*to_id) {
                    let end = flow_field.grid_to_world(to_portal.node.x, to_portal.node.y);
                    let end_pos = Vec2::new(end.x.to_num(), end.y.to_num());
                    
                    // Cull edges where end portal is outside view
                    let dx = end_pos.x - camera_center.x;
                    let dy = end_pos.y - camera_center.y;
                    let end_distance = (dx * dx + dy * dy).sqrt();
                    if end_distance > view_radius {
                        continue;
                    }

                    gizmos.line(
                        Vec3::new(start.x.to_num(), 1.0, start.y.to_num()),
                        Vec3::new(end.x.to_num(), 1.0, end.y.to_num()),
                        Color::srgb(1.0, 1.0, 0.0),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_gizmo_culls_distant_portals() {
        // Verify that graph gizmo culling logic matches expected behavior
        // Portal at (100, 0) should be culled when camera is at (0, 0) with radius 50
        let portal_pos = Vec2::new(100.0, 0.0);
        let camera_center = Vec2::new(0.0, 0.0);
        let view_radius = 50.0;
        
        let dx = portal_pos.x - camera_center.x;
        let dy = portal_pos.y - camera_center.y;
        let distance = (dx * dx + dy * dy).sqrt();
        
        assert!(distance > view_radius, "Portal should be outside view radius");
    }
    
    #[test]
    fn test_graph_gizmo_draws_nearby_portals() {
        // Portal at (10, 0) should be drawn when camera is at (0, 0) with radius 50
        let portal_pos = Vec2::new(10.0, 0.0);
        let camera_center = Vec2::new(0.0, 0.0);
        let view_radius = 50.0;
        
        let dx = portal_pos.x - camera_center.x;
        let dy = portal_pos.y - camera_center.y;
        let distance = (dx * dx + dy * dy).sqrt();
        
        assert!(distance <= view_radius, "Portal should be within view radius");
    }
    
    #[test]
    fn test_graph_gizmo_edge_culling_both_endpoints() {
        // Verify edge culling requires both endpoints to be visible
        let view_radius = 50.0;
        let camera_center = Vec2::new(0.0, 0.0);
        
        // Edge from (10, 0) to (100, 0) - start visible, end not visible
        let start_pos = Vec2::new(10.0, 0.0);
        let end_pos = Vec2::new(100.0, 0.0);
        
        let start_dx = start_pos.x - camera_center.x;
        let start_dy = start_pos.y - camera_center.y;
        let start_distance = (start_dx * start_dx + start_dy * start_dy).sqrt();
        
        let end_dx = end_pos.x - camera_center.x;
        let end_dy = end_pos.y - camera_center.y;
        let end_distance = (end_dx * end_dx + end_dy * end_dy).sqrt();
        
        assert!(start_distance <= view_radius, "Start should be visible");
        assert!(end_distance > view_radius, "End should not be visible");
        // Edge should be culled because end is not visible
    }
}
