use bevy::prelude::*;
use crate::game::math::{FixedVec2, FixedNum};
use crate::game::simulation::{MapFlowField, DebugConfig};
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::cmp::Ordering;

const CLUSTER_SIZE: usize = 10;

#[derive(Event, Message, Debug, Clone)]
pub struct PathRequest {
    pub entity: Entity,
    #[allow(dead_code)]
    pub start: FixedVec2,
    pub goal: FixedVec2,
}

#[derive(Component, Debug, Clone, Default)]
pub struct Path {
    pub waypoints: Vec<FixedVec2>,
    pub current_index: usize,
}

#[derive(Resource, Default)]
pub struct HierarchicalGraph {
    pub nodes: Vec<Portal>,
    pub edges: HashMap<usize, Vec<(usize, FixedNum)>>, // PortalId -> [(TargetPortalId, Cost)]
    pub cluster_portals: HashMap<(usize, usize), Vec<usize>>,
    pub initialized: bool,
}

impl HierarchicalGraph {
    pub fn reset(&mut self) {
        self.nodes.clear();
        self.edges.clear();
        self.cluster_portals.clear();
        self.initialized = false;
    }
}

#[derive(Clone, Debug)]
pub struct Portal {
    #[allow(dead_code)]
    pub id: usize,
    pub node: Node,
    #[allow(dead_code)]
    pub cluster: (usize, usize),
}

pub struct PathfindingPlugin;

impl Plugin for PathfindingPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PathRequest>();
        app.init_resource::<HierarchicalGraph>();
        app.add_systems(Update, (build_graph, draw_graph_gizmos));
        app.add_systems(FixedUpdate, process_path_requests);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
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

fn build_graph(
    mut graph: ResMut<HierarchicalGraph>,
    map_flow_field: Res<MapFlowField>,
) {
    if graph.initialized {
        return;
    }
    let flow_field = &map_flow_field.0;
    if flow_field.width == 0 { return; }

    info!("Building Hierarchical Graph...");
    
    let width_clusters = (flow_field.width + CLUSTER_SIZE - 1) / CLUSTER_SIZE;
    let height_clusters = (flow_field.height + CLUSTER_SIZE - 1) / CLUSTER_SIZE;

    let mut cluster_portals: HashMap<(usize, usize), Vec<usize>> = HashMap::new();

    // 1. Find Portals (Inter-cluster edges)
    
    // Vertical edges (Right neighbors)
    for cx in 0..width_clusters - 1 {
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
                        create_portal_vertical(&mut graph, &mut cluster_portals, x1, x2, sy, y - 1, cx, cy, cx + 1, cy);
                        start_segment = None;
                    }
                }
            }
            if let Some(sy) = start_segment {
                 create_portal_vertical(&mut graph, &mut cluster_portals, x1, x2, sy, end_y - 1, cx, cy, cx + 1, cy);
            }
        }
    }

    // Horizontal edges (Down neighbors)
    for cx in 0..width_clusters {
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
                        create_portal_horizontal(&mut graph, &mut cluster_portals, sx, x - 1, y1, y2, cx, cy, cx, cy + 1);
                        start_segment = None;
                    }
                }
            }
            if let Some(sx) = start_segment {
                create_portal_horizontal(&mut graph, &mut cluster_portals, sx, end_x - 1, y1, y2, cx, cy, cx, cy + 1);
            }
        }
    }

    // 2. Intra-Cluster Edges
    for ((cx, cy), portals) in cluster_portals.iter() {
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

    graph.cluster_portals = cluster_portals;
    graph.initialized = true;
    info!("Hierarchical Graph Built. Nodes: {}, Edges: {}", graph.nodes.len(), graph.edges.values().map(|v| v.len()).sum::<usize>());
}

fn create_portal_vertical(
    graph: &mut HierarchicalGraph,
    cluster_portals: &mut HashMap<(usize, usize), Vec<usize>>,
    x1: usize, x2: usize,
    y_start: usize, y_end: usize,
    c1x: usize, c1y: usize,
    c2x: usize, c2y: usize,
) {
    let mid_y = (y_start + y_end) / 2;
    
    let id1 = graph.nodes.len();
    graph.nodes.push(Portal { id: id1, node: Node { x: x1, y: mid_y }, cluster: (c1x, c1y) });
    cluster_portals.entry((c1x, c1y)).or_default().push(id1);

    let id2 = graph.nodes.len();
    graph.nodes.push(Portal { id: id2, node: Node { x: x2, y: mid_y }, cluster: (c2x, c2y) });
    cluster_portals.entry((c2x, c2y)).or_default().push(id2);

    let cost = FixedNum::from_num(1.0);
    graph.edges.entry(id1).or_default().push((id2, cost));
    graph.edges.entry(id2).or_default().push((id1, cost));
}

fn create_portal_horizontal(
    graph: &mut HierarchicalGraph,
    cluster_portals: &mut HashMap<(usize, usize), Vec<usize>>,
    x_start: usize, x_end: usize,
    y1: usize, y2: usize,
    c1x: usize, c1y: usize,
    c2x: usize, c2y: usize,
) {
    let mid_x = (x_start + x_end) / 2;
    
    let id1 = graph.nodes.len();
    graph.nodes.push(Portal { id: id1, node: Node { x: mid_x, y: y1 }, cluster: (c1x, c1y) });
    cluster_portals.entry((c1x, c1y)).or_default().push(id1);

    let id2 = graph.nodes.len();
    graph.nodes.push(Portal { id: id2, node: Node { x: mid_x, y: y2 }, cluster: (c2x, c2y) });
    cluster_portals.entry((c2x, c2y)).or_default().push(id2);

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
) -> Option<Vec<FixedVec2>> {
    // 1. Check Line of Sight
    if has_line_of_sight(start, goal, flow_field) {
        return Some(vec![
            flow_field.grid_to_world(start.x, start.y),
            flow_field.grid_to_world(goal.x, goal.y)
        ]);
    }

    let start_cluster = (start.x / CLUSTER_SIZE, start.y / CLUSTER_SIZE);
    let goal_cluster = (goal.x / CLUSTER_SIZE, goal.y / CLUSTER_SIZE);

    // If in same cluster, use local A*
    if start_cluster == goal_cluster {
        let min_x = start_cluster.0 * CLUSTER_SIZE;
        let max_x = ((start_cluster.0 + 1) * CLUSTER_SIZE).min(flow_field.width) - 1;
        let min_y = start_cluster.1 * CLUSTER_SIZE;
        let max_y = ((start_cluster.1 + 1) * CLUSTER_SIZE).min(flow_field.height) - 1;
        
        return find_path_astar_local_points(start, goal, flow_field, min_x, max_x, min_y, max_y);
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

         if let Some(path) = find_path_astar_local_points(start, goal, flow_field, min_x, max_x, min_y, max_y) {
             return Some(path);
         }
    }

    let mut open_set = BinaryHeap::new();
    let mut came_from: HashMap<usize, usize> = HashMap::new();
    let mut g_score: HashMap<usize, FixedNum> = HashMap::new();

    // Add start node connections to graph
    // Find portals in start cluster
    let mut start_portals = Vec::new();
    if let Some(portals) = graph.cluster_portals.get(&start_cluster) {
        for &portal_id in portals {
            let portal_node = graph.nodes[portal_id].node;
            let min_x = start_cluster.0 * CLUSTER_SIZE;
            let max_x = ((start_cluster.0 + 1) * CLUSTER_SIZE).min(flow_field.width) - 1;
            let min_y = start_cluster.1 * CLUSTER_SIZE;
            let max_y = ((start_cluster.1 + 1) * CLUSTER_SIZE).min(flow_field.height) - 1;

            if let Some(path) = find_path_astar_local(start, portal_node, flow_field, min_x, max_x, min_y, max_y) {
                let cost = FixedNum::from_num(path.len() as f64); // Approximate cost
                g_score.insert(portal_id, cost);
                let h = heuristic(portal_node.x, portal_node.y, goal.x, goal.y, flow_field.cell_size);
                open_set.push(GraphState { cost: cost + h, portal_id });
                start_portals.push(portal_id);
            }
        }
    }

    let mut goal_portals = HashSet::new();
    if let Some(portals) = graph.cluster_portals.get(&goal_cluster) {
        for &portal_id in portals {
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
        let mut path = Vec::new();
        path.push(flow_field.grid_to_world(goal.x, goal.y));
        
        let mut curr = end_portal;
        path.push(flow_field.grid_to_world(graph.nodes[curr].node.x, graph.nodes[curr].node.y));
        
        while let Some(&prev) = came_from.get(&curr) {
            curr = prev;
            path.push(flow_field.grid_to_world(graph.nodes[curr].node.x, graph.nodes[curr].node.y));
        }
        
        path.reverse();
        return Some(path);
    }

    None
}



fn heuristic(x1: usize, y1: usize, x2: usize, y2: usize, cell_size: FixedNum) -> FixedNum {
    let dx = (x1 as i32 - x2 as i32).abs();
    let dy = (y1 as i32 - y2 as i32).abs();
    FixedNum::from_num(dx + dy) * cell_size
}

fn reconstruct_path(came_from: HashMap<Node, Node>, mut current: Node, flow_field: &crate::game::flow_field::FlowField) -> Vec<FixedVec2> {
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
            if let Some(waypoints) = find_path_hierarchical(
                Node { x: start_node.0, y: start_node.1 },
                Node { x: goal_node.0, y: goal_node.1 },
                flow_field,
                &graph
            ) {
                info!("Path found with {} waypoints: {:?}", waypoints.len(), waypoints);
                commands.entity(request.entity).insert(Path {
                    waypoints,
                    current_index: 0,
                });
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

    let mut came_from: HashMap<Node, Node> = HashMap::new();
    let mut g_score: HashMap<Node, FixedNum> = HashMap::new();
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
