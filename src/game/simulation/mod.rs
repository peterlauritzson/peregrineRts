/// Simulation layer - deterministic game logic.
///
/// This module is organized into:
/// - **components**: Simulation components (position, velocity, collision, etc.)
/// - **resources**: Simulation resources (config, flow field, etc.)
/// - **events**: Commands and events for controlling simulation
/// - **collision**: Collision detection and resolution
/// - **physics**: Physics integration and movement
/// - **systems**: Core systems (pathfollowing, spatial hash, etc.)
/// - **debug**: Debug visualization (gizmos, paths, etc.)

use bevy::prelude::*;
use crate::game::GameState;
use crate::game::spatial_hash::SpatialHash;
use crate::game::fixed_math::FixedNum;

// Module declarations
pub mod components;
pub mod resources;
pub mod events;
pub mod collision;
pub mod physics;
pub mod systems;
pub mod debug;

// Re-export commonly used items
pub use components::*;
pub use resources::*;
pub use events::*;

// Re-export specific functions that are used externally
pub use systems::{follow_path, apply_obstacle_to_flow_field};

// System sets for organizing execution order
#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub enum SimSet {
    Input,      // Processing inputs into commands
    Steering,   // Calculating desired velocities (Pathfinding, Boids)
    Physics,    // Collision detection and resolution
    Integration // Applying velocity to position
}

/// Main simulation plugin
pub struct SimulationPlugin;

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        // Configure FixedUpdate timestep
        app.insert_resource(Time::<Fixed>::from_seconds(1.0 / 20.0)); 
        
        // Initialize resources (SpatialHash will be properly initialized in init_sim_config_from_initial)
        app.init_resource::<SimConfig>();
        app.init_resource::<SimPerformance>();
        app.init_resource::<SimTick>();
        app.init_resource::<systems::PendingVecIdxUpdates>();
        app.insert_resource(SpatialHash::new(
            FixedNum::from_num(100.0),
            FixedNum::from_num(100.0),
            &[0.5, 10.0],  // Default entity radii
            4.0,           // Default radius to cell ratio
            10_000,        // Default max entities (will be overwritten by InitialConfig)
            1.0            // No overcapacity for initial creation
        ));
        // Initialize scratch buffer for zero-allocation spatial queries
        app.insert_resource(crate::game::spatial_hash::SpatialHashScratch::default_capacity());
        app.insert_resource(MapFlowField(Default::default()));
        app.init_resource::<MapStatus>();
        app.init_resource::<DebugConfig>();
        
        // Register events
        app.add_message::<UnitMoveCommand>();
        app.add_message::<UnitStopCommand>();
        app.add_message::<SpawnUnitCommand>();
        app.add_message::<collision::CollisionEvent>();

        // Configure System Sets
        app.configure_sets(FixedUpdate, (
            SimSet::Input,
            SimSet::Steering,
            SimSet::Integration,
            SimSet::Physics,
        ).chain().run_if(in_state(GameState::InGame).or(in_state(GameState::Editor))));

        // Startup systems
        app.add_systems(Startup, (
            systems::init_flow_field,
            systems::init_sim_config_from_initial
        ).chain());
        
        // OnEnter systems (map loading will be added back later)
        // app.add_systems(OnEnter(GameState::Loading), systems::load_default_map);
        // app.add_systems(OnEnter(GameState::InGame), systems::update_ground_plane_from_loaded_map);
        
        // Update systems (run in all states)
        app.add_systems(Update, (
            systems::update_sim_from_runtime_config,
            debug::toggle_debug,
            debug::draw_force_sources,
            debug::draw_unit_paths,
        ).run_if(in_state(GameState::InGame).or(in_state(GameState::Editor)).or(in_state(GameState::Loading))));
        
        app.add_systems(Update, 
            systems::apply_new_obstacles
                .run_if(in_state(GameState::InGame).or(in_state(GameState::Loading)))
        );
        
        // Use sequential spatial hash update (simple and efficient for typical workloads)
        
        // Fixed update systems (deterministic simulation)
        app.add_systems(FixedUpdate, (
            // Increment tick counter first (before all other systems)
            systems::increment_sim_tick.before(systems::sim_start),
            
            // Pre-simulation
            systems::sim_start.before(SimSet::Input),
            
            // Input processing
            physics::cache_previous_state.in_set(SimSet::Input),
            systems::process_input.in_set(SimSet::Input),
            
            // Steering
            physics::apply_friction.in_set(SimSet::Steering).before(systems::follow_path),
            systems::follow_path.in_set(SimSet::Steering),
            physics::apply_forces.in_set(SimSet::Steering).before(systems::follow_path),
            
            // Integration
            physics::apply_velocity.in_set(SimSet::Integration),
            
            // Physics - Spatial Hash (full rebuild every frame)
            systems::update_spatial_hash
                .in_set(SimSet::Physics)
                .before(collision::detect_collisions)
                .before(collision::resolve_collisions),
            collision::detect_collisions
                .in_set(SimSet::Physics)
                .before(collision::resolve_collisions),
            collision::resolve_collisions.in_set(SimSet::Physics),
            collision::resolve_obstacle_collisions.in_set(SimSet::Physics),
            
            // Post-simulation
            systems::sim_end.after(SimSet::Physics),
        ));
    }
}
