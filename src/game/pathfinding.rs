use bevy::prelude::*;
use crate::game::math::{FixedVec2, FixedNum};
use crate::game::simulation::{MapFlowField, DebugConfig};
use crate::game::GameState;
use crate::game::loading::{LoadingProgress, TargetGameState};
use std::collections::{BinaryHeap, BTreeMap, BTreeSet, VecDeque};
use std::cmp::Ordering;
use serde::{Serialize, Deserialize};

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

#[derive(Resource, Default, Serialize, Deserialize)]
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
/// - **Cache invalidation:** Clusters clear cache when obstacles added (see [`clear_cache`])
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
        app.init_resource::<GraphBuildState>();
        // Removed synchronous build_graph system that froze the game for 10+ seconds on large maps
        app.add_systems(Update, (draw_graph_gizmos).run_if(in_state(GameState::InGame)));
        app.add_systems(FixedUpdate, process_path_requests.run_if(in_state(GameState::InGame).or(in_state(GameState::Editor))));
        app.add_systems(Update, incremental_build_graph.run_if(in_state(GameState::Loading).or(in_state(GameState::Editor))));
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
    ) -> &LocalFlowField {
        self.flow_field_cache.get(&portal_id).expect("Flow field not found in cache")
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

fn find_path_hierarchical(
    start: Node,
    goal: Node,
    flow_field: &crate::game::flow_field::FlowField,
    graph: &HierarchicalGraph
) -> Option<Path> {
    // 1. Check Line of Sight
    if has_line_of_sight(start, goal, flow_field) {
        return Some(Path::Direct(flow_field.grid_to_world(goal.x, goal.y)));
    }

    let start_cluster = (start.x / CLUSTER_SIZE, start.y / CLUSTER_SIZE);
    let goal_cluster = (goal.x / CLUSTER_SIZE, goal.y / CLUSTER_SIZE);

    // If in same cluster, use local A*
    if start_cluster == goal_cluster {
        let min_x = start_cluster.0 * CLUSTER_SIZE;
        let max_x = ((start_cluster.0 + 1) * CLUSTER_SIZE).min(flow_field.width) - 1;
        let min_y = start_cluster.1 * CLUSTER_SIZE;
        let max_y = ((start_cluster.1 + 1) * CLUSTER_SIZE).min(flow_field.height) - 1;
        
        if let Some(points) = find_path_astar_local_points(start, goal, flow_field, min_x, max_x, min_y, max_y) {
            return Some(Path::LocalAStar { waypoints: points, current_index: 0 });
        }
    }

    // 2. Check if close enough for local A* even if different clusters
    let dist_sq = (start.x as i32 - goal.x as i32).pow(2) + (start.y as i32 - goal.y as i32).pow(2);
    let threshold = (CLUSTER_SIZE as i32 * 2).pow(2); 

    if dist_sq < threshold {
         let min_x = start.x.min(goal.x).saturating_sub(CLUSTER_SIZE);
         let max_x = start.x.max(goal.x) + CLUSTER_SIZE;
         let min_y = start.y.min(goal.y).saturating_sub(CLUSTER_SIZE);
         let max_y = start.y.max(goal.y) + CLUSTER_SIZE;
         
         let min_x = min_x.max(0);
         let max_x = max_x.min(flow_field.width - 1);
         let min_y = min_y.max(0);
         let max_y = max_y.min(flow_field.height - 1);

         if let Some(points) = find_path_astar_local_points(start, goal, flow_field, min_x, max_x, min_y, max_y) {
             return Some(Path::LocalAStar { waypoints: points, current_index: 0 });
         }
    }

    let mut open_set = BinaryHeap::new();
    let mut came_from: BTreeMap<usize, usize> = BTreeMap::new();
    let mut g_score: BTreeMap<usize, FixedNum> = BTreeMap::new();

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
                let h = heuristic(portal_node.x, portal_node.y, goal.x, goal.y, flow_field.cell_size);
                open_set.push(GraphState { cost: c + h, portal_id });
                start_portals.push(portal_id);
            }
        }
    }

    let mut goal_portals = BTreeSet::new();
    if let Some(cluster) = graph.clusters.get(&goal_cluster) {
        for &portal_id in &cluster.portals {
            goal_portals.insert(portal_id);
        }
    }

    let mut final_portal = None;

    while let Some(GraphState { cost: _, portal_id: current_id }) = open_set.pop() {
        if goal_portals.contains(&current_id) {
            let portal_node = graph.nodes[current_id].node;
            let min_x = goal_cluster.0 * CLUSTER_SIZE;
            let max_x = ((goal_cluster.0 + 1) * CLUSTER_SIZE).min(flow_field.width) - 1;
            let min_y = goal_cluster.1 * CLUSTER_SIZE;
            let max_y = ((goal_cluster.1 + 1) * CLUSTER_SIZE).min(flow_field.height) - 1;

            if let Some(_) = find_path_astar_local(portal_node, goal, flow_field, min_x, max_x, min_y, max_y) {
                final_portal = Some(current_id);
                break;
            }
        }

        if let Some(neighbors) = graph.edges.get(&current_id) {
            for &(neighbor_id, edge_cost) in neighbors {
                let tentative_g = g_score[&current_id] + edge_cost;
                if tentative_g < *g_score.get(&neighbor_id).unwrap_or(&FixedNum::MAX) {
                    g_score.insert(neighbor_id, tentative_g);
                    came_from.insert(neighbor_id, current_id);
                    
                    let neighbor_node = graph.nodes[neighbor_id].node;
                    let h = heuristic(neighbor_node.x, neighbor_node.y, goal.x, goal.y, flow_field.cell_size);
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
            final_goal: flow_field.grid_to_world(goal.x, goal.y),
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
) {
    if path_requests.is_empty() {
        return;
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

    for request in path_requests.read() {
        info!("Processing path request from {:?} to {:?}", request.start, request.goal);
        let start_node_opt = flow_field.world_to_grid(request.start);
        let goal_node_opt = flow_field.world_to_grid(request.goal);

        if let (Some(start_node), Some(goal_node)) = (start_node_opt, goal_node_opt) {
            info!("Grid coords: {:?} -> {:?}", start_node, goal_node);
            if let Some(path) = find_path_hierarchical(
                Node { x: start_node.0, y: start_node.1 },
                Node { x: goal_node.0, y: goal_node.1 },
                flow_field,
                &graph
            ) {
                info!("Path found: {:?}", path);
                commands.entity(request.entity).insert(path);
            } else {
                warn!("No path found");
            }
        } else {
            warn!("Start or goal out of bounds");
        }
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
    let mut open_set = BinaryHeap::new();
    open_set.push(State { cost: FixedNum::ZERO, node: start });

    let mut came_from: BTreeMap<Node, Node> = BTreeMap::new();
    let mut g_score: BTreeMap<Node, FixedNum> = BTreeMap::new();
    g_score.insert(start, FixedNum::ZERO);

    while let Some(State { cost: _, node: current }) = open_set.pop() {
        if current == goal {
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
) {
    let flow_field = &map_flow_field.0;
    if flow_field.width == 0 { return; }

    let width_clusters = (flow_field.width + CLUSTER_SIZE - 1) / CLUSTER_SIZE;
    let height_clusters = (flow_field.height + CLUSTER_SIZE - 1) / CLUSTER_SIZE;

    match build_state.step {
        GraphBuildStep::NotStarted => {
            loading_progress.task = "Initializing Graph...".to_string();
            loading_progress.progress = 0.0;
            build_state.step = GraphBuildStep::InitializingClusters;
        }
        GraphBuildStep::InitializingClusters => {
            for cy in 0..height_clusters {
                for cx in 0..width_clusters {
                    graph.clusters.insert((cx, cy), Cluster {
                        id: (cx, cy),
                        portals: Vec::new(),
                        flow_field_cache: BTreeMap::new(),
                    });
                }
            }
            build_state.cx = 0;
            build_state.cy = 0;
            build_state.step = GraphBuildStep::FindingVerticalPortals;
            loading_progress.progress = 0.1;
        }
        GraphBuildStep::FindingVerticalPortals => {
            loading_progress.task = "Finding Vertical Portals...".to_string();
            let cx = build_state.cx;
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
                build_state.cluster_keys = graph.clusters.keys().cloned().collect();
                build_state.current_cluster_idx = 0;
                build_state.step = GraphBuildStep::ConnectingIntraCluster;
            }
        }
        GraphBuildStep::ConnectingIntraCluster => {
            loading_progress.task = "Connecting Intra-Cluster...".to_string();
            let batch_size = 5; 
            for _ in 0..batch_size {
                if build_state.current_cluster_idx < build_state.cluster_keys.len() {
                    let key = build_state.cluster_keys[build_state.current_cluster_idx];
                    connect_intra_cluster(&mut graph, flow_field, key);
                    build_state.current_cluster_idx += 1;
                } else {
                    build_state.current_cluster_idx = 0;
                    build_state.step = GraphBuildStep::PrecomputingFlowFields;
                    break;
                }
            }
            loading_progress.progress = 0.5 + 0.25 * (build_state.current_cluster_idx as f32 / build_state.cluster_keys.len() as f32);
        }
        GraphBuildStep::PrecomputingFlowFields => {
            loading_progress.task = "Precomputing Flow Fields...".to_string();
            let batch_size = 5;
            for _ in 0..batch_size {
                if build_state.current_cluster_idx < build_state.cluster_keys.len() {
                    let key = build_state.cluster_keys[build_state.current_cluster_idx];
                    precompute_flow_fields_for_cluster(&mut graph, flow_field, key);
                    build_state.current_cluster_idx += 1;
                } else {
                    graph.initialized = true;
                    build_state.step = GraphBuildStep::Done;
                    loading_progress.progress = 1.0;
                    break;
                }
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
    let portals = graph.clusters[&key].portals.clone();
    for portal_id in portals {
        if let Some(portal) = graph.nodes.get(portal_id).cloned() {
            let field = generate_local_flow_field(key, &portal, flow_field);
            if let Some(cluster) = graph.clusters.get_mut(&key) {
                cluster.flow_field_cache.insert(portal_id, field);
            }
        }
    }
}

fn draw_graph_gizmos(
    graph: Res<HierarchicalGraph>,
    map_flow_field: Res<MapFlowField>,
    debug_config: Res<DebugConfig>,
    mut gizmos: Gizmos,
) {
    if !debug_config.show_pathfinding_graph {
        return;
    }

    let flow_field = &map_flow_field.0;
    if flow_field.width == 0 { return; }

    // Draw nodes
    for portal in &graph.nodes {
        let pos = flow_field.grid_to_world(portal.node.x, portal.node.y);
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

    // Draw edges
    for (from_id, edges) in &graph.edges {
        if let Some(from_portal) = graph.nodes.get(*from_id) {
            let start = flow_field.grid_to_world(from_portal.node.x, from_portal.node.y);
            for (to_id, _) in edges {
                if let Some(to_portal) = graph.nodes.get(*to_id) {
                    let end = flow_field.grid_to_world(to_portal.node.x, to_portal.node.y);
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
