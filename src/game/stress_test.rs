use bevy::prelude::*;
use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};
use bevy::window::PrimaryWindow;
use crate::game::simulation::{SpawnUnitCommand, SimConfig};
use crate::game::math::{FixedVec2, FixedNum};
use crate::game::spatial_hash::SpatialHash;
use crate::game::camera::RtsCamera;
use rand::{rng, Rng};

pub struct StressTestPlugin;

impl Plugin for StressTestPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            FrameTimeDiagnosticsPlugin::default(),
            LogDiagnosticsPlugin::default(),
        ))
        .add_systems(Startup, setup_stress_test)
        .add_systems(Update, handle_stress_test_input);
    }
}

fn setup_stress_test(
    mut sim_config: ResMut<SimConfig>,
    mut spatial_hash: ResMut<SpatialHash>,
) {
    let map_size = 2000.0; // Huge map
    
    // Update SimConfig
    sim_config.map_width = FixedNum::from_num(map_size);
    sim_config.map_height = FixedNum::from_num(map_size);
    
    // Resize SpatialHash
    spatial_hash.resize(sim_config.map_width, sim_config.map_height, FixedNum::from_num(2.0));

    info!("Stress Test Setup: Map size set to {}x{}. Press SPACE to spawn units.", map_size, map_size);
}

fn handle_stress_test_input(
    keys: Res<ButtonInput<KeyCode>>,
    q_window: Query<&Window, With<PrimaryWindow>>,
    q_camera: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
    mut spawn_events: MessageWriter<SpawnUnitCommand>,
) {
    if keys.just_pressed(KeyCode::Space) {
        let Some((camera, camera_transform)) = q_camera.iter().next() else { return };
        let Some(window) = q_window.iter().next() else { return };
        let Some(cursor_position) = window.cursor_position() else { return };

        // Raycast to ground plane (y=0)
        let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) else { return };
        
        // Plane is at y=0, normal is (0, 1, 0)
        // Ray: origin + t * direction
        // y = origin.y + t * direction.y = 0
        // t = -origin.y / direction.y
        
        if ray.direction.y.abs() > 0.0001 {
            let t = -ray.origin.y / ray.direction.y;
            if t >= 0.0 {
                let hit_point = ray.origin + ray.direction * t;
                info!("Spawning batch at {:?}", hit_point);
                spawn_batch_at(&mut spawn_events, 100, hit_point.x, hit_point.z);
            }
        }
    }
}

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
