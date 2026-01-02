use bevy::prelude::*;
use bevy_common_assets::ron::RonAssetPlugin;
use serde::Deserialize;

#[derive(Deserialize, Asset, TypePath, Clone, Debug)]
pub struct GameConfig {
    pub tick_rate: f64,
    pub unit_speed: f32,
    pub map_width: f32,
    pub map_height: f32,
    pub unit_radius: f32,
    pub collision_push_strength: f32,
    pub collision_restitution: f32,
    pub collision_drag: f32,
    pub collision_iterations: usize,
    pub collision_search_radius_multiplier: f32,
    pub obstacle_search_range: i32,
    pub epsilon: f32,
    pub obstacle_push_strength: f32,
    pub arrival_threshold: f32,

    // Controls
    pub key_camera_forward: KeyCode,
    pub key_camera_backward: KeyCode,
    pub key_camera_left: KeyCode,
    pub key_camera_right: KeyCode,
    pub key_debug_flow: KeyCode,
    pub key_debug_graph: KeyCode,
    pub key_debug_path: KeyCode,
    pub key_spawn_black_hole: KeyCode,
    pub key_spawn_wind_spot: KeyCode,
    pub key_spawn_unit: KeyCode,

    // Camera
    pub camera_speed: f32,
    pub camera_zoom_speed: f32,

    // Gameplay
    pub black_hole_strength: f32,
    pub wind_spot_strength: f32,
    pub force_source_radius: f32,
    pub selection_drag_threshold: f32,
    pub selection_click_radius: f32,

    // Physics
    pub max_force: f32,
    pub steering_force: f32,
    pub repulsion_force: f32,
    pub repulsion_decay: f32,
    pub friction: f32,
    pub min_velocity: f32,
    pub braking_force: f32,
    pub touch_dist_multiplier: f32,
    pub check_dist_multiplier: f32,
    
    // Boids
    pub separation_weight: f32,
    pub alignment_weight: f32,
    pub cohesion_weight: f32,
    pub neighbor_radius: f32,
    pub separation_radius: f32,
}

#[derive(Resource)]
pub struct GameConfigHandle(pub Handle<GameConfig>);

pub struct GameConfigPlugin;

impl Plugin for GameConfigPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(RonAssetPlugin::<GameConfig>::new(&["game_config.ron"]))
           .add_systems(Startup, setup_config);
    }
}

fn setup_config(mut commands: Commands, asset_server: Res<AssetServer>) {
    let handle = asset_server.load("game_config.ron");
    commands.insert_resource(GameConfigHandle(handle));
}
