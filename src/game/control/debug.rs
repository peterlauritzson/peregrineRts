use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use crate::game::simulation::{ForceSource, ForceType, SpawnUnitCommand, SimPosition};
use crate::game::fixed_math::{FixedVec2, FixedNum};
use crate::game::camera::RtsCamera;
use crate::game::config::{GameConfig, GameConfigHandle, InitialConfig};
use rand::{rng, Rng};

/// Handle debug spawning via keyboard shortcuts
pub fn handle_debug_spawning(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    q_window: Query<&Window, With<PrimaryWindow>>,
    q_camera: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    initial_config: Res<InitialConfig>,
    mut spawn_events: MessageWriter<SpawnUnitCommand>,
) {
    let Some((camera, camera_transform)) = q_camera.iter().next() else { return };
    let Some(window) = q_window.iter().next() else { return };
    let Some(cursor_position) = window.cursor_position() else { return };
    let Some(config) = game_configs.get(&config_handle.0) else { return };

    if keys.just_pressed(config.key_spawn_black_hole) || 
       keys.just_pressed(config.key_spawn_wind_spot) ||
       keys.just_pressed(config.key_spawn_unit) ||
       keys.just_pressed(config.key_spawn_batch) {
         let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) else { return };
        
        // Intersect with ground plane (y=0)
        let normal = Vec3::Y;
        let denom = ray.direction.dot(normal);

        if denom.abs() > 0.0001 {
            let t = -ray.origin.y / denom;
            if t >= 0.0 {
                let intersection_point = ray.origin + ray.direction * t;
                let pos_fixed = FixedVec2::from_f32(intersection_point.x, intersection_point.z);

                if keys.just_pressed(config.key_spawn_black_hole) {
                    // Spawn Black Hole (Attract)
                    info!("Spawning Black Hole at {:?}", pos_fixed);
                    commands.spawn((
                        crate::game::GameEntity,
                        Transform::from_translation(intersection_point),
                        GlobalTransform::default(),
                        SimPosition(pos_fixed),
                        ForceSource {
                            force_type: ForceType::Radial(FixedNum::from_num(initial_config.black_hole_strength)), // Positive = Attract
                            radius: FixedNum::from_num(initial_config.force_source_radius),
                        }
                    ));
                } else if keys.just_pressed(config.key_spawn_wind_spot) {
                    // Spawn Wind Spot (Repel)
                    info!("Spawning Wind Spot at {:?}", pos_fixed);
                     commands.spawn((
                        crate::game::GameEntity,
                        Transform::from_translation(intersection_point),
                        GlobalTransform::default(),
                        SimPosition(pos_fixed),
                        ForceSource {
                            force_type: ForceType::Radial(FixedNum::from_num(initial_config.wind_spot_strength)), // Negative = Repel
                            radius: FixedNum::from_num(initial_config.force_source_radius),
                        }
                    ));
                } else if keys.just_pressed(config.key_spawn_unit) {
                    info!("Spawning Unit at {:?}", pos_fixed);
                    spawn_events.write(SpawnUnitCommand {
                        player_id: 0,
                        position: pos_fixed,
                    });
                } else if keys.just_pressed(config.key_spawn_batch) {
                    info!("Spawning batch of units at {:?}", pos_fixed);
                    spawn_batch_at(&mut spawn_events, 100, intersection_point.x, intersection_point.z);
                }
            }
        }
    }
}

/// Spawn a batch of units around a center point
fn spawn_batch_at(
    spawn_events: &mut MessageWriter<SpawnUnitCommand>,
    count: usize,
    center_x: f32,
    center_z: f32,
) {
    let mut rng = rng();
    let spread = 50.0; // Spread units around the click
    
    for _ in 0..count {
        let pos_x = center_x + rng.random_range(-spread..spread);
        let pos_z = center_z + rng.random_range(-spread..spread);
        
        spawn_events.write(SpawnUnitCommand {
            player_id: 0,
            position: FixedVec2::from_f32(pos_x, pos_z),
        });
    }
}

/// Clear all force sources when the clear key is pressed
pub fn clear_force_sources(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    q_force_sources: Query<Entity, With<ForceSource>>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
) {
    let Some(config) = game_configs.get(&config_handle.0) else { return };
    
    if keys.just_pressed(config.key_clear_force_sources) {
        for entity in q_force_sources.iter() {
            info!("Despawning ForceSource entity");
            commands.entity(entity).despawn();
        }
    }
}
