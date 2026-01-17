/// Resource definitions for the simulation.
///
/// This module contains all resources used by the simulation,
/// including configuration and performance tracking.

use bevy::prelude::*;
use crate::game::fixed_math::{FixedNum};
use crate::game::structures::FlowField;
// NOLINT: Duration is a data type for storing time values, not for profiling/timing
use std::time::Duration;

// ============================================================================
// Simulation Tick Counter
// ============================================================================

/// Global deterministic tick counter for the simulation.
/// 
/// This resource is incremented once per fixed update cycle and provides
/// a reliable, deterministic tick count for:
/// - Conditional logging (every N ticks)
/// - Replay/determinism validation
/// - Time-independent simulation logic
/// 
/// # Determinism
/// Unlike `Time<Fixed>::elapsed_secs()` which uses floats, this counter
/// is purely integer-based and guaranteed to be identical across all clients
/// in multiplayer scenarios.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct SimTick(pub u64);

impl SimTick {
    /// Increment the tick counter (wraps on overflow)
    pub fn increment(&mut self) {
        self.0 = self.0.wrapping_add(1);
    }
    
    /// Get the current tick value
    pub fn get(&self) -> u64 {
        self.0
    }
    
    /// Check if this tick should trigger periodic logging (every 100 ticks)
    pub fn should_log(&self) -> bool {
        self.0 % 100 == 0
    }
}

// ============================================================================
// Performance Tracking
// ============================================================================

/// Performance tracking for simulation ticks.
/// 
/// Stores the last recorded simulation tick duration for display in status logs.
/// Individual system timing is handled by the #[profile] macro.
#[derive(Resource, Default)]
pub struct SimPerformance {
    pub last_duration: Duration,
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
    pub max_acceleration: FixedNum,
    pub repulsion_force: FixedNum,
    pub repulsion_decay: FixedNum,
    pub friction: FixedNum,
    pub min_velocity: FixedNum,
    pub max_velocity: FixedNum,
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
    
    // Spatial Hash Optimization
    pub spatial_hash_max_ticks_without_update: u8,
    pub spatial_hash_velocity_estimate_scale: FixedNum,
    
    // Parallel Update Configuration
    /// Enable parallel spatial hash updates (requires rayon)
    pub spatial_hash_parallel_updates: bool,
    /// Number of regions per axis for parallel updates (e.g., 10 = 10×10 = 100 regions)
    pub spatial_hash_regions_per_axis: usize,
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
            max_acceleration: FixedNum::from_num(100.0),
            repulsion_force: FixedNum::from_num(20.0),
            repulsion_decay: FixedNum::from_num(2.0),
            friction: FixedNum::from_num(0.9),
            min_velocity: FixedNum::from_num(0.01),
            max_velocity: FixedNum::from_num(50.0),
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
            spatial_hash_max_ticks_without_update: 8,
            spatial_hash_velocity_estimate_scale: FixedNum::from_num(1.0),
            spatial_hash_parallel_updates: true,  // Enable by default for performance
            spatial_hash_regions_per_axis: 10,    // 10×10 = 100 parallel chunks
        }
    }
}

// ============================================================================
// Debug Configuration
// ============================================================================

/// Debug visualization settings
#[derive(Resource)]
pub struct DebugConfig {
    pub show_pathfinding_graph: bool,
    pub show_paths: bool,
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self { 
            show_pathfinding_graph: false,
            show_paths: false,
        }
    }
}
