/// Main simulation plugin that wires up all simulation systems.

use bevy::prelude::*;
use crate::game::GameState;
use crate::game::spatial_hash::SpatialHash;
use crate::game::math::FixedNum;

// Import the simulation submodules
mod r#mod;

// Re-export the public API
pub use r#mod::*;

/// Main simulation plugin
pub struct SimulationPlugin;

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        // Configure FixedUpdate timestep
        app.insert_resource(Time::<Fixed>::from_seconds(1.0 / 20.0)); 
        
        // Initialize resources
        app.init_resource::<SimConfig>();
        app.init_resource::<SimPerformance>();
        app.insert_resource(SpatialHash::new(FixedNum::from_num(100), FixedNum::from_num(100), FixedNum::from_num(2)));
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
            debug::draw_flow_field_gizmos,
            debug::draw_force_sources,
            debug::draw_unit_paths,
        ).run_if(in_state(GameState::InGame).or(in_state(GameState::Editor)).or(in_state(GameState::Loading))));
        
        app.add_systems(Update, 
            systems::apply_new_obstacles
                .run_if(in_state(GameState::InGame).or(in_state(GameState::Loading)))
        );
        
        // Fixed update systems (deterministic simulation)
        app.add_systems(FixedUpdate, (
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
            
            // Physics
            systems::update_spatial_hash
                .in_set(SimSet::Physics)
                .before(collision::update_neighbor_cache)
                .before(collision::update_boids_neighbor_cache)
                .before(collision::detect_collisions)
                .before(collision::resolve_collisions),
            collision::update_neighbor_cache
                .in_set(SimSet::Physics)
                .before(collision::detect_collisions),
            collision::update_boids_neighbor_cache.in_set(SimSet::Physics),
            physics::constrain_to_map_bounds.in_set(SimSet::Physics),
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
