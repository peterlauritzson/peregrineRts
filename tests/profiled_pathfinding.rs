/// Profiled version of pathfinding for performance testing
/// NOTE: This test is outdated after routing table optimization (Jan 11, 2026)
/// TODO: Update or remove this test file - pathfinding now uses precomputed routing table
/// instead of A* between clusters, making this profiling approach obsolete.

use std::collections::{BinaryHeap, BTreeMap, BTreeSet};
use std::time::Instant;
use peregrine::game::fixed_math::FixedNum;
use peregrine::game::pathfinding::{Node, Path, CLUSTER_SIZE};
use peregrine::game::pathfinding::HierarchicalGraph;
use peregrine::game::pathfinding::ConnectedComponents;
use peregrine::game::structures::FlowField;

#[derive(Default)]
pub struct PathfindingProfile {
    pub goal_validation_ms: f32,
    pub line_of_sight_ms: f32,
    pub connectivity_check_ms: f32,
    pub local_astar_ms: f32,
    pub portal_graph_astar_ms: f32,
    pub flow_field_lookup_ms: f32,
}

// State types for A* (copied from pathfinding module)
#[derive(Copy, Clone, PartialEq, Eq)]
struct State {
    cost: FixedNum,
    node: Node,
}

impl Ord for State {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.cost.cmp(&self.cost)
    }
}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
struct GraphState {
    cost: FixedNum,
    portal_id: usize,
}

impl Ord for GraphState {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.cost.cmp(&self.cost)
            .then_with(|| self.portal_id.cmp(&other.portal_id))
    }
}

impl PartialOrd for GraphState {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/*
// COMMENTED OUT: This function is outdated after routing table optimization
// The pathfinding system now uses precomputed cluster routing tables instead
// of running A* between clusters at pathfinding time.

pub fn find_path_hierarchical_profiled(
    start: Node,
    goal: Node,
    flow_field: &FlowField,
    graph: &HierarchicalGraph,
    components: &ConnectedComponents,
) -> (Option<Path>, PathfindingProfile) {
    let mut profile = PathfindingProfile::default();
    
    // 1. Goal Validation
    let t = Instant::now();
    let goal_idx = flow_field.get_index(goal.x, goal.y);
    let actual_goal = if flow_field.cost_field[goal_idx] == 255 {
        if let Some(walkable_goal) = find_nearest_walkable(goal, flow_field) {
            walkable_goal
        } else {
            profile.goal_validation_ms = t.elapsed().as_secs_f32() * 1000.0;
            return (None, profile);
        }
    } else {
        goal
    };
    profile.goal_validation_ms = t.elapsed().as_secs_f32() * 1000.0;

    // 2. Line of Sight Check
    let t = Instant::now();
    if has_line_of_sight(start, actual_goal, flow_field) {
        profile.line_of_sight_ms = t.elapsed().as_secs_f32() * 1000.0;
        return (Some(Path::Direct(flow_field.grid_to_world(actual_goal.x, actual_goal.y))), profile);
    }
    profile.line_of_sight_ms = t.elapsed().as_secs_f32() * 1000.0;

    let start_cluster = (start.x / CLUSTER_SIZE, start.y / CLUSTER_SIZE);
    let goal_cluster = (actual_goal.x / CLUSTER_SIZE, actual_goal.y / CLUSTER_SIZE);

    // 3. Connectivity Check
    let t = Instant::now();
    let actual_goal_after_connectivity = if components.initialized && !components.are_connected(start_cluster, goal_cluster) {
        if let Some(fallback_portals) = components.get_fallback_portals(start_cluster, goal_cluster) {
            if let Some(&fallback_portal_id) = fallback_portals.first() {
                let fallback_portal = &graph.nodes[fallback_portal_id];
                fallback_portal.node
            } else {
                actual_goal
            }
        } else {
            actual_goal
        }
    } else {
        actual_goal
    };
    profile.connectivity_check_ms = t.elapsed().as_secs_f32() * 1000.0;

    let actual_goal_cluster = (actual_goal_after_connectivity.x / CLUSTER_SIZE, actual_goal_after_connectivity.y / CLUSTER_SIZE);

    // 4. Local A* (same cluster)
    let t = Instant::now();
    if start_cluster == actual_goal_cluster {
        let min_x = start_cluster.0 * CLUSTER_SIZE;
        let max_x = ((start_cluster.0 + 1) * CLUSTER_SIZE).min(flow_field.width) - 1;
        let min_y = start_cluster.1 * CLUSTER_SIZE;
        let max_y = ((start_cluster.1 + 1) * CLUSTER_SIZE).min(flow_field.height) - 1;
        
        if let Some(points) = find_path_astar_local(start, actual_goal_after_connectivity, flow_field, min_x, max_x, min_y, max_y) {
            profile.local_astar_ms = t.elapsed().as_secs_f32() * 1000.0;
            return (Some(Path::LocalAStar { waypoints: points, current_index: 0 }), profile);
        }
    }

    // 5. Local A* (close distance)
    let dist_sq = (start.x as i32 - actual_goal_after_connectivity.x as i32).pow(2) + 
                  (start.y as i32 - actual_goal_after_connectivity.y as i32).pow(2);
    let threshold = (CLUSTER_SIZE as i32 * 2).pow(2);

    if dist_sq < threshold {
        let min_x = start.x.min(actual_goal_after_connectivity.x).saturating_sub(CLUSTER_SIZE).max(0);
        let max_x = start.x.max(actual_goal_after_connectivity.x).saturating_add(CLUSTER_SIZE).min(flow_field.width - 1);
        let min_y = start.y.min(actual_goal_after_connectivity.y).saturating_sub(CLUSTER_SIZE).max(0);
        let max_y = start.y.max(actual_goal_after_connectivity.y).saturating_add(CLUSTER_SIZE).min(flow_field.height - 1);

        if let Some(points) = find_path_astar_local(start, actual_goal_after_connectivity, flow_field, min_x, max_x, min_y, max_y) {
            profile.local_astar_ms = t.elapsed().as_secs_f32() * 1000.0;
            return (Some(Path::LocalAStar { waypoints: points, current_index: 0 }), profile);
        }
    }
    profile.local_astar_ms = t.elapsed().as_secs_f32() * 1000.0;

    // 6. Portal Graph A* + Flow Field Lookups
    let t_portal = Instant::now();
    let t_ff = Instant::now();
    
    let mut open_set = BinaryHeap::new();
    let mut came_from: BTreeMap<usize, usize> = BTreeMap::new();
    let mut g_score: BTreeMap<usize, FixedNum> = BTreeMap::new();
    let mut closed_set: BTreeSet<usize> = BTreeSet::new();
    
    const MAX_PORTAL_ITERATIONS: usize = 1000;
    let mut iterations = 0;

    // Find start portals (with flow field lookups)
    let mut flow_field_time = 0.0;
    if let Some(cluster) = graph.clusters.get(&start_cluster) {
        for &portal_id in &cluster.portals {
            let portal_node = graph.nodes[portal_id].node;
            let mut cost = None;

            // Flow field lookup timing
            let t_ff_lookup = Instant::now();
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
            flow_field_time += t_ff_lookup.elapsed().as_secs_f32() * 1000.0;

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
                let h = heuristic(portal_node.x, portal_node.y, actual_goal_after_connectivity.x, actual_goal_after_connectivity.y, flow_field.cell_size);
                open_set.push(GraphState { cost: c + h, portal_id });
            }
        }
    }

    // Find goal portals
    let mut goal_portals = BTreeSet::new();
    if let Some(cluster) = graph.clusters.get(&actual_goal_cluster) {
        for &portal_id in &cluster.portals {
            goal_portals.insert(portal_id);
        }
    }

    let mut final_portal = None;

    // Portal graph search
    while let Some(GraphState { cost: _, portal_id: current_id }) = open_set.pop() {
        iterations += 1;
        
        if iterations > MAX_PORTAL_ITERATIONS {
            break;
        }
        
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

            if let Some(_) = find_path_astar_local(portal_node, actual_goal_after_connectivity, flow_field, min_x, max_x, min_y, max_y) {
                final_portal = Some(current_id);
                break;
            }
        }

        if let Some(neighbors) = graph.edges.get(&current_id) {
            for &(neighbor_id, edge_cost) in neighbors {
                if closed_set.contains(&neighbor_id) {
                    continue;
                }
                
                let tentative_g = g_score[&current_id] + edge_cost;
                if tentative_g < *g_score.get(&neighbor_id).unwrap_or(&FixedNum::MAX) {
                    g_score.insert(neighbor_id, tentative_g);
                    came_from.insert(neighbor_id, current_id);
                    
                    let neighbor_node = graph.nodes[neighbor_id].node;
                    let h = heuristic(neighbor_node.x, neighbor_node.y, actual_goal_after_connectivity.x, actual_goal_after_connectivity.y, flow_field.cell_size);
                    open_set.push(GraphState { cost: tentative_g + h, portal_id: neighbor_id });
                }
            }
        }
    }

    profile.portal_graph_astar_ms = t_portal.elapsed().as_secs_f32() * 1000.0;
    profile.flow_field_lookup_ms = flow_field_time;

    if let Some(end_portal) = final_portal {
        let mut portals = Vec::new();
        let mut curr = end_portal;
        portals.push(curr);
        
        while let Some(&prev) = came_from.get(&curr) {
            curr = prev;
            portals.push(curr);
        }
        portals.reverse();
        
        return (Some(Path::Hierarchical {
            portals,
            final_goal: flow_field.grid_to_world(actual_goal_after_connectivity.x, actual_goal_after_connectivity.y),
            current_index: 0,
        }), profile);
    }

    (None, profile)
}
*/
