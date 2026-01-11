use bevy::prelude::*;
use std::collections::{BinaryHeap, BTreeMap};
use crate::game::fixed_math::{FixedVec2, FixedNum};
use super::types::{Node, State};

pub(super) fn heuristic(x1: usize, y1: usize, x2: usize, y2: usize, cell_size: FixedNum) -> FixedNum {
    let dx = (x1 as i32 - x2 as i32).abs();
    let dy = (y1 as i32 - y2 as i32).abs();
    FixedNum::from_num(dx + dy) * cell_size
}

fn reconstruct_path(came_from: BTreeMap<Node, Node>, mut current: Node, flow_field: &crate::game::structures::FlowField) -> Vec<FixedVec2> {
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
    flow_field: &crate::game::structures::FlowField,
    min_x: usize, max_x: usize,
    min_y: usize, max_y: usize,
) -> Option<Vec<FixedVec2>> {
    find_path_astar_local_points(start, goal, flow_field, min_x, max_x, min_y, max_y)
}

pub(super) fn find_path_astar_local_points(
    start: Node,
    goal: Node,
    flow_field: &crate::game::structures::FlowField,
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
