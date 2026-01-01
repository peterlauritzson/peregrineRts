use bevy::prelude::*;
use bevy::input::mouse::MouseWheel;
use crate::game::config::{GameConfig, GameConfigHandle};

pub struct RtsCameraPlugin;

impl Plugin for RtsCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera)
           .add_systems(Update, move_camera);
    }
}

#[derive(Component)]
pub struct RtsCamera;

fn spawn_camera(mut commands: Commands) {
    // RTS Camera: High up, looking down at an angle
    let translation = Vec3::new(0.0, 15.0, 15.0);
    let look_at = Vec3::ZERO;

    commands.spawn((
        Camera3d::default(),
        Transform::from_translation(translation)
            .looking_at(look_at, Vec3::Y),
        RtsCamera,
    ));
}

fn move_camera(
    mut query: Query<&mut Transform, With<RtsCamera>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut scroll_evr: MessageReader<MouseWheel>,
    time: Res<Time>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
) {
    let Some(mut transform) = query.iter_mut().next() else { return };
    let Some(config) = game_configs.get(&config_handle.0) else { return };
    
    let mut velocity = Vec3::ZERO;
    let speed = config.camera_speed;
    let zoom_speed = config.camera_zoom_speed;

    // Forward/Backward (Z)
    if keys.pressed(config.key_camera_forward) {
        velocity.z -= 1.0;
    }
    if keys.pressed(config.key_camera_backward) {
        velocity.z += 1.0;
    }

    // Left/Right (X)
    if keys.pressed(config.key_camera_left) {
        velocity.x -= 1.0;
    }
    if keys.pressed(config.key_camera_right) {
        velocity.x += 1.0;
    }

    // Normalize velocity
    if velocity.length_squared() > 0.0 {
        velocity = velocity.normalize();
    }

    // Move in world XZ plane
    transform.translation.x += velocity.x * speed * time.delta_secs();
    transform.translation.z += velocity.z * speed * time.delta_secs();

    // Zoom (Scroll)
    for ev in scroll_evr.read() {
        let zoom = ev.y;
        // Move along the forward vector
        let forward = transform.forward();
        transform.translation += forward * zoom * zoom_speed * time.delta_secs();
    }
}
