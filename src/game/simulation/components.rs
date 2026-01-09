/// Component definitions for the simulation layer.
///
/// This module contains all components used by the deterministic simulation,
/// including position, velocity, collision, and caching components.

use bevy::prelude::*;
use crate::game::math::{FixedVec2, FixedNum};

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

/// Marker component to indicate if a unit is currently colliding with another unit.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Colliding;

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
/// Stores the result of spatial hash queries to avoid redundant lookups.
/// Cache is invalidated when the entity moves significantly or after a timeout.
#[derive(Component, Debug, Clone)]
pub struct CachedNeighbors {
    /// List of nearby entities from last spatial query
    pub neighbors: Vec<(Entity, FixedVec2)>,
    /// Position where the last query was performed
    pub last_query_pos: FixedVec2,
    /// Frames elapsed since last cache update
    pub frames_since_update: u32,
    /// Whether this entity is classified as a fast mover
    pub is_fast_mover: bool,
}

impl Default for CachedNeighbors {
    fn default() -> Self {
        Self {
            neighbors: Vec::new(),
            last_query_pos: FixedVec2::ZERO,
            // Initialize to high value to force update on first tick
            frames_since_update: 999,
            is_fast_mover: false,
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
            neighbors: smallvec::SmallVec::new(),
            last_query_pos: FixedVec2::ZERO,
            // Initialize to high value to force update on first tick
            frames_since_update: 999,
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
/// See SPATIAL_PARTITIONING.md Section 2.2 for detailed explanation.
#[derive(Component, Debug, Clone)]
pub struct OccupiedCells {
    /// All (col, row) pairs this entity currently occupies in the spatial hash
    pub cells: Vec<(usize, usize)>,
}

impl Default for OccupiedCells {
    fn default() -> Self {
        Self {
            cells: Vec::new(),
        }
    }
}
