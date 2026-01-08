use bevy::prelude::*;
use bevy_common_assets::ron::RonAssetPlugin;
use serde::{Deserialize, Serialize};

/// Static configuration loaded once at startup. These values define fundamental
/// game parameters that should not change during gameplay (e.g., physics constants,
/// initial map size). Changing these mid-game would break determinism in multiplayer.
#[derive(Resource, Deserialize, Serialize, Clone, Debug)]
pub struct InitialConfig {
    // Physics & Simulation (deterministic, must not change mid-game)
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
    pub max_force: f32,
    pub steering_force: f32,
    pub repulsion_force: f32,
    pub repulsion_decay: f32,
    pub friction: f32,
    pub min_velocity: f32,
    pub braking_force: f32,
    pub touch_dist_multiplier: f32,
    pub check_dist_multiplier: f32,
    
    // Boids parameters
    pub separation_weight: f32,
    pub alignment_weight: f32,
    pub cohesion_weight: f32,
    pub neighbor_radius: f32,
    pub separation_radius: f32,
    pub boids_max_neighbors: usize,
    
    // Force sources
    pub black_hole_strength: f32,
    pub wind_spot_strength: f32,
    pub force_source_radius: f32,

    // Editor defaults
    pub editor_num_obstacles: usize,
    pub editor_obstacle_min_radius: f32,
    pub editor_obstacle_max_radius: f32,
    pub editor_default_obstacle_radius: f32,
    pub editor_map_size_x: f32,
    pub editor_map_size_y: f32,
    
    // Pathfinding settings
    pub pathfinding_build_batch_size: usize,
}

/// Runtime configuration that can be hot-reloaded during gameplay.
/// These are settings that don't affect determinism (controls, camera, UI, debug).
#[derive(Deserialize, Serialize, Asset, TypePath, Clone, Debug)]
pub struct GameConfig {
    // Controls (hot-reloadable)
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
    pub key_spawn_batch: KeyCode,
    pub key_pause: KeyCode,
    pub key_toggle_health_bars: KeyCode,
    pub key_clear_force_sources: KeyCode,

    // Camera (hot-reloadable)
    pub camera_speed: f32,
    pub camera_zoom_speed: f32,

    // UI (hot-reloadable)
    pub selection_drag_threshold: f32,
    pub selection_click_radius: f32,
    
    // Debug visualization (hot-reloadable)
    pub debug_flow_field_view_radius: f32,
    pub debug_path_trace_max_steps: usize,
    pub debug_unit_lod_height_threshold: f32,
}

#[derive(Resource)]
pub struct GameConfigHandle(pub Handle<GameConfig>);

pub struct GameConfigPlugin;

impl Plugin for GameConfigPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(RonAssetPlugin::<GameConfig>::new(&["game_config.ron"]))
           .add_systems(Startup, (load_initial_config, setup_runtime_config).chain());
    }
}

/// Load static initial configuration synchronously at startup.
/// This must complete before any game state that depends on these values.
fn load_initial_config(mut commands: Commands) {
    let initial_config_path = "assets/initial_config.ron";
    
    match std::fs::read_to_string(initial_config_path) {
        Ok(contents) => {
            match ron::from_str::<InitialConfig>(&contents) {
                Ok(config) => {
                    info!("Loaded initial config from {}", initial_config_path);
                    commands.insert_resource(config);
                }
                Err(e) => {
                    error!("Failed to parse initial config: {}", e);
                    error!("Using default InitialConfig");
                    commands.insert_resource(InitialConfig::default());
                }
            }
        }
        Err(e) => {
            error!("Failed to read {}: {}", initial_config_path, e);
            error!("Using default InitialConfig");
            commands.insert_resource(InitialConfig::default());
        }
    }
}

/// Load runtime configuration asynchronously (can be hot-reloaded).
fn setup_runtime_config(mut commands: Commands, asset_server: Res<AssetServer>) {
    let handle = asset_server.load("game_config.ron");
    commands.insert_resource(GameConfigHandle(handle));
}

impl Default for InitialConfig {
    fn default() -> Self {
        Self {
            tick_rate: 30.0,
            unit_speed: 10.0,
            map_width: 2048.0,
            map_height: 2048.0,
            unit_radius: 0.5,
            collision_push_strength: 1.0,
            collision_restitution: 0.5,
            collision_drag: 0.02,
            collision_iterations: 4,
            collision_search_radius_multiplier: 4.0,
            obstacle_search_range: 1,
            epsilon: 0.0001,
            obstacle_push_strength: 1.0,
            arrival_threshold: 0.01,
            max_force: 40.0,
            steering_force: 40.0,
            repulsion_force: 20.0,
            repulsion_decay: 2.0,
            friction: 0.98,
            min_velocity: 0.01,
            braking_force: 5.0,
            touch_dist_multiplier: 2.1,
            check_dist_multiplier: 4.0,
            separation_weight: 1.5,
            alignment_weight: 1.0,
            cohesion_weight: 1.0,
            neighbor_radius: 5.0,
            separation_radius: 1.5,
            boids_max_neighbors: 8,
            black_hole_strength: 50.0,
            wind_spot_strength: -50.0,
            force_source_radius: 10.0,
            editor_num_obstacles: 50,
            editor_obstacle_min_radius: 10.0,
            editor_obstacle_max_radius: 50.0,
            editor_default_obstacle_radius: 20.0,
            editor_map_size_x: 2048.0,
            editor_map_size_y: 2048.0,
            pathfinding_build_batch_size: 5,
        }
    }
}
