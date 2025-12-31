use bevy::prelude::*;
use crate::game::config::{GameConfig, GameConfigHandle};
use crate::game::math::{FixedVec2, FixedNum};

pub struct SimulationPlugin;

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub enum SimSet {
    Input,      // Processing inputs into commands
    Steering,   // Calculating desired velocities (Pathfinding, Boids)
    Physics,    // Collision detection and resolution
    Integration // Applying velocity to position
}

#[derive(Resource, Default)]
pub struct GlobalFlow {
    pub velocity: FixedVec2,
}

#[derive(Event, Message, Debug, Clone)]
pub struct UnitMoveCommand {
    pub player_id: u8,
    pub entity: Entity,
    pub target: FixedVec2,
}

#[derive(Event, Message, Debug, Clone)]
pub struct SpawnUnitCommand {
    pub player_id: u8,
    pub position: FixedVec2,
}

#[derive(Resource, Default)]
pub struct SimConfig {
    pub tick_rate: f64,
    pub unit_speed: FixedNum,
    pub map_width: FixedNum,
    pub map_height: FixedNum,
    pub unit_radius: FixedNum,
    pub collision_push_strength: FixedNum,
    pub obstacle_push_strength: FixedNum,
    pub arrival_threshold: FixedNum,
    pub separation_weight: FixedNum,
    pub alignment_weight: FixedNum,
    pub cohesion_weight: FixedNum,
    pub neighbor_radius: FixedNum,
    pub separation_radius: FixedNum,
}

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        // Configure FixedUpdate
        // Time::<Fixed>::from_hz might be deprecated or removed.
        // Using from_seconds(1.0 / 20.0)
        app.insert_resource(Time::<Fixed>::from_seconds(1.0 / 20.0)); 
        app.init_resource::<GlobalFlow>();
        app.init_resource::<SimConfig>();
        // app.register_type::<GlobalFlow>(); // Removed Reflect
        app.add_message::<UnitMoveCommand>();
        app.add_message::<SpawnUnitCommand>();

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
            cache_previous_state.in_set(SimSet::Input),
            process_input.in_set(SimSet::Input),
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
    mut sim_config: ResMut<SimConfig>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    mut events: MessageReader<AssetEvent<GameConfig>>,
) {
    for event in events.read() {
        info!("Config event: {:?}", event);
        if event.is_modified(config_handle.0.id()) || event.is_loaded_with_dependencies(config_handle.0.id()) {
             if let Some(config) = game_configs.get(&config_handle.0) {
                 // fixed_time.set_timestep_hz(config.tick_rate);
                 fixed_time.set_timestep_seconds(1.0 / config.tick_rate);
                 
                 sim_config.tick_rate = config.tick_rate;
                 sim_config.unit_speed = FixedNum::from_num(config.unit_speed);
                 sim_config.map_width = FixedNum::from_num(config.map_width);
                 sim_config.map_height = FixedNum::from_num(config.map_height);
                 sim_config.unit_radius = FixedNum::from_num(config.unit_radius);
                 sim_config.collision_push_strength = FixedNum::from_num(config.collision_push_strength);
                 sim_config.obstacle_push_strength = FixedNum::from_num(config.obstacle_push_strength);
                 sim_config.arrival_threshold = FixedNum::from_num(config.arrival_threshold);
                 sim_config.separation_weight = FixedNum::from_num(config.separation_weight);
                 sim_config.alignment_weight = FixedNum::from_num(config.alignment_weight);
                 sim_config.cohesion_weight = FixedNum::from_num(config.cohesion_weight);
                 sim_config.neighbor_radius = FixedNum::from_num(config.neighbor_radius);
                 sim_config.separation_radius = FixedNum::from_num(config.separation_radius);

                 info!("Updated tick rate to {}", config.tick_rate);
             }
        }
    }
}

fn process_input(
    mut commands: Commands,
    mut move_events: MessageReader<UnitMoveCommand>,
    mut spawn_events: MessageReader<SpawnUnitCommand>,
) {
    // Deterministic Input Processing:
    // 1. Collect all events
    // 2. Sort by Player ID (and potentially sequence number if we had one)
    // 3. Execute in order
    
    // Handle Move Commands
    let mut moves: Vec<&UnitMoveCommand> = move_events.read().collect();
    moves.sort_by_key(|e| e.player_id);
    
    for event in moves {
        commands.entity(event.entity).insert(SimTarget(event.target));
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
            crate::game::unit::Unit,
            SimPosition(event.position),
            SimPositionPrev(event.position),
            SimVelocity(FixedVec2::ZERO),
        ));
    }
}

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

/// Logical target position for movement.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct SimTarget(pub FixedVec2);

/// Component to mark if a unit is currently colliding with another unit.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Colliding;

/// Component for static circular obstacles.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct StaticObstacle {
    pub radius: FixedNum,
}

fn cache_previous_state(
    mut query: Query<(&mut SimPositionPrev, &SimPosition)>,
) {
    for (mut prev, pos) in query.iter_mut() {
        prev.0 = pos.0;
    }
}

fn apply_velocity(
    sim_config: Res<SimConfig>,
    mut query: Query<(&mut SimPosition, &SimVelocity)>,
) {
    let delta = FixedNum::from_num(1.0) / FixedNum::from_num(sim_config.tick_rate);

    for (mut pos, vel) in query.iter_mut() {
        if vel.0.length_squared() > FixedNum::ZERO {
            pos.0 = pos.0 + vel.0 * delta;
        }
    }
}

fn constrain_to_map_bounds(
    mut query: Query<(&mut SimPosition, &mut SimVelocity)>,
    sim_config: Res<SimConfig>,
) {
    let half_w = sim_config.map_width / FixedNum::from_num(2.0);
    let half_h = sim_config.map_height / FixedNum::from_num(2.0);

    for (mut pos, mut vel) in query.iter_mut() {
        // 1. Clamp Position
        if pos.0.x < -half_w { pos.0.x = -half_w; }
        if pos.0.x > half_w { pos.0.x = half_w; }
        if pos.0.y < -half_h { pos.0.y = -half_h; }
        if pos.0.y > half_h { pos.0.y = half_h; }

        // 2. Zero Velocity against walls
        if pos.0.x <= -half_w && vel.0.x < FixedNum::ZERO { vel.0.x = FixedNum::ZERO; }
        if pos.0.x >= half_w && vel.0.x > FixedNum::ZERO { vel.0.x = FixedNum::ZERO; }
        if pos.0.y <= -half_h && vel.0.y < FixedNum::ZERO { vel.0.y = FixedNum::ZERO; }
        if pos.0.y >= half_h && vel.0.y > FixedNum::ZERO { vel.0.y = FixedNum::ZERO; }
    }
}

fn detect_collisions(
    mut commands: Commands,
    query: Query<(Entity, &SimPosition)>,
    sim_config: Res<SimConfig>,
) {
    let radius = sim_config.unit_radius;

    // Reset collision state
    // Note: In a real ECS, adding/removing components every frame can be expensive (archetype moves).
    // For now, it's fine for prototyping. A better way would be a boolean field or a resource.
    // Or we can just query for Colliding and remove it if not colliding.
    
    // First, collect all positions
    let mut units: Vec<(Entity, FixedVec2)> = query.iter().map(|(e, p)| (e, p.0)).collect();
    units.sort_by_key(|(e, _)| *e); // Sort for determinism

    let collision_dist_sq = (radius * FixedNum::from_num(2.0)) * (radius * FixedNum::from_num(2.0));

    let mut colliding_entities = std::collections::HashSet::new();

    // N^2 check
    for i in 0..units.len() {
        for j in (i + 1)..units.len() {
            let (e1, p1) = units[i];
            let (e2, p2) = units[j];

            // Distance squared check
            let delta = p1 - p2;
            if delta.length_squared() < collision_dist_sq {
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
    sim_config: Res<SimConfig>,
) {
    let radius = sim_config.unit_radius;
    let min_dist = radius * FixedNum::from_num(2.0);
    let min_dist_sq = min_dist * min_dist;
    let strength = sim_config.collision_push_strength;
    
    // Collect and sort for determinism
    let mut units: Vec<_> = query.iter_mut().collect();
    units.sort_by_key(|(e, _, _)| *e);

    let mut impulses = vec![FixedVec2::ZERO; units.len()];

    for i in 0..units.len() {
        for j in (i + 1)..units.len() {
            let (_, pos1, _) = units[i];
            let (_, pos2, _) = units[j];
            
            let delta = pos1.0 - pos2.0;
            let dist_sq = delta.length_squared();
            
            if dist_sq < min_dist_sq && dist_sq > FixedNum::from_num(0.0001) {
                let _dist = dist_sq.sqrt(); // Fixed point sqrt
                // Wait, FixedVec2::length() returns Fixed.
                // But here I have dist_sq which is Fixed.
                // I need sqrt of Fixed.
                // Assuming Fixed implements sqrt via num-traits or similar if available, 
                // or I can use a helper.
                // Since I don't have easy sqrt on Fixed without features, let's use a rough approximation or assume it works.
                // Actually, `fixed` crate types have `sqrt()` method.
                let dist = dist_sq.sqrt();

                let overlap = min_dist - dist;
                let dir = delta / dist;
                
                let impulse = dir * overlap * strength;
                
                impulses[i] = impulses[i] + impulse;
                impulses[j] = impulses[j] - impulse;
            }
        }
    }

    // Apply impulses
    for (i, (_, _, vel)) in units.iter_mut().enumerate() {
        vel.0 = vel.0 + impulses[i];
    }
}

fn apply_global_flow(
    flow: Res<GlobalFlow>,
    mut query: Query<&mut SimVelocity>,
) {
    if flow.velocity.length_squared() > FixedNum::ZERO {
        for mut vel in query.iter_mut() {
            vel.0 = vel.0 + flow.velocity;
        }
    }
}

fn resolve_obstacle_collisions(
    mut units: Query<(&SimPosition, &mut SimVelocity), Without<StaticObstacle>>,
    obstacles: Query<(&SimPosition, &StaticObstacle)>,
    sim_config: Res<SimConfig>,
) {
    let unit_radius = sim_config.unit_radius;
    let strength = sim_config.obstacle_push_strength;
    
    for (u_pos, mut u_vel) in units.iter_mut() {
        for (o_pos, obstacle) in obstacles.iter() {
            let min_dist = unit_radius + obstacle.radius;
            let min_dist_sq = min_dist * min_dist;
            
            let delta = u_pos.0 - o_pos.0;
            let dist_sq = delta.length_squared();
            
            if dist_sq < min_dist_sq && dist_sq > FixedNum::from_num(0.0001) {
                // info!("Obstacle collision detected!");
                let dist = dist_sq.sqrt();
                let overlap = min_dist - dist;
                let dir = delta / dist;
                
                let impulse = dir * overlap * strength;
                
                u_vel.0 = u_vel.0 + impulse;
            }
        }
    }
}
