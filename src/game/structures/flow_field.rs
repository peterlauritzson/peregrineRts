use crate::game::fixed_math::{FixedNum, FixedVec2};
use bevy::prelude::*;
use std::collections::VecDeque;
use serde::{Serialize, Deserialize};

/// Fixed cell size for the flow field grid (1 world unit per cell).
pub const CELL_SIZE: f32 = 1.0;

/// Flow field navigation grid using Dijkstra-based integration and vector fields.
///
/// A flow field guides units to a target by precomputing the optimal direction
/// for each grid cell. This enables thousands of units to navigate efficiently
/// without individual pathfinding.
///
/// # Algorithm
///
/// 1. **Cost Field:** Mark obstacles (255) and walkable cells (1)
/// 2. **Integration Field:** Dijkstra flood-fill from target, storing distance
/// 3. **Vector Field:** Gradient descent - each cell points toward lowest neighbor
///
/// # Use Cases
///
/// - **Large unit groups:** Single flow field can guide unlimited units
/// - **Static targets:** Precompute once, reuse for all units going to same location
/// - **Dynamic obstacles:** Regenerate affected areas when obstacles change
///
/// # Example
///
/// ```rust,ignore
/// let mut flow_field = FlowField::new(width, height, cell_size, origin);
///
/// // Mark obstacles
/// flow_field.set_obstacle(x, y);
///
/// // Generate fields for target
/// flow_field.generate_integration_field(target_x, target_y);
/// flow_field.generate_vector_field();
///
/// // Query direction at world position
/// if let Some((gx, gy)) = flow_field.world_to_grid(unit_pos) {
///     let direction = flow_field.vector_field[flow_field.get_index(gx, gy)];
/// }
/// ```
///
/// # Performance
///
/// - **Generation:** O(width × height) via breadth-first search
/// - **Query:** O(1) array lookup
/// - **Memory:** ~20 bytes per cell (cost + integration + vector)
///
/// # See Also
///
/// - Hierarchical pathfinding uses multiple local flow fields per cluster
/// - Map-wide flow field in `MapFlowField` resource for obstacle tracking
#[derive(Resource, Default, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct FlowField {
    pub width: usize,
    pub height: usize,
    pub cell_size: FixedNum,
    pub origin: FixedVec2, // Bottom-left corner of the grid in world space
    pub cost_field: Vec<u8>, // 1 = walkable, 255 = obstacle
    pub integration_field: Vec<u32>, // Distance to target
    pub vector_field: Vec<FixedVec2>, // Direction to move
    pub target_cell: Option<(usize, usize)>,
}

#[allow(dead_code)]
impl FlowField {
    pub fn new(width: usize, height: usize, cell_size: FixedNum, origin: FixedVec2) -> Self {
        let size = width * height;
        Self {
            width,
            height,
            cell_size,
            origin,
            cost_field: vec![1; size],
            integration_field: vec![u32::MAX; size],
            vector_field: vec![FixedVec2::ZERO; size],
            target_cell: None,
        }
    }

    pub fn world_to_grid(&self, world_pos: FixedVec2) -> Option<(usize, usize)> {
        let local_pos = world_pos - self.origin;
        if local_pos.x < FixedNum::ZERO || local_pos.y < FixedNum::ZERO {
            return None;
        }

        let x = (local_pos.x / self.cell_size).to_num::<usize>();
        let y = (local_pos.y / self.cell_size).to_num::<usize>();

        if x < self.width && y < self.height {
            Some((x, y))
        } else {
            None
        }
    }

    pub fn grid_to_world(&self, x: usize, y: usize) -> FixedVec2 {
        let offset = self.cell_size / FixedNum::from_num(2.0);
        self.origin + FixedVec2::new(
            FixedNum::from_num(x) * self.cell_size + offset,
            FixedNum::from_num(y) * self.cell_size + offset,
        )
    }

    pub fn get_index(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    pub fn set_obstacle(&mut self, x: usize, y: usize) {
        let idx = self.get_index(x, y);
        self.cost_field[idx] = 255;
    }

    pub fn generate_integration_field(&mut self, target_x: usize, target_y: usize) {
        self.target_cell = Some((target_x, target_y));
        self.integration_field.fill(u32::MAX);
        
        let target_idx = self.get_index(target_x, target_y);
        self.integration_field[target_idx] = 0;

        let mut queue = VecDeque::new();
        queue.push_back((target_x, target_y));

        while let Some((cx, cy)) = queue.pop_front() {
            let c_idx = self.get_index(cx, cy);
            let current_cost = self.integration_field[c_idx];

            // Check neighbors (up, down, left, right)
            let neighbors = [
                (cx.wrapping_sub(1), cy), // Left
                (cx + 1, cy),             // Right
                (cx, cy.wrapping_sub(1)), // Down
                (cx, cy + 1),             // Up
            ];

            for (nx, ny) in neighbors {
                if nx >= self.width || ny >= self.height {
                    continue;
                }

                let n_idx = self.get_index(nx, ny);
                let cost = self.cost_field[n_idx];

                if cost == 255 {
                    continue; // Obstacle
                }

                let new_cost = current_cost + cost as u32;
                if new_cost < self.integration_field[n_idx] {
                    self.integration_field[n_idx] = new_cost;
                    queue.push_back((nx, ny));
                }
            }
        }
    }

    pub fn generate_vector_field(&mut self) {
        for y in 0..self.height {
            for x in 0..self.width {
                let idx = self.get_index(x, y);
                
                if self.cost_field[idx] == 255 {
                    self.vector_field[idx] = FixedVec2::ZERO;
                    continue;
                }

                if self.integration_field[idx] == u32::MAX {
                    self.vector_field[idx] = FixedVec2::ZERO;
                    continue;
                }
                
                // If this is the target, velocity is zero (or handle arrival logic elsewhere)
                if let Some((tx, ty)) = self.target_cell {
                    if x == tx && y == ty {
                        self.vector_field[idx] = FixedVec2::ZERO;
                        continue;
                    }
                }

                let mut best_cost = self.integration_field[idx];
                let mut best_dir = FixedVec2::ZERO;

                // Check all 8 neighbors for smoother flow
                let neighbors = [
                    (-1, 0), (1, 0), (0, -1), (0, 1), // Cardinals
                    (-1, -1), (-1, 1), (1, -1), (1, 1) // Diagonals
                ];

                for (dx, dy) in neighbors {
                    let nx = x as isize + dx;
                    let ny = y as isize + dy;

                    if nx < 0 || nx >= self.width as isize || ny < 0 || ny >= self.height as isize {
                        continue;
                    }

                    let n_idx = self.get_index(nx as usize, ny as usize);
                    let n_cost = self.integration_field[n_idx];

                    if n_cost < best_cost {
                        best_cost = n_cost;
                        best_dir = FixedVec2::new(FixedNum::from_num(dx), FixedNum::from_num(dy));
                    }
                }

                if best_dir != FixedVec2::ZERO {
                    self.vector_field[idx] = best_dir.normalize();
                } else {
                    self.vector_field[idx] = FixedVec2::ZERO;
                }
            }
        }
    }
}
