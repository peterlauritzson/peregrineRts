/// Core simulation systems.
///
/// This module contains systems for:
/// - Tick management
/// - Input processing (commands → pathfinding requests)
/// - Performance tracking
///
/// Note: Path following has been moved to pathfinding::navigation module

#[path = "systems_spatial.rs"]
mod systems_spatial;
#[path = "systems_config.rs"]
mod systems_config;

use bevy::prelude::*;
use crate::game::fixed_math::FixedVec2;
use crate::game::pathfinding::{Path, PathRequest};
use peregrine_macros::profile;

use super::components::*;
use super::resources::*;
use super::events::*;

// Re-export systems from submodules
pub use systems_spatial::{update_spatial_hash, init_flow_field, apply_obstacle_to_flow_field, apply_new_obstacles, PendingVecIdxUpdates};
pub use systems_config::{init_sim_config_from_initial, update_sim_from_runtime_config, SpatialHashRebuilt};

// ============================================================================
// Tick Management
// ============================================================================

/// Increment the global simulation tick counter.
/// 
/// This system runs first in the FixedUpdate schedule to ensure all other
/// systems have access to the current tick value for deterministic logic
/// and conditional logging.
pub fn increment_sim_tick(mut tick: ResMut<SimTick>) {
    tick.increment();
}

// ============================================================================
// Input Processing
// ============================================================================

/// Process player input commands deterministically
pub fn process_input(
    mut commands: Commands,
    mut move_events: MessageReader<UnitMoveCommand>,
    mut stop_events: MessageReader<UnitStopCommand>,
    mut spawn_events: MessageReader<SpawnUnitCommand>,
    mut path_requests: MessageWriter<PathRequest>,
    query: Query<&SimPosition>,
) {
    
    
    // Deterministic Input Processing:
    // 1. Collect all events
    // 2. Sort by Player ID (and potentially sequence number if we had one)
    // 3. Execute in order
    
    // Handle Stop Commands
    let mut stops: Vec<&UnitStopCommand> = stop_events.read().collect();
    stops.sort_by_key(|e| e.player_id);

    for event in stops {
        commands.entity(event.entity).remove::<Path>();
        // Also reset velocity?
        // MEMORY_OK: ECS component insert, not collection growth
        commands.entity(event.entity).insert(SimVelocity(FixedVec2::ZERO));
    }

    // Handle Move Commands
    let mut moves: Vec<&UnitMoveCommand> = move_events.read().collect();
    moves.sort_by_key(|e| e.player_id);
    
    for event in moves {
        if let Ok(_pos) = query.get(event.entity) {
            // Send Path Request instead of setting target directly
            path_requests.write(PathRequest {
                entity: event.entity,
                goal: event.target,
            });
            // Remove old path component to stop movement until path is found
            commands.entity(event.entity).remove::<Path>();
        }
    }

    // Handle Spawn Commands
    let mut spawns: Vec<&SpawnUnitCommand> = spawn_events.read().collect();
    spawns.sort_by_key(|e| e.player_id);

    for event in spawns {
        // Note: In a real game, we'd need a way to deterministically assign Entity IDs 
        // or use a reservation system. For now, we let Bevy spawn.
        // To be strictly deterministic across clients, we would need to reserve Entity IDs 
        // or use a deterministic ID generator.
        commands.spawn((
            crate::game::GameEntity,
            crate::game::unit::Unit,
            crate::game::unit::Health { current: 100.0, max: 100.0 },
            SimPosition(event.position),
            SimPositionPrev(event.position),
            SimVelocity(FixedVec2::ZERO),
            SimAcceleration(FixedVec2::ZERO),
            Collider::default(),
            CollisionState::default(),
            // OccupiedCell added by update_spatial_hash on first frame
        ));
    }
}

// ============================================================================
// Performance Tracking
// ============================================================================

/// Log simulation status periodically
pub fn sim_start(
    #[allow(unused_variables)] stats: Res<SimPerformance>,
    #[allow(unused_variables)] tick: Res<SimTick>,
    #[allow(unused_variables)] units_query: Query<Entity, With<crate::game::unit::Unit>>,
    #[allow(unused_variables)] paths_query: Query<&Path>,
) {
    use crate::profile_log;
    
    profile_log!(tick, "[SIM STATUS] Tick: {} | Units: {} | Active Paths: {} | Last sim duration: {:?}", 
          tick.0, units_query.iter().len(), paths_query.iter().len(), stats.last_duration);
}

/// Update simulation performance stats
/// 
/// NOTE: Individual system timing is handled by #[profile] macro.
/// This tracks overall fixed update duration for monitoring.
#[profile(16)]  // Warn if entire simulation tick > 16ms
pub fn sim_end(mut stats: ResMut<SimPerformance>, time: Res<Time<Fixed>>) {
    // Store the actual fixed timestep duration for status reporting
    // This represents the configured tick duration, not the wall-clock time
    stats.last_duration = time.delta();
}

// ... Additional loading/setup systems will be added later if needed
