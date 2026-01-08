/// Resource definitions for the simulation.
///
/// This module contains all resources used by the simulation,
/// including configuration and performance tracking.

use bevy::prelude::*;
use crate::game::math::{FixedNum};
use crate::game::flow_field::FlowField;
use std::time::{Instant, Duration};

// ============================================================================
// Performance Tracking
// ============================================================================

/// Performance tracking for simulation ticks
#[derive(Resource)]
pub struct SimPerformance {
    pub start_time: Option<Instant>,
    pub last_duration: Duration,
}

impl Default for SimPerformance {
    fn default() -> Self {
        Self {
            start_time: None,
            last_duration: Duration::from_secs(0),
        }
    }
}

// ============================================================================
// Map Resources
// ============================================================================

/// Map loading status
#[derive(Resource, Default)]
pub struct MapStatus {
    pub loaded: bool,
}

/// Flow field resource for the entire map
#[derive(Resource, Default)]
pub struct MapFlowField(pub FlowField);

// ============================================================================
// Simulation Configuration
// ============================================================================

/// Runtime simulation configuration with fixed-point values for deterministic physics.
///
/// This resource stores all simulation parameters converted from [`GameConfig`] (f32/f64)
/// to fixed-point math ([`FixedNum`]) for cross-platform determinism.
///
/// # Determinism Guarantees
///
/// - All physics parameters use fixed-point arithmetic to ensure identical results across platforms
/// - Config values are converted from floats once when loaded on startup or hot-reload
/// - **IMPORTANT:** Config changes during gameplay will break determinism in multiplayer
///   
/// # Multiplayer Considerations
///
/// In multiplayer/networked games:
/// - All clients MUST load identical GameConfig files before match start
/// - Config reloads during a match will desync clients (floating-point → fixed-point conversion may vary)
/// - `tick_rate` changes mid-game will invalidate simulation state
///
/// **Recommendation:** Lock configuration at match start, prevent runtime changes in multiplayer.
///
/// # Why Not Store Config as FixedNum?
///
/// The [`GameConfig`] asset is user-facing (loaded from RON files) where f32/f64 is more ergonomic.
/// This separation allows:
/// - Human-readable config files with decimal numbers (e.g., `unit_speed: 5.5`)
/// - Single conversion point (not scattered throughout codebase)
/// - Clear boundary between "config layer" (floats) and "simulation layer" (fixed-point)
///
/// See also: [ARCHITECTURE.md](documents/Guidelines/ARCHITECTURE.md) - Determinism section
#[derive(Resource)]
pub struct SimConfig {
    pub tick_rate: f64,
    pub unit_speed: FixedNum,
    pub map_width: FixedNum,
    pub map_height: FixedNum,
    pub unit_radius: FixedNum,
    pub collision_push_strength: FixedNum,
    pub collision_restitution: FixedNum,
    pub collision_drag: FixedNum,
    pub collision_iterations: usize,
    pub collision_search_radius_multiplier: FixedNum,
    pub obstacle_search_range: i32,
    pub epsilon: FixedNum,
    pub obstacle_push_strength: FixedNum,
    pub arrival_threshold: FixedNum,
    pub max_force: FixedNum,
    pub steering_force: FixedNum,
    pub repulsion_force: FixedNum,
    pub repulsion_decay: FixedNum,
    pub friction: FixedNum,
    pub min_velocity: FixedNum,
    pub braking_force: FixedNum,
    pub touch_dist_multiplier: FixedNum,
    pub check_dist_multiplier: FixedNum,
    pub separation_weight: FixedNum,
    pub alignment_weight: FixedNum,
    pub cohesion_weight: FixedNum,
    pub neighbor_radius: FixedNum,
    pub separation_radius: FixedNum,
    pub boids_max_neighbors: usize,
    pub black_hole_strength: FixedNum,
    pub wind_spot_strength: FixedNum,
    pub force_source_radius: FixedNum,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            tick_rate: 30.0,
            unit_speed: FixedNum::from_num(5.0),
            map_width: FixedNum::from_num(2048.0),
            map_height: FixedNum::from_num(2048.0),
            unit_radius: FixedNum::from_num(0.5),
            collision_push_strength: FixedNum::from_num(1.0),
            collision_restitution: FixedNum::from_num(0.5),
            collision_drag: FixedNum::from_num(0.1),
            collision_iterations: 4,
            collision_search_radius_multiplier: FixedNum::from_num(4.0),
            obstacle_search_range: 1,
            epsilon: FixedNum::from_num(0.0001),
            obstacle_push_strength: FixedNum::from_num(1.0),
            arrival_threshold: FixedNum::from_num(0.1),
            max_force: FixedNum::from_num(20.0),
            steering_force: FixedNum::from_num(15.0),
            repulsion_force: FixedNum::from_num(20.0),
            repulsion_decay: FixedNum::from_num(2.0),
            friction: FixedNum::from_num(0.9),
            min_velocity: FixedNum::from_num(0.01),
            braking_force: FixedNum::from_num(5.0),
            touch_dist_multiplier: FixedNum::from_num(2.1),
            check_dist_multiplier: FixedNum::from_num(4.0),
            separation_weight: FixedNum::from_num(1.0),
            alignment_weight: FixedNum::from_num(1.0),
            cohesion_weight: FixedNum::from_num(1.0),
            neighbor_radius: FixedNum::from_num(5.0),
            separation_radius: FixedNum::from_num(1.5),
            boids_max_neighbors: 8,
            black_hole_strength: FixedNum::from_num(50.0),
            wind_spot_strength: FixedNum::from_num(-50.0),
            force_source_radius: FixedNum::from_num(10.0),
        }
    }
}

// ============================================================================
// Debug Configuration
// ============================================================================

/// Debug visualization settings
#[derive(Resource)]
pub struct DebugConfig {
    pub show_flow_field: bool,
    pub show_pathfinding_graph: bool,
    pub show_paths: bool,
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self { 
            show_flow_field: false,
            show_pathfinding_graph: false,
            show_paths: false,
        }
    }
}
