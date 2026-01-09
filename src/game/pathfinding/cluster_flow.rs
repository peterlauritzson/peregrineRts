use std::collections::VecDeque;
use crate::game::math::{FixedVec2, FixedNum};
use super::types::{LocalFlowField, Portal, CLUSTER_SIZE};

pub(super) fn generate_local_flow_field(
    cluster_id: (usize, usize),
    portal: &Portal,
    map_flow_field: &crate::game::structures::FlowField,
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
