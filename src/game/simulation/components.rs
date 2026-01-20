/// Component definitions for the simulation layer.
///
/// This module contains all components used by the deterministic simulation,
/// including position, velocity, collision, and caching components.

use bevy::prelude::*;
use crate::game::fixed_math::{FixedVec2, FixedNum};
use crate::game::pathfinding::RegionId;

// ============================================================================
// Position & Physics Components
// ============================================================================

/// Logical position of an entity in the simulation world.
/// We use FixedVec2 for deterministic gameplay.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct SimPosition(pub FixedVec2);

/// Previous logical position for interpolation.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct SimPositionPrev(pub FixedVec2);

/// Logical velocity of an entity.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct SimVelocity(pub FixedVec2);

/// Logical acceleration of an entity.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct SimAcceleration(pub FixedVec2);

// ============================================================================
// Collision Components
// ============================================================================

/// Collision layers for filtering
pub mod layers {
    pub const NONE: u32 = 0;
    pub const UNIT: u32 = 1 << 0;
    pub const OBSTACLE: u32 = 1 << 1;
    pub const PROJECTILE: u32 = 1 << 2;
    pub const ALL: u32 = u32::MAX;
}

/// Collider component for collision detection
#[derive(Component, Debug, Clone, Copy)]
pub struct Collider {
    pub radius: FixedNum,
    pub layer: u32,
    pub mask: u32,
}

impl Default for Collider {
    fn default() -> Self {
        Self {
            radius: FixedNum::from_num(0.5),
            layer: layers::UNIT,
            mask: layers::UNIT | layers::OBSTACLE,
        }
    }
}

/// Component to track collision state of a unit.
/// Always present to avoid insert/remove overhead and leverage ECS change detection.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CollisionState {
    pub is_colliding: bool,
}

// ============================================================================
// Obstacle Components
// ============================================================================

/// Marker component for static circular obstacles.
/// The actual radius is stored in the Collider component.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct StaticObstacle;

/// Component to mark obstacles that are part of the flow field.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct FlowFieldObstacle;

// ============================================================================
// Force Components
// ============================================================================

/// Force source component (for black holes, wind, etc.)
#[derive(Component, Debug, Clone)]
pub struct ForceSource {
    pub force_type: ForceType,
    pub radius: FixedNum, 
}

/// Type of force applied by a ForceSource
#[derive(Debug, Clone, Copy)]
pub enum ForceType {
    Radial(FixedNum), // Strength. >0 attract, <0 repel.
    Directional(FixedVec2), // Vector force.
}

// ============================================================================
// Neighbor Caching Components
// ============================================================================

/// Cached neighbor list for collision detection.
/// 
/// Stores neighbor entities with their positions and collider radii.
/// Updated every tick to ensure positions are always current.
/// This eliminates redundant ECS queries in the collision detection hot path.
#[derive(Component, Debug, Clone)]
pub struct CachedNeighbors {
    /// List of nearby entities with cached position and collider radius
    /// (Entity, Position, ColliderRadius)
    pub neighbors: Vec<(Entity, FixedVec2, FixedNum)>,
}

impl Default for CachedNeighbors {
    fn default() -> Self {
        Self {
            // Preallocate with typical capacity (~8-20 neighbors expected)
            neighbors: Vec::with_capacity(16),
        }
    }
}

/// Cached neighbor list for boids steering calculations.
/// 
/// Separate from CachedNeighbors because boids needs:
/// - Larger search radius (5.0 units vs 2.0 for collision)
/// - Velocity data for alignment behavior
/// - Fewer neighbors (limit to 8 closest)
/// - Can tolerate stale data (visual-only behavior)
#[derive(Component, Debug, Clone)]
pub struct BoidsNeighborCache {
    /// Closest N neighbors with position and velocity (stack-allocated up to 8)
    pub neighbors: smallvec::SmallVec<[(Entity, FixedVec2, FixedVec2); 8]>,
    /// Position where the last query was performed
    pub last_query_pos: FixedVec2,
    /// Frames elapsed since last cache update
    pub frames_since_update: u32,
}

impl Default for BoidsNeighborCache {
    fn default() -> Self {
        Self {
            // MEMORY_OK: SmallVec has inline storage, no heap allocation for small sizes
            neighbors: smallvec::SmallVec::new(),
            last_query_pos: FixedVec2::ZERO,
            // Initialize to high value to force update on first tick
            frames_since_update: 999,
        }
    }
}

// ============================================================================
// Pathfinding Cache Components
// ============================================================================

/// Cached pathfinding data to avoid expensive region lookups every frame.
/// 
/// **Performance Impact:** 3.75x speedup by caching cluster/region
/// - Without cache: ~75ns per unit per frame (full lookup)
/// - With cache + skip-frame: ~20ns per unit per frame
/// 
/// See: PATHFINDING.md Section 2.3 - Caching Strategy
#[derive(Component, Debug, Clone, Copy)]
pub struct PathCache {
    /// Currently occupied cluster (x, y)
    pub cached_cluster: (usize, usize),
    /// Currently occupied region within cluster
    pub cached_region: RegionId,
    /// Frames since last validation (revalidate every 4 frames)
    pub frames_since_validation: u8,
}

impl Default for PathCache {
    fn default() -> Self {
        Self {
            cached_cluster: (0, 0),
            cached_region: RegionId(0),
            // Force validation on first frame
            frames_since_validation: 4,
        }
    }
}

// ============================================================================
// Spatial Hash Tracking
// ============================================================================

/// Tracks which spatial hash cells an entity currently occupies.
///
/// For correct collision detection with variable entity sizes, entities are stored
/// in **all** spatial hash cells their radius overlaps. This component tracks those
/// cells so they can be efficiently updated when the entity moves.
///
/// # Multi-Cell Storage Rationale
///
/// - Small entities (radius ≤ cell_size): Occupy 1-4 cells
/// - Medium entities (radius = 2× cell_size): Occupy ~9 cells  
/// - Large entities (radius = 10× cell_size): Occupy ~100 cells
///
/// Without multi-cell storage, large entities can be invisible to queries from
/// nearby small entities, causing collision detection failures.
///
/// # Performance Optimization (StarCraft 2 Approach)
///
/// Instead of checking if an entity moved, we cache the grid bounding box
/// (min/max grid coordinates) that the entity occupies. We only update the
/// spatial hash if this bounding box changes. This is mathematically sound:
/// if the box didn't change, the cells cannot have changed.
///
/// This approach is superior to distance-based checks because:
/// - Only 4 integer comparisons (no floating point math)
/// Component tracking which cell an entity occupies in the spatial hash.
///
/// **NEW DESIGN (Staggered Multi-Resolution Grids):**
/// - Entities are ALWAYS single-cell (no multi-cell complexity)
/// - Inserted into whichever grid (A or B) they're closest to center of
/// - Only updates when entity crosses midpoint between grid centers
///
/// See SPATIAL_PARTITIONING.md Section 2.2 for detailed explanation.
#[derive(Component, Debug, Clone, Copy)]
pub struct OccupiedCell {
    /// Which cell size class (index into SpatialHash.size_classes)
    pub size_class: u8,
    /// Which grid (0 = Grid A, 1 = Grid B)
    pub grid_offset: u8,
    /// Cell column
    pub col: usize,
    /// Cell row
    pub row: usize,
    /// Index WITHIN the cell's range (0..cell.count) for O(1) swap-based removal
    pub vec_idx: usize,
}

impl Default for OccupiedCell {
    fn default() -> Self {
        Self {
            size_class: 0,
            grid_offset: 0,
            col: 0,
            row: 0,
            vec_idx: 0,
        }
    }
}

