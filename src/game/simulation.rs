use bevy::prelude::*;
use crate::game::config::{GameConfig, GameConfigHandle};

pub struct SimulationPlugin;

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub enum SimSet {
    Input,      // Processing inputs into commands
    Steering,   // Calculating desired velocities (Pathfinding, Boids)
    Physics,    // Collision detection and resolution
    Integration // Applying velocity to position
}

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        // Configure FixedUpdate
        app.insert_resource(Time::<Fixed>::from_hz(20.0)); // Default, will be overridden by config

        // Configure System Sets
        app.configure_sets(FixedUpdate, (
            SimSet::Input,
            SimSet::Steering,
            SimSet::Physics,
            SimSet::Integration,
        ).chain());

        // Register Systems
        app.add_systems(Update, update_sim_from_config);
        app.add_systems(FixedUpdate, (
            cache_previous_state.in_set(SimSet::Integration),
            apply_velocity.in_set(SimSet::Integration),
        ).chain());
    }
}

fn update_sim_from_config(
    mut fixed_time: ResMut<Time<Fixed>>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    mut events: MessageReader<AssetEvent<GameConfig>>,
) {
    for event in events.read() {
        info!("Config event: {:?}", event);
        if event.is_modified(config_handle.0.id()) || event.is_loaded_with_dependencies(config_handle.0.id()) {
             if let Some(config) = game_configs.get(&config_handle.0) {
                 fixed_time.set_timestep_hz(config.tick_rate);
                 info!("Updated tick rate to {}", config.tick_rate);
             }
        }
    }
}

/// Logical position of an entity in the simulation world.
/// We use Vec2 because the gameplay is strictly 2D (X, Z plane).
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
pub struct SimPosition(pub Vec2);

/// Previous logical position for interpolation.
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
pub struct SimPositionPrev(pub Vec2);

/// Logical velocity of an entity.
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
pub struct SimVelocity(pub Vec2);

/// Logical target position for movement.
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
pub struct SimTarget(pub Vec2);

fn cache_previous_state(
    mut query: Query<(&mut SimPositionPrev, &SimPosition)>,
) {
    for (mut prev, pos) in query.iter_mut() {
        prev.0 = pos.0;
    }
}

fn apply_velocity(
    time: Res<Time>,
    mut query: Query<(&mut SimPosition, &SimVelocity)>,
) {
    let delta = time.delta_secs();
    // info!("Sim Tick: delta={}", delta);
    for (mut pos, vel) in query.iter_mut() {
        if vel.0.length_squared() > 0.0 {
            info!("Moving unit: pos={:?}, vel={:?}, delta={}", pos.0, vel.0, delta);
            pos.0 += vel.0 * delta;
        }
    }
}
