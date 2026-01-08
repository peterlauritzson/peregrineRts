use bevy::prelude::*;
use std::collections::{BinaryHeap, BTreeMap, BTreeSet, VecDeque};
use crate::game::math::{FixedVec2, FixedNum};
use super::types::{Node, Path, State, GraphState, CLUSTER_SIZE};
use super::graph::HierarchicalGraph;
use super::components::ConnectedComponents;

/// Find the nearest walkable cell to a target node using BFS
pub(super) fn find_nearest_walkable(
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

pub(super) fn heuristic(x1: usize, y1: usize, x2: usize, y2: usize, cell_size: FixedNum) -> FixedNum {
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

pub(super) fn find_path_astar_local(
    start: Node,
    goal: Node,
    flow_field: &crate::game::flow_field::FlowField,
    min_x: usize, max_x: usize,
    min_y: usize, max_y: usize,
) -> Option<Vec<FixedVec2>> {
    find_path_astar_local_points(start, goal, flow_field, min_x, max_x, min_y, max_y)
}

pub(super) fn find_path_astar_local_points(
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

pub fn find_path_hierarchical(
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
