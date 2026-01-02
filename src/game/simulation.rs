use bevy::prelude::*;
use crate::game::config::{GameConfig, GameConfigHandle};
use crate::game::math::{FixedVec2, FixedNum};
use crate::game::flow_field::{FlowField, CELL_SIZE};
use crate::game::spatial_hash::SpatialHash;
use crate::game::pathfinding::{Path, HierarchicalGraph, CLUSTER_SIZE};
use crate::game::map::{self, MAP_VERSION};
use crate::game::GameState;
use std::time::{Instant, Duration};
// use std::collections::HashMap;

pub struct SimulationPlugin;

#[derive(Resource, Default)]
pub struct MapStatus {
    pub loaded: bool,
}

#[derive(Resource, Default)]
pub struct SimPerformance {
    pub start_time: Option<Instant>,
    pub last_duration: Duration,
}

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub enum SimSet {
    Input,      // Processing inputs into commands
    Steering,   // Calculating desired velocities (Pathfinding, Boids)
    Physics,    // Collision detection and resolution
    Integration // Applying velocity to position
}

#[derive(Component, Debug, Clone)]
pub struct ForceSource {
    pub force_type: ForceType,
    pub radius: FixedNum, 
}

#[derive(Debug, Clone, Copy)]
pub enum ForceType {
    Radial(FixedNum), // Strength. >0 attract, <0 repel.
    Directional(FixedVec2), // Vector force.
}

#[derive(Resource, Default)]
pub struct MapFlowField(pub FlowField);

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

#[derive(Event, Message, Debug, Clone)]
pub struct CollisionEvent {
    pub entity1: Entity,
    pub entity2: Entity,
    pub overlap: FixedNum,
    pub normal: FixedVec2,
}

pub mod layers {
    pub const NONE: u32 = 0;
    pub const UNIT: u32 = 1 << 0;
    pub const OBSTACLE: u32 = 1 << 1;
    pub const PROJECTILE: u32 = 1 << 2;
    pub const ALL: u32 = u32::MAX;
}

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
    pub black_hole_strength: FixedNum,
    pub wind_spot_strength: FixedNum,
    pub force_source_radius: FixedNum,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            tick_rate: 30.0,
            unit_speed: FixedNum::from_num(5.0),
            map_width: FixedNum::from_num(50.0),
            map_height: FixedNum::from_num(50.0),
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
            black_hole_strength: FixedNum::from_num(50.0),
            wind_spot_strength: FixedNum::from_num(-50.0),
            force_source_radius: FixedNum::from_num(10.0),
        }
    }
}

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

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        // Configure FixedUpdate
        // Time::<Fixed>::from_hz might be deprecated or removed.
        // Using from_seconds(1.0 / 20.0)
        app.insert_resource(Time::<Fixed>::from_seconds(1.0 / 20.0)); 
        app.init_resource::<SimConfig>();
        app.init_resource::<SimPerformance>();
        app.insert_resource(SpatialHash::new(FixedNum::from_num(100), FixedNum::from_num(100), FixedNum::from_num(2)));
        // app.init_resource::<FlowFieldCache>();
        app.insert_resource(MapFlowField(FlowField::default()));
        app.init_resource::<MapStatus>();
        app.init_resource::<DebugConfig>();
        // app.register_type::<GlobalFlow>(); // Removed Reflect
        app.add_message::<UnitMoveCommand>();
        app.add_message::<SpawnUnitCommand>();
        app.add_message::<CollisionEvent>();

        // Configure System Sets
        app.configure_sets(FixedUpdate, (
            SimSet::Input,
            SimSet::Steering,
            SimSet::Integration,
            SimSet::Physics,
        ).chain().run_if(in_state(GameState::InGame).or(in_state(GameState::Editor))));

        // Register Systems
        app.add_systems(Startup, init_flow_field);
        app.add_systems(Update, (update_sim_from_config, apply_new_obstacles, toggle_debug, draw_flow_field_gizmos, draw_force_sources, draw_unit_paths).run_if(in_state(GameState::InGame).or(in_state(GameState::Editor))));
        app.add_systems(FixedUpdate, (
            sim_start.before(SimSet::Input),
            cache_previous_state.in_set(SimSet::Input),
            process_input.in_set(SimSet::Input),
            check_arrival_crowding.in_set(SimSet::Steering).before(follow_path),
            apply_friction.in_set(SimSet::Steering).before(follow_path),
            follow_path.in_set(SimSet::Steering),
            apply_forces.in_set(SimSet::Steering).before(follow_path),
            apply_velocity.in_set(SimSet::Integration),
            update_spatial_hash.in_set(SimSet::Physics).before(detect_collisions).before(resolve_collisions),
            constrain_to_map_bounds.in_set(SimSet::Physics),
            detect_collisions.in_set(SimSet::Physics).before(resolve_collisions),
            resolve_collisions.in_set(SimSet::Physics),
            resolve_obstacle_collisions.in_set(SimSet::Physics),
            sim_end.after(SimSet::Physics),
        ));
    }
}

fn update_sim_from_config(
    mut fixed_time: ResMut<Time<Fixed>>,
    mut sim_config: ResMut<SimConfig>,
    mut spatial_hash: ResMut<SpatialHash>,
    mut map_flow_field: ResMut<MapFlowField>,
    mut graph: ResMut<HierarchicalGraph>,
    mut map_status: ResMut<MapStatus>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    mut events: MessageReader<AssetEvent<GameConfig>>,
    obstacles: Query<(Entity, &SimPosition, &StaticObstacle)>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
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
                 sim_config.collision_restitution = FixedNum::from_num(config.collision_restitution);
                 sim_config.collision_drag = FixedNum::from_num(config.collision_drag);
                 sim_config.collision_iterations = config.collision_iterations;
                 sim_config.collision_search_radius_multiplier = FixedNum::from_num(config.collision_search_radius_multiplier);
                 sim_config.obstacle_search_range = config.obstacle_search_range;
                 sim_config.epsilon = FixedNum::from_num(config.epsilon);
                 sim_config.obstacle_push_strength = FixedNum::from_num(config.obstacle_push_strength);
                 sim_config.friction = FixedNum::from_num(config.friction);
                 sim_config.min_velocity = FixedNum::from_num(config.min_velocity);
                 sim_config.braking_force = FixedNum::from_num(config.braking_force);
                 sim_config.touch_dist_multiplier = FixedNum::from_num(config.touch_dist_multiplier);
                 sim_config.check_dist_multiplier = FixedNum::from_num(config.check_dist_multiplier);
                 sim_config.arrival_threshold = FixedNum::from_num(config.arrival_threshold);
                 sim_config.max_force = FixedNum::from_num(config.max_force);
                 sim_config.steering_force = FixedNum::from_num(config.steering_force);
                 sim_config.repulsion_force = FixedNum::from_num(config.repulsion_force);
                 sim_config.repulsion_decay = FixedNum::from_num(config.repulsion_decay);
                 sim_config.separation_weight = FixedNum::from_num(config.separation_weight);
                 sim_config.alignment_weight = FixedNum::from_num(config.alignment_weight);
                 sim_config.cohesion_weight = FixedNum::from_num(config.cohesion_weight);
                 sim_config.neighbor_radius = FixedNum::from_num(config.neighbor_radius);
                 sim_config.separation_radius = FixedNum::from_num(config.separation_radius);
                 sim_config.black_hole_strength = FixedNum::from_num(config.black_hole_strength);
                 sim_config.wind_spot_strength = FixedNum::from_num(config.wind_spot_strength);
                 sim_config.force_source_radius = FixedNum::from_num(config.force_source_radius);

                 // Resize spatial hash. Cell size should be at least diameter of unit (2 * radius)
                 // Using neighbor_radius might be better if we use it for boids too.
                 // But for collision, 2*radius is enough.
                 // Let's use max(2*radius, neighbor_radius) to support both if we want to use it for boids later.
                 // For now, just 2*radius + margin.
                 let cell_size = sim_config.unit_radius * FixedNum::from_num(2.0) * FixedNum::from_num(1.5);
                 spatial_hash.resize(sim_config.map_width, sim_config.map_height, cell_size);

                 let mut loaded_from_file = false;
                 let map_path = "assets/maps/default.pmap";

                 if !map_status.loaded {
                     if let Ok(map_data) = map::load_map(map_path) {
                         if map_data.version == MAP_VERSION && 
                            map_data.cell_size == FixedNum::from_num(CELL_SIZE) &&
                            map_data.cluster_size == CLUSTER_SIZE {
                             
                             info!("Loading map from {}", map_path);
                             
                             // Despawn existing obstacles
                             for (e, _, _) in obstacles.iter() {
                                 commands.entity(e).despawn();
                             }

                             // Spawn obstacles from map
                             for obs in &map_data.obstacles {
                                 commands.spawn((
                                     crate::game::GameEntity,
                                     Mesh3d(meshes.add(Cylinder::new(obs.radius.to_num(), 2.0))),
                                     MeshMaterial3d(materials.add(Color::srgb(0.5, 0.5, 0.5))),
                                     Transform::from_xyz(obs.position.x.to_num(), 1.0, obs.position.y.to_num()),
                                     SimPosition(obs.position),
                                     StaticObstacle { radius: obs.radius },
                                     Collider {
                                         radius: obs.radius,
                                         layer: layers::OBSTACLE,
                                         mask: layers::UNIT,
                                     },
                                 ));
                             }

                             // Set FlowField
                             let width = (map_data.map_width / map_data.cell_size).ceil().to_num::<usize>();
                             let height = (map_data.map_height / map_data.cell_size).ceil().to_num::<usize>();
                             let origin = FixedVec2::new(
                                 -map_data.map_width / FixedNum::from_num(2.0),
                                 -map_data.map_height / FixedNum::from_num(2.0),
                             );
                             
                             let mut flow_field = FlowField::new(width, height, map_data.cell_size, origin);
                             flow_field.cost_field = map_data.cost_field;
                             map_flow_field.0 = flow_field;

                             // Set Graph
                             *graph = map_data.graph;
                             graph.initialized = true;

                             loaded_from_file = true;
                             map_status.loaded = true;
                         }
                     }
                 }

                 if !loaded_from_file {
                     // Resize MapFlowField
                     let width = (sim_config.map_width / FixedNum::from_num(CELL_SIZE)).ceil().to_num::<usize>();
                     let height = (sim_config.map_height / FixedNum::from_num(CELL_SIZE)).ceil().to_num::<usize>();
                     let origin = FixedVec2::new(
                         -sim_config.map_width / FixedNum::from_num(2.0),
                         -sim_config.map_height / FixedNum::from_num(2.0),
                     );
                     map_flow_field.0 = FlowField::new(width, height, FixedNum::from_num(CELL_SIZE), origin);
                     
                     if !map_status.loaded {
                         // Despawn existing
                         for (e, _, _) in obstacles.iter() {
                             commands.entity(e).despawn();
                         }
                         
                         // Default Obstacle
                         let obstacle_pos = FixedVec2::from_f32(5.0, 5.0);
                         let obstacle_radius = FixedNum::from_num(2.0);
                         
                         commands.spawn((
                             crate::game::GameEntity,
                             Mesh3d(meshes.add(Cylinder::new(obstacle_radius.to_num(), 2.0))),
                             MeshMaterial3d(materials.add(Color::srgb(0.5, 0.5, 0.5))),
                             Transform::from_xyz(obstacle_pos.x.to_num(), 1.0, obstacle_pos.y.to_num()),
                             SimPosition(obstacle_pos),
                             StaticObstacle { radius: obstacle_radius },
                             Collider {
                                 radius: obstacle_radius,
                                 layer: layers::OBSTACLE,
                                 mask: layers::UNIT,
                             },
                         ));

                         // Apply to flow field
                         let flow_field = &mut map_flow_field.0;
                         apply_obstacle_to_flow_field(flow_field, obstacle_pos, obstacle_radius);
                         
                         map_status.loaded = true;
                     } else {
                         // Re-apply obstacles
                         let flow_field = &mut map_flow_field.0;
                         for (_, pos, obs) in obstacles.iter() {
                             apply_obstacle_to_flow_field(flow_field, pos.0, obs.radius);
                         }
                     }

                     // Reset Graph
                     graph.reset();
                 }

                 info!("Updated tick rate to {}", config.tick_rate);
             }
        }
    }
}

use crate::game::pathfinding::PathRequest;

fn process_input(
    mut commands: Commands,
    mut move_events: MessageReader<UnitMoveCommand>,
    mut spawn_events: MessageReader<SpawnUnitCommand>,
    mut path_requests: MessageWriter<PathRequest>,
    query: Query<&SimPosition>,
) {
    // Deterministic Input Processing:
    // 1. Collect all events
    // 2. Sort by Player ID (and potentially sequence number if we had one)
    // 3. Execute in order
    
    // Handle Move Commands
    let mut moves: Vec<&UnitMoveCommand> = move_events.read().collect();
    moves.sort_by_key(|e| e.player_id);
    
    for event in moves {
        if let Ok(pos) = query.get(event.entity) {
            // Send Path Request instead of setting SimTarget directly
            path_requests.write(PathRequest {
                entity: event.entity,
                start: pos.0,
                goal: event.target,
            });
            // Remove old target/path components to stop movement until path is found
            commands.entity(event.entity).remove::<SimTarget>();
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
            SimPosition(event.position),
            SimPositionPrev(event.position),
            SimVelocity(FixedVec2::ZERO),
            SimAcceleration(FixedVec2::ZERO),
            Collider::default(),
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

/// Logical acceleration of an entity.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct SimAcceleration(pub FixedVec2);

/// Logical target position for movement.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct SimTarget(#[allow(dead_code)] pub FixedVec2);

/// Component to mark if a unit is currently colliding with another unit.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Colliding;

/// Component for static circular obstacles.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct StaticObstacle {
    #[allow(dead_code)]
    pub radius: FixedNum,
}

/// Component to mark obstacles that are part of the flow field.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct FlowFieldObstacle;

fn cache_previous_state(
    mut query: Query<(&mut SimPositionPrev, &SimPosition)>,
) {
    for (mut prev, pos) in query.iter_mut() {
        prev.0 = pos.0;
    }
}

fn apply_velocity(
    sim_config: Res<SimConfig>,
    mut query: Query<(&mut SimPosition, &mut SimVelocity, &mut SimAcceleration)>,
) {
    let delta = FixedNum::from_num(1.0) / FixedNum::from_num(sim_config.tick_rate);

    for (mut pos, mut vel, mut acc) in query.iter_mut() {
        // Apply acceleration
        if acc.0.length_squared() > FixedNum::ZERO {
            vel.0 = vel.0 + acc.0 * delta;
            // Limit velocity to max speed? Or let drag handle it?
            // Let's clamp it for safety, though drag is better.
            // Actually, let's not clamp here, let steering/drag handle it.
            // But we should reset acceleration.
            acc.0 = FixedVec2::ZERO;
        }

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
    query: Query<(Entity, &SimPosition, &Collider)>,
    spatial_hash: Res<SpatialHash>,
    sim_config: Res<SimConfig>,
    mut events: MessageWriter<CollisionEvent>,
) {
    let mut colliding_entities = std::collections::HashSet::new();

    // We need to check collisions.
    // To avoid duplicates, we can enforce order, but SpatialHash returns neighbors which might include entities "behind" us in iteration order.
    // A simple way is to iterate all, check neighbors, and only process if entity1 < entity2.
    
    for (entity, pos, collider) in query.iter() {
        // Query radius: my radius + max possible neighbor radius.
        // For now, we assume max neighbor radius is similar to ours or use a safe upper bound.
        // Let's use 2.0 * radius as a safe search radius for now, assuming units are similar size.
        // Ideally SpatialHash should handle this better.
        let search_radius = collider.radius * sim_config.collision_search_radius_multiplier; 
        
        let potential_collisions = spatial_hash.get_potential_collisions(pos.0, search_radius);
        
        for (other_entity, _) in potential_collisions {
            if entity >= other_entity { continue; } // Avoid self and duplicates
            
            if let Ok((_, other_pos, other_collider)) = query.get(other_entity) {
                // Check layers
                if (collider.mask & other_collider.layer) == 0 && (other_collider.mask & collider.layer) == 0 {
                    continue;
                }

                let min_dist = collider.radius + other_collider.radius;
                let min_dist_sq = min_dist * min_dist;

                let delta = pos.0 - other_pos.0;
                let dist_sq = delta.length_squared();
                
                if dist_sq < min_dist_sq {
                    colliding_entities.insert(entity);
                    colliding_entities.insert(other_entity);
                    
                    let dist = dist_sq.sqrt();
                    let overlap = min_dist - dist;
                    let normal = if dist > sim_config.epsilon {
                        delta / dist
                    } else {
                        FixedVec2::new(FixedNum::ONE, FixedNum::ZERO) // Arbitrary
                    };

                    events.write(CollisionEvent {
                        entity1: entity,
                        entity2: other_entity,
                        overlap,
                        normal,
                    });
                }
            }
        }
    }

    // Sync component state
    for (entity, _, _) in query.iter() {
        if colliding_entities.contains(&entity) {
            commands.entity(entity).insert(Colliding);
        } else {
            commands.entity(entity).remove::<Colliding>();
        }
    }
}

fn apply_friction(
    mut query: Query<&mut SimVelocity>,
    sim_config: Res<SimConfig>,
) {
    let friction = sim_config.friction;
    let min_velocity_sq = sim_config.min_velocity * sim_config.min_velocity;
    for mut vel in query.iter_mut() {
        vel.0 = vel.0 * friction;
        if vel.0.length_squared() < min_velocity_sq {
            vel.0 = FixedVec2::ZERO;
        }
    }
}

fn apply_forces(
    mut units: Query<(&SimPosition, &mut SimAcceleration)>,
    sources: Query<(&SimPosition, &ForceSource)>,
) {
    for (u_pos, mut u_acc) in units.iter_mut() {
        for (s_pos, source) in sources.iter() {
             let delta = s_pos.0 - u_pos.0;
             let dist_sq = delta.length_squared();
             
             // Check radius
             if source.radius > FixedNum::ZERO {
                 let r_sq = source.radius * source.radius;
                 if dist_sq > r_sq { continue; }
             }

             match source.force_type {
                 ForceType::Radial(strength) => {
                     let dist = dist_sq.sqrt();
                     if dist > FixedNum::from_num(0.1) {
                         let dir = delta / dist;
                         u_acc.0 = u_acc.0 + dir * strength;
                     }
                 },
                 ForceType::Directional(dir) => {
                     u_acc.0 = u_acc.0 + dir;
                 }
             }
        }
    }
}

fn resolve_collisions(
    mut query: Query<&mut SimAcceleration>,
    sim_config: Res<SimConfig>,
    mut events: MessageReader<CollisionEvent>,
) {
    let repulsion_strength = sim_config.repulsion_force;
    let decay = sim_config.repulsion_decay;
    
    for event in events.read() {
        // Apply repulsion force based on overlap
        // Force increases as overlap increases
        let force_mag = repulsion_strength * (FixedNum::ONE + event.overlap * decay);
        let force = event.normal * force_mag;
        
        // Apply to entity 1
        if let Ok(mut acc1) = query.get_mut(event.entity1) {
            acc1.0 = acc1.0 + force;
        }
        
        // Apply to entity 2 (opposite direction)
        if let Ok(mut acc2) = query.get_mut(event.entity2) {
            acc2.0 = acc2.0 - force;
        }
    }
}

fn resolve_obstacle_collisions(
    mut units: Query<(Entity, &SimPosition, &mut SimAcceleration, &Collider), Without<StaticObstacle>>,
    map_flow_field: Res<MapFlowField>,
    sim_config: Res<SimConfig>,
    free_obstacles: Query<(&SimPosition, &Collider), (With<StaticObstacle>, Without<FlowFieldObstacle>)>,
) {
    let repulsion_strength = sim_config.repulsion_force;
    let decay = sim_config.repulsion_decay;
    let flow_field = &map_flow_field.0;
    let obstacle_radius = flow_field.cell_size / FixedNum::from_num(2.0);
    
    for (_entity, u_pos, mut u_acc, u_collider) in units.iter_mut() {
        let unit_radius = u_collider.radius;
        let min_dist = unit_radius + obstacle_radius;
        let min_dist_sq = min_dist * min_dist;

        if let Some((cx, cy)) = flow_field.world_to_grid(u_pos.0) {
            // Check 3x3 neighbors
            let range = sim_config.obstacle_search_range as usize;
            let min_x = if cx >= range { cx - range } else { 0 };
            let max_x = if cx + range < flow_field.width { cx + range } else { flow_field.width - 1 };
            let min_y = if cy >= range { cy - range } else { 0 };
            let max_y = if cy + range < flow_field.height { cy + range } else { flow_field.height - 1 };

            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    if flow_field.cost_field[flow_field.get_index(x, y)] == 255 {
                        let o_pos = flow_field.grid_to_world(x, y);
                        let delta = u_pos.0 - o_pos;
                        let dist_sq = delta.length_squared();
                        
                        if dist_sq < min_dist_sq && dist_sq > sim_config.epsilon {
                            let dist = dist_sq.sqrt();
                            let overlap = min_dist - dist;
                            let dir = delta / dist;

                            // Apply force
                            let force_mag = repulsion_strength * (FixedNum::ONE + overlap * decay);
                            u_acc.0 = u_acc.0 + dir * force_mag;
                        }
                    }
                }
            }
        }

        // Check free obstacles (not in flow field)
        for (obs_pos, obs_collider) in free_obstacles.iter() {
            let min_dist_free = unit_radius + obs_collider.radius;
            let min_dist_sq_free = min_dist_free * min_dist_free;

            let delta = u_pos.0 - obs_pos.0;
            let dist_sq = delta.length_squared();

            if dist_sq < min_dist_sq_free && dist_sq > sim_config.epsilon {
                let dist = dist_sq.sqrt();
                let overlap = min_dist_free - dist;
                let dir = delta / dist;

                // Apply force
                let force_mag = repulsion_strength * (FixedNum::ONE + overlap * decay);
                u_acc.0 = u_acc.0 + dir * force_mag;
            }
        }
    }
}

fn init_flow_field(
    mut map_flow_field: ResMut<MapFlowField>,
) {
    let width = 50;
    let height = 50;
    let cell_size = FixedNum::from_num(CELL_SIZE);
    let origin = FixedVec2::new(
        FixedNum::from_num(-(width as f32) * CELL_SIZE / 2.0),
        FixedNum::from_num(-(height as f32) * CELL_SIZE / 2.0),
    );

    map_flow_field.0 = FlowField::new(width, height, cell_size, origin);
}

fn apply_obstacle_to_flow_field(flow_field: &mut FlowField, pos: FixedVec2, radius: FixedNum) {
    // Rasterize circle
    // Even if center is outside, part of it might be inside.
    // But world_to_grid returns None if outside.
    // We should compute bounding box in grid coords.
    
    let min_world = pos - FixedVec2::new(radius, radius);
    let max_world = pos + FixedVec2::new(radius, radius);
    
    // Convert to grid coords manually to handle out of bounds
    let cell_size = flow_field.cell_size;
    let origin = flow_field.origin;
    
    let min_local = min_world - origin;
    let max_local = max_world - origin;
    
    let min_x = (min_local.x / cell_size).floor().to_num::<i32>();
    let min_y = (min_local.y / cell_size).floor().to_num::<i32>();
    let max_x = (max_local.x / cell_size).ceil().to_num::<i32>();
    let max_y = (max_local.y / cell_size).ceil().to_num::<i32>();
    
    for y in min_y..max_y {
        for x in min_x..max_x {
            if x >= 0 && x < flow_field.width as i32 && y >= 0 && y < flow_field.height as i32 {
                let cell_center = flow_field.grid_to_world(x as usize, y as usize);
                // Check if cell overlaps with circle.
                // Simple check: distance from circle center to cell center < radius + cell_radius
                // Or check if cell center is inside circle?
                // Or check if circle intersects AABB of cell?
                
                // Let's use a conservative check: if cell center is within radius + cell_radius/2
                // Actually, we want to block cells that are mostly covered?
                // Or block any cell that touches?
                // For pathfinding, blocking any touching cell is safer.
                
                let cell_radius = cell_size / FixedNum::from_num(2.0);
                let dist_sq = (cell_center - pos).length_squared();
                let threshold = radius + cell_radius;
                
                if dist_sq < threshold * threshold {
                    flow_field.set_obstacle(x as usize, y as usize);
                }
            }
        }
    }
}

fn apply_new_obstacles(
    mut map_flow_field: ResMut<MapFlowField>,
    obstacles: Query<(&SimPosition, &StaticObstacle), Added<StaticObstacle>>,
) {
    let flow_field = &mut map_flow_field.0;
    for (pos, obs) in obstacles.iter() {
        apply_obstacle_to_flow_field(flow_field, pos.0, obs.radius);
    }
}


fn toggle_debug(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut debug_config: ResMut<DebugConfig>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
) {
    let Some(config) = game_configs.get(&config_handle.0) else { return };

    if keyboard.just_pressed(config.key_debug_flow) {
        debug_config.show_flow_field = !debug_config.show_flow_field;
        info!("Flow field debug: {}", debug_config.show_flow_field);
    }
    if keyboard.just_pressed(config.key_debug_graph) {
        debug_config.show_pathfinding_graph = !debug_config.show_pathfinding_graph;
        info!("Pathfinding graph debug: {}", debug_config.show_pathfinding_graph);
    }
    if keyboard.just_pressed(config.key_debug_path) {
        debug_config.show_paths = !debug_config.show_paths;
        info!("Path debug: {}", debug_config.show_paths);
    }
}

fn draw_flow_field_gizmos(
    graph: Res<HierarchicalGraph>,
    map_flow_field: Res<MapFlowField>,
    debug_config: Res<DebugConfig>,
    mut gizmos: Gizmos,
    q_camera: Query<(&Camera, &GlobalTransform), With<crate::game::camera::RtsCamera>>,
) {
    if !debug_config.show_flow_field {
        return;
    }

    let Ok((camera, camera_transform)) = q_camera.single() else { return };
    let flow_field = &map_flow_field.0;

    // Get camera view bounds roughly
    let camera_pos = camera_transform.translation();
    // Simple distance check for now. 
    // A better way would be to project the frustum to the ground plane.
    // But for debug gizmos, a radius around the camera look-at point is fine.
    
    // Raycast to ground to find center of view
    let center_pos = if let Ok(ray) = camera.viewport_to_world(camera_transform, Vec2::new(640.0, 360.0)) { // Center of screen approx
         if ray.direction.y.abs() > 0.001 {
             let t = -ray.origin.y / ray.direction.y;
             if t >= 0.0 {
                 ray.origin + ray.direction * t
             } else {
                 camera_pos
             }
         } else {
             camera_pos
         }
    } else {
        camera_pos
    };

    let view_radius = 50.0; // Only draw within 50 units
    let _view_radius_sq = view_radius * view_radius;

    for ((cx, cy), cluster) in &graph.clusters {
        let min_x = cx * CLUSTER_SIZE;
        let min_y = cy * CLUSTER_SIZE;
        
        let center_x = (min_x as f32 + CLUSTER_SIZE as f32 / 2.0) * CELL_SIZE;
        let center_y = (min_y as f32 + CLUSTER_SIZE as f32 / 2.0) * CELL_SIZE;
        
        // Check if cluster is roughly in view
        let dist_sq = (center_x - center_pos.x).powi(2) + (center_y - center_pos.z).powi(2);
        if dist_sq > (view_radius + CLUSTER_SIZE as f32 * CELL_SIZE).powi(2) {
            continue;
        }

        // Debug: Draw a box around active clusters with cached fields
        if !cluster.flow_field_cache.is_empty() {
             gizmos.rect(
                 Isometry3d::new(
                     Vec3::new(center_x, 0.6, center_y),
                     Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                 ),
                 Vec2::new(CLUSTER_SIZE as f32 * CELL_SIZE, CLUSTER_SIZE as f32 * CELL_SIZE),
                 Color::srgb(1.0, 0.5, 0.0).with_alpha(0.3),
             );
        }

        for local_field in cluster.flow_field_cache.values() {
            for ly in 0..local_field.height {
                for lx in 0..local_field.width {
                    let idx = ly * local_field.width + lx;
                    if idx < local_field.vectors.len() {
                        let vec = local_field.vectors[idx];
                        if vec != FixedVec2::ZERO {
                            let gx = min_x + lx;
                            let gy = min_y + ly;
                            
                            let start = flow_field.grid_to_world(gx, gy).to_vec2();
                            
                            // Check individual arrow distance if needed, but cluster check is usually enough
                            
                            let end = start + vec.to_vec2() * 0.4; // Scale for visibility

                            gizmos.arrow(
                                Vec3::new(start.x, 0.6, start.y),
                                Vec3::new(end.x, 0.6, end.y),
                                Color::srgb(0.5, 0.5, 1.0), // Light blue for flow vectors
                            );
                        }
                    }
                }
            }
        }
    }
}

fn check_arrival_crowding(
    mut commands: Commands,
    query: Query<(Entity, &SimPosition, &SimTarget)>,
    other_units: Query<(Entity, &SimPosition, Option<&SimTarget>), With<SimPosition>>,
    sim_config: Res<SimConfig>,
    spatial_hash: Res<SpatialHash>,
) {
    let radius = sim_config.unit_radius;
    // Use a slightly larger radius for "touching" to be safe
    let touch_dist = radius * sim_config.touch_dist_multiplier;
    let collision_dist_sq = touch_dist * touch_dist; 
    // If within a reasonable distance to target (e.g. 3 unit radii or threshold)
    // If they are crowding, they might be a bit further than threshold.
    let check_dist = radius * sim_config.check_dist_multiplier;
    let check_dist_sq = check_dist * check_dist;

    for (entity, pos, target) in query.iter() {
        let dist_to_target_sq = (pos.0 - target.0).length_squared();
        
        if dist_to_target_sq < check_dist_sq {
            // Check for collision with arrived units
            let potential_collisions = spatial_hash.get_potential_collisions(pos.0, touch_dist);
            
            for (other_entity, _) in potential_collisions {
                if entity == other_entity { continue; }
                
                if let Ok((_, other_pos, other_target)) = other_units.get(other_entity) {
                    // If other unit has NO target, it is arrived/stopped.
                    if other_target.is_none() {
                        let dist_sq = (pos.0 - other_pos.0).length_squared();
                        if dist_sq < collision_dist_sq {
                            // Colliding with an arrived unit, and we are close to target.
                            // Consider arrived.
                            commands.entity(entity).remove::<SimTarget>();
                            // Also zero velocity to stop immediately
                            commands.entity(entity).insert(SimVelocity(FixedVec2::ZERO));
                            break; 
                        }
                    }
                }
            }
        }
    }
}

pub fn follow_path(
    mut commands: Commands,
    mut query: Query<(Entity, &SimPosition, &SimVelocity, &mut SimAcceleration, &mut Path)>,
    sim_config: Res<SimConfig>,
    graph: Res<HierarchicalGraph>,
    map_flow_field: Res<MapFlowField>,
) {
    let speed = sim_config.unit_speed;
    let max_force = sim_config.steering_force;
    let dt = FixedNum::ONE / FixedNum::from_num(sim_config.tick_rate);
    let step_dist = speed * dt;
    let threshold = if step_dist > sim_config.arrival_threshold { step_dist } else { sim_config.arrival_threshold };
    let threshold_sq = threshold * threshold;
    
    let flow_field = &map_flow_field.0;

    for (entity, pos, vel, mut acc, mut path) in query.iter_mut() {
        match &mut *path {
            Path::Direct(target) => {
                let delta = *target - pos.0;
                if delta.length_squared() < threshold_sq {
                    commands.entity(entity).remove::<Path>();
                    acc.0 = acc.0 - vel.0 * sim_config.braking_force;
                    continue;
                }
                seek(pos.0, *target, vel.0, &mut acc.0, speed, max_force);
            },
            Path::LocalAStar { waypoints, current_index } => {
                if *current_index >= waypoints.len() {
                    let braking_force = -vel.0 * sim_config.braking_force; 
                    acc.0 = acc.0 + braking_force;
                    continue;
                }

                let target = waypoints[*current_index];
                let delta = target - pos.0;
                let dist_sq = delta.length_squared();
                
                if dist_sq < threshold_sq {
                     *current_index += 1;
                     if *current_index >= waypoints.len() {
                         commands.entity(entity).remove::<Path>();
                     }
                     continue;
                }
                seek(pos.0, target, vel.0, &mut acc.0, speed, max_force);
            },
            Path::Hierarchical { portals, final_goal, current_index } => {
                if *current_index >= portals.len() {
                     let delta = *final_goal - pos.0;
                     if delta.length_squared() < threshold_sq {
                         commands.entity(entity).remove::<Path>();
                         acc.0 = acc.0 - vel.0 * sim_config.braking_force;
                         continue;
                     }
                     seek(pos.0, *final_goal, vel.0, &mut acc.0, speed, max_force);
                     continue;
                }

                let target_portal_id = portals[*current_index];
                
                let current_grid = flow_field.world_to_grid(pos.0);
                if let Some((gx, gy)) = current_grid {
                    let cx = gx / CLUSTER_SIZE;
                    let cy = gy / CLUSTER_SIZE;
                    let current_cluster_id = (cx, cy);
                    
                    if let Some(portal) = graph.nodes.get(target_portal_id) {
                        if current_cluster_id != portal.cluster {
                            if *current_index + 1 < portals.len() {
                                let next_portal_id = portals[*current_index + 1];
                                if let Some(next_portal) = graph.nodes.get(next_portal_id) {
                                    if next_portal.cluster == current_cluster_id {
                                        *current_index += 1;
                                    }
                                }
                            }
                        }
                        
                        let target_portal_id = portals[*current_index];
                        let portal = graph.nodes[target_portal_id].clone(); 
                        
                        if current_cluster_id == portal.cluster {
                             if let Some(cluster) = graph.clusters.get(&current_cluster_id) {
                                 let local_field = cluster.get_flow_field(target_portal_id);
                                 
                                 let min_x = cx * CLUSTER_SIZE;
                                 let min_y = cy * CLUSTER_SIZE;
                                 
                                 if gx >= min_x && gy >= min_y {
                                     let lx = gx - min_x;
                                     let ly = gy - min_y;
                                     let idx = ly * local_field.width + lx;
                                     
                                     if idx < local_field.vectors.len() {
                                         let dir = local_field.vectors[idx];
                                         if dir != FixedVec2::ZERO {
                                             let desired_vel = dir * speed;
                                             let steer = desired_vel - vel.0;
                                             let steer_len_sq = steer.length_squared();
                                             let final_steer = if steer_len_sq > max_force * max_force {
                                                 steer.normalize() * max_force
                                             } else {
                                                 steer
                                             };
                                             acc.0 = acc.0 + final_steer;
                                         } else {
                                             let target_pos = flow_field.grid_to_world(portal.node.x, portal.node.y);
                                             seek(pos.0, target_pos, vel.0, &mut acc.0, speed, max_force);
                                         }
                                     }
                                 }
                             }
                        } else {
                            let target_pos = flow_field.grid_to_world(portal.node.x, portal.node.y);
                            seek(pos.0, target_pos, vel.0, &mut acc.0, speed, max_force);
                        }
                    }
                }
            }
        }
    }
}

fn seek(pos: FixedVec2, target: FixedVec2, vel: FixedVec2, acc: &mut FixedVec2, speed: FixedNum, max_force: FixedNum) {
    let delta = target - pos;
    let dist_sq = delta.length_squared();
    if dist_sq > FixedNum::ZERO {
        let desired_vel = delta.normalize() * speed;
        let steer = desired_vel - vel;
        let steer_len_sq = steer.length_squared();
        let final_steer = if steer_len_sq > max_force * max_force {
            steer.normalize() * max_force
        } else {
            steer
        };
        *acc = *acc + final_steer;
    }
}

fn update_spatial_hash(
    mut spatial_hash: ResMut<SpatialHash>,
    query: Query<(Entity, &SimPosition)>,
) {
    spatial_hash.clear();
    for (entity, pos) in query.iter() {
        spatial_hash.insert(entity, pos.0);
    }
}

fn draw_force_sources(
    query: Query<(&Transform, &ForceSource)>,
    mut gizmos: Gizmos,
) {
    for (transform, source) in query.iter() {
        let color = match source.force_type {
            ForceType::Radial(strength) => {
                if strength > FixedNum::ZERO {
                    Color::srgb(0.5, 0.0, 0.5) // Purple for Black Hole
                } else {
                    Color::srgb(0.0, 1.0, 1.0) // Cyan for Wind
                }
            },
            ForceType::Directional(_) => Color::srgb(1.0, 1.0, 0.0),
        };
        
        let radius = source.radius.to_num::<f32>();
        gizmos.circle(transform.translation, radius, color);
        // Draw a smaller inner circle to indicate center
        gizmos.circle(transform.translation, 0.5, color);
    }
}

fn draw_unit_paths(
    query: Query<(&Transform, &Path)>,
    debug_config: Res<DebugConfig>,
    mut gizmos: Gizmos,
    map_flow_field: Res<MapFlowField>,
    graph: Res<HierarchicalGraph>,
) {
    if !debug_config.show_paths {
        return;
    }
    
    let flow_field = &map_flow_field.0;
    let nodes = graph.nodes.clone();

    for (transform, path) in query.iter() {
        let mut current_pos = transform.translation;
        current_pos.y = 0.6;

        match path {
            Path::Direct(target) => {
                let next_pos = Vec3::new(target.x.to_num(), 0.6, target.y.to_num());
                gizmos.line(current_pos, next_pos, Color::srgb(0.0, 1.0, 0.0));
                gizmos.sphere(next_pos, 0.2, Color::srgb(0.0, 1.0, 0.0));
            },
            Path::LocalAStar { waypoints, current_index } => {
                if *current_index >= waypoints.len() { continue; }
                for i in *current_index..waypoints.len() {
                    let wp = waypoints[i];
                    let next_pos = Vec3::new(wp.x.to_num(), 0.6, wp.y.to_num());
                    gizmos.line(current_pos, next_pos, Color::srgb(0.0, 1.0, 0.0));
                    gizmos.sphere(next_pos, 0.2, Color::srgb(0.0, 1.0, 0.0));
                    current_pos = next_pos;
                }
            },
            Path::Hierarchical { portals, final_goal, current_index } => {
                let mut trace_pos = FixedVec2::from_f32(current_pos.x, current_pos.z);
                
                for i in *current_index..portals.len() {
                    let portal_id = portals[i];
                    if let Some(portal) = nodes.get(portal_id) {
                        // Handle cluster transition if needed
                        let grid_pos_opt = flow_field.world_to_grid(trace_pos);
                        if let Some((gx, gy)) = grid_pos_opt {
                            let cx = gx / CLUSTER_SIZE;
                            let cy = gy / CLUSTER_SIZE;
                            
                            if (cx, cy) != portal.cluster {
                                // We are not in the correct cluster. Snap to entry portal if possible.
                                if i > 0 {
                                    let prev_id = portals[i-1];
                                    if let Some(prev_portal) = nodes.get(prev_id) {
                                        if prev_portal.cluster == portal.cluster {
                                            let snap_pos = flow_field.grid_to_world(prev_portal.node.x, prev_portal.node.y);
                                            gizmos.line(
                                                Vec3::new(trace_pos.x.to_num(), 0.6, trace_pos.y.to_num()),
                                                Vec3::new(snap_pos.x.to_num(), 0.6, snap_pos.y.to_num()),
                                                Color::srgb(0.0, 1.0, 0.0)
                                            );
                                            trace_pos = snap_pos;
                                        }
                                    }
                                }
                            }
                        }

                        // Get flow field for this portal
                        if let Some(cluster) = graph.clusters.get(&portal.cluster) {
                            let ff = cluster.get_flow_field(portal_id);
                            
                            // Trace
                            let mut steps = 0;
                            let max_steps = 200;
                            let step_size = FixedNum::from_num(0.5);
                            
                            while steps < max_steps {
                                let grid_pos_opt = flow_field.world_to_grid(trace_pos);
                                if let Some((gx, gy)) = grid_pos_opt {
                                    let cx = gx / CLUSTER_SIZE;
                                    let cy = gy / CLUSTER_SIZE;
                                    
                                    if (cx, cy) != portal.cluster {
                                        break;
                                    }
                                    
                                    let min_x = portal.cluster.0 * CLUSTER_SIZE;
                                    let min_y = portal.cluster.1 * CLUSTER_SIZE;
                                    let lx = gx - min_x;
                                    let ly = gy - min_y;
                                    
                                    if lx >= ff.width || ly >= ff.height { break; }
                                    
                                    let idx = ly * ff.width + lx;
                                    let dir = ff.vectors[idx];
                                    
                                    if dir == FixedVec2::ZERO {
                                        break;
                                    }
                                    
                                    let next_trace_pos = trace_pos + dir * step_size;
                                    
                                    gizmos.line(
                                        Vec3::new(trace_pos.x.to_num(), 0.6, trace_pos.y.to_num()),
                                        Vec3::new(next_trace_pos.x.to_num(), 0.6, next_trace_pos.y.to_num()),
                                        Color::srgb(0.0, 1.0, 0.0)
                                    );
                                    
                                    trace_pos = next_trace_pos;
                                } else {
                                    break;
                                }
                                steps += 1;
                            }
                        }
                    }
                }
                
                // Draw line to final goal
                let final_pos_vec = Vec3::new(final_goal.x.to_num(), 0.6, final_goal.y.to_num());
                gizmos.line(
                    Vec3::new(trace_pos.x.to_num(), 0.6, trace_pos.y.to_num()),
                    final_pos_vec,
                    Color::srgb(0.0, 1.0, 0.0)
                );
                gizmos.sphere(final_pos_vec, 0.2, Color::srgb(0.0, 1.0, 0.0));
            }
        }
    }
}

fn sim_start(mut stats: ResMut<SimPerformance>) {
    stats.start_time = Some(Instant::now());
}

fn sim_end(mut stats: ResMut<SimPerformance>) {
    if let Some(start) = stats.start_time {
        stats.last_duration = start.elapsed();
        // Log occasionally? Or just store it.
        // Let's log if it exceeds a threshold, e.g., 16ms (60fps)
        if stats.last_duration.as_millis() > 16 {
            warn!("Sim tick took too long: {:?}", stats.last_duration);
        }
    }
}
