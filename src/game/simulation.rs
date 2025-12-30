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

#[derive(Resource, Default, Reflect)]
#[reflect(Resource)]
pub struct GlobalFlow {
    pub velocity: Vec2,
}

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        // Configure FixedUpdate
        app.insert_resource(Time::<Fixed>::from_hz(20.0)); // Default, will be overridden by config
        app.init_resource::<GlobalFlow>();
        app.register_type::<GlobalFlow>();

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
            apply_global_flow.in_set(SimSet::Physics).before(resolve_collisions),
            constrain_to_map_bounds.in_set(SimSet::Physics),
            detect_collisions.in_set(SimSet::Physics),
            resolve_collisions.in_set(SimSet::Physics),
            resolve_obstacle_collisions.in_set(SimSet::Physics),
        ));
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

/// Component to mark if a unit is currently colliding with another unit.
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
pub struct Colliding;

/// Component for static circular obstacles.
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
pub struct StaticObstacle {
    pub radius: f32,
}

fn cache_previous_state(
    mut query: Query<(&mut SimPositionPrev, &SimPosition)>,
) {
    for (mut prev, pos) in query.iter_mut() {
        prev.0 = pos.0;
    }
}

fn apply_velocity(
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    mut query: Query<(&mut SimPosition, &SimVelocity)>,
) {
    let Some(config) = game_configs.get(&config_handle.0) else { return };
    let delta = 1.0 / config.tick_rate as f32;

    for (mut pos, vel) in query.iter_mut() {
        if vel.0.length_squared() > 0.0 {
            // info!("Moving unit: pos={:?}, vel={:?}, delta={}", pos.0, vel.0, delta);
            pos.0 += vel.0 * delta;
        }
    }
}

fn constrain_to_map_bounds(
    mut query: Query<(&mut SimPosition, &mut SimVelocity)>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
) {
    let Some(config) = game_configs.get(&config_handle.0) else { return };
    let half_w = config.map_width / 2.0;
    let half_h = config.map_height / 2.0;

    for (mut pos, mut vel) in query.iter_mut() {
        // 1. Clamp Position
        if pos.0.x < -half_w { pos.0.x = -half_w; }
        if pos.0.x > half_w { pos.0.x = half_w; }
        if pos.0.y < -half_h { pos.0.y = -half_h; }
        if pos.0.y > half_h { pos.0.y = half_h; }

        // 2. Zero Velocity against walls
        if pos.0.x <= -half_w && vel.0.x < 0.0 { vel.0.x = 0.0; }
        if pos.0.x >= half_w && vel.0.x > 0.0 { vel.0.x = 0.0; }
        if pos.0.y <= -half_h && vel.0.y < 0.0 { vel.0.y = 0.0; }
        if pos.0.y >= half_h && vel.0.y > 0.0 { vel.0.y = 0.0; }
    }
}

fn detect_collisions(
    mut commands: Commands,
    query: Query<(Entity, &SimPosition)>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
) {
    let Some(config) = game_configs.get(&config_handle.0) else { return };
    let radius = config.unit_radius;

    // Reset collision state
    // Note: In a real ECS, adding/removing components every frame can be expensive (archetype moves).
    // For now, it's fine for prototyping. A better way would be a boolean field or a resource.
    // Or we can just query for Colliding and remove it if not colliding.
    
    // First, collect all positions
    let mut units: Vec<(Entity, Vec2)> = query.iter().map(|(e, p)| (e, p.0)).collect();
    units.sort_by_key(|(e, _)| *e); // Sort for determinism

    let collision_dist_sq = (radius * 2.0) * (radius * 2.0);

    let mut colliding_entities = std::collections::HashSet::new();

    // N^2 check
    for i in 0..units.len() {
        for j in (i + 1)..units.len() {
            let (e1, p1) = units[i];
            let (e2, p2) = units[j];

            if p1.distance_squared(p2) < collision_dist_sq {
                colliding_entities.insert(e1);
                colliding_entities.insert(e2);
            }
        }
    }

    // Sync component state
    for (entity, _) in query.iter() {
        if colliding_entities.contains(&entity) {
            commands.entity(entity).insert(Colliding);
        } else {
            commands.entity(entity).remove::<Colliding>();
        }
    }
}

fn resolve_collisions(
    mut query: Query<(Entity, &SimPosition, &mut SimVelocity)>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
) {
    let Some(config) = game_configs.get(&config_handle.0) else { return };
    let radius = config.unit_radius;
    let min_dist = radius * 2.0;
    let min_dist_sq = min_dist * min_dist;
    let strength = config.collision_push_strength;
    
    // Collect and sort for determinism
    let mut units: Vec<_> = query.iter_mut().collect();
    units.sort_by_key(|(e, _, _)| *e);

    let mut impulses = vec![Vec2::ZERO; units.len()];

    for i in 0..units.len() {
        for j in (i + 1)..units.len() {
            let (_, pos1, _) = units[i];
            let (_, pos2, _) = units[j];
            
            let delta = pos1.0 - pos2.0;
            let dist_sq = delta.length_squared();
            
            if dist_sq < min_dist_sq && dist_sq > 0.0001 {
                let dist = dist_sq.sqrt();
                let overlap = min_dist - dist;
                let dir = delta / dist;
                
                let impulse = dir * overlap * strength;
                
                impulses[i] += impulse;
                impulses[j] -= impulse;
            }
        }
    }

    // Apply impulses
    for (i, (_, _, vel)) in units.iter_mut().enumerate() {
        vel.0 += impulses[i];
    }
}

fn apply_global_flow(
    flow: Res<GlobalFlow>,
    mut query: Query<&mut SimVelocity>,
) {
    if flow.velocity.length_squared() > 0.0 {
        for mut vel in query.iter_mut() {
            vel.0 += flow.velocity;
        }
    }
}

fn resolve_obstacle_collisions(
    mut units: Query<(&SimPosition, &mut SimVelocity), Without<StaticObstacle>>,
    obstacles: Query<(&SimPosition, &StaticObstacle)>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
) {
    let Some(config) = game_configs.get(&config_handle.0) else { return };
    let unit_radius = config.unit_radius;
    let strength = config.obstacle_push_strength;
    
    for (u_pos, mut u_vel) in units.iter_mut() {
        for (o_pos, obstacle) in obstacles.iter() {
            let min_dist = unit_radius + obstacle.radius;
            let min_dist_sq = min_dist * min_dist;
            
            let delta = u_pos.0 - o_pos.0;
            let dist_sq = delta.length_squared();
            
            if dist_sq < min_dist_sq && dist_sq > 0.0001 {
                // info!("Obstacle collision detected!");
                let dist = dist_sq.sqrt();
                let overlap = min_dist - dist;
                let dir = delta / dist;
                
                let impulse = dir * overlap * strength;
                
                u_vel.0 += impulse;
            }
        }
    }
}
