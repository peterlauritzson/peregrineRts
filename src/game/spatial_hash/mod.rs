use bevy::prelude::*;
use crate::game::fixed_math::{FixedNum, FixedVec2};

mod grid;
mod query;
#[cfg(test)]
mod tests;

/// Spatial partitioning grid for efficient proximity queries in 2D space.
///
/// The spatial hash divides the game world into a uniform grid of cells, allowing
/// O(1) amortized insertion and efficient proximity queries by only checking
/// entities in nearby cells.
///
/// # Use Cases
///
/// - **Collision Detection:** Find entities within collision radius
/// - **Boids/Flocking:** Query neighbors for separation, alignment, cohesion
/// - **AI/Aggro:** Find nearby enemies or threats
/// - **Area Effects:** Find all entities in blast radius
///
/// # Example
///
/// ```rust
/// use bevy::prelude::Entity;
/// use peregrine::game::fixed_math::{FixedNum, FixedVec2};
/// use peregrine::game::spatial_hash::SpatialHash;
///
/// let mut hash = SpatialHash::new(
///     FixedNum::from_num(100.0), // map width
///     FixedNum::from_num(100.0), // map height  
///     FixedNum::from_num(5.0)    // cell size
/// );
///
/// // Insert entities
/// let entity = Entity::PLACEHOLDER;
/// let pos = FixedVec2::new(FixedNum::from_num(10.0), FixedNum::from_num(20.0));
/// hash.insert(entity, pos);
///
/// // Query nearby entities within radius (excludes self)
/// let radius = FixedNum::from_num(5.0);
/// let nearby = hash.query_radius(entity, pos, radius);
/// assert_eq!(nearby.len(), 0); // No other entities nearby
/// ```
///
/// # Performance
///
/// - **Insert:** O(1) amortized
/// - **Query:** O(k) where k = entities in nearby cells (typically << N)
/// - **Clear:** O(1) (reuses allocated vectors)
///
/// # Implementation Notes
///
/// - Uses fixed-point math for deterministic cross-platform behavior
/// - Cells use `Vec` instead of `HashSet` for better cache locality
/// - Origin is at bottom-left corner of map (-width/2, -height/2)
#[derive(Resource)]
pub struct SpatialHash {
    cell_size: FixedNum,
    cols: usize,
    rows: usize,
    cells: Vec<Vec<(Entity, FixedVec2)>>,
    map_width: FixedNum,
    map_height: FixedNum,
}

impl SpatialHash {
    pub fn new(map_width: FixedNum, map_height: FixedNum, cell_size: FixedNum) -> Self {
        let cols = (map_width / cell_size).ceil().to_num::<usize>() + 1;
        let rows = (map_height / cell_size).ceil().to_num::<usize>() + 1;
        
        Self {
            cell_size,
            cols,
            rows,
            cells: vec![Vec::new(); cols * rows],
            map_width,
            map_height,
        }
    }

    pub fn resize(&mut self, map_width: FixedNum, map_height: FixedNum, cell_size: FixedNum) {
        let cols = (map_width / cell_size).ceil().to_num::<usize>() + 1;
        let rows = (map_height / cell_size).ceil().to_num::<usize>() + 1;
        
        self.map_width = map_width;
        self.map_height = map_height;
        self.cell_size = cell_size;
        self.cols = cols;
        self.rows = rows;
        self.cells = vec![Vec::new(); cols * rows];
    }

    pub fn clear(&mut self) {
        for cell in &mut self.cells {
            cell.clear();
        }
    }

    /// Count the total number of entity entries across all cells.
    /// Useful for debugging and diagnostics.
    pub fn total_entries(&self) -> usize {
        self.cells.iter().map(|cell| cell.len()).sum()
    }

    /// Count the number of non-empty cells.
    /// Useful for debugging and diagnostics.
    pub fn non_empty_cells(&self) -> usize {
        self.cells.iter().filter(|cell| !cell.is_empty()).count()
    }

    // Getters for grid parameters
    pub fn cell_size(&self) -> FixedNum { self.cell_size }
    pub fn map_width(&self) -> FixedNum { self.map_width }
    pub fn map_height(&self) -> FixedNum { self.map_height }
    pub fn cols(&self) -> usize { self.cols }
    pub fn rows(&self) -> usize { self.rows }

    // Internal accessor for submodules
    pub(crate) fn cells(&self) -> &Vec<Vec<(Entity, FixedVec2)>> {
        &self.cells
    }

    pub(crate) fn cells_mut(&mut self) -> &mut Vec<Vec<(Entity, FixedVec2)>> {
        &mut self.cells
    }
}
