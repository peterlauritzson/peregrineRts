/// Configuration initialization systems
///
/// Systems that handle loading and updating simulation configuration from:
/// - InitialConfig (loaded at startup from initial_config.ron)
/// - GameConfig (hot-reloadable runtime settings from game_config.ron)

use bevy::prelude::*;
use crate::game::config::{GameConfig, GameConfigHandle, InitialConfig};
use crate::game::fixed_math::{FixedNum, FixedVec2};
use crate::game::map::MapSize;
use crate::game::spatial_hash::SpatialHash;

use crate::game::simulation::resources::*;

/// Marker resource indicating spatial hash was rebuilt (clear all OccupiedCell components)
#[derive(Resource)]
pub struct SpatialHashRebuilt;

/// Initialize SimConfig from InitialConfig at startup
pub fn init_sim_config_from_initial(
    mut fixed_time: ResMut<Time<Fixed>>,
    mut sim_config: ResMut<SimConfig>,
    mut spatial_hash: ResMut<SpatialHash>,
    initial_config: Option<Res<InitialConfig>>,
    mut commands: Commands,
) {
    info!("Initializing SimConfig from InitialConfig (lightweight startup init)");
    
    let config = match &initial_config {
        Some(cfg) => cfg.as_ref(),
        None => {
            warn!("InitialConfig not found, using defaults ");
            &InitialConfig::default()
        }
    };
    
    // Set fixed timestep
    fixed_time.set_timestep_seconds(1.0 / config.tick_rate);
    
    // Copy all values from InitialConfig to SimConfig
    sim_config.tick_rate = config.tick_rate;
    sim_config.unit_speed = FixedNum::from_num(config.unit_speed);
    let half_width = FixedNum::from_num(config.map_width) / FixedNum::from_num(2.0);
    let half_height = FixedNum::from_num(config.map_height) / FixedNum::from_num(2.0);
    sim_config.map_size = MapSize {
        top_left: FixedVec2::new(-half_width, -half_height),
        bottom_right: FixedVec2::new(half_width, half_height),
    };
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
    sim_config.max_velocity = FixedNum::from_num(config.max_velocity);
    sim_config.braking_force = FixedNum::from_num(config.braking_force);
    sim_config.touch_dist_multiplier = FixedNum::from_num(config.touch_dist_multiplier);
    sim_config.check_dist_multiplier = FixedNum::from_num(config.check_dist_multiplier);
    sim_config.arrival_threshold = FixedNum::from_num(config.arrival_threshold);
    sim_config.max_force = FixedNum::from_num(config.max_force);
    sim_config.steering_force = FixedNum::from_num(config.steering_force);
    sim_config.max_acceleration = FixedNum::from_num(config.max_acceleration);
    sim_config.repulsion_force = FixedNum::from_num(config.repulsion_force);
    sim_config.repulsion_decay = FixedNum::from_num(config.repulsion_decay);
    sim_config.separation_weight = FixedNum::from_num(config.separation_weight);
    sim_config.alignment_weight = FixedNum::from_num(config.alignment_weight);
    sim_config.cohesion_weight = FixedNum::from_num(config.cohesion_weight);
    sim_config.neighbor_radius = FixedNum::from_num(config.neighbor_radius);
    sim_config.separation_radius = FixedNum::from_num(config.separation_radius);
    sim_config.boids_max_neighbors = config.boids_max_neighbors;
    sim_config.black_hole_strength = FixedNum::from_num(config.black_hole_strength);
    sim_config.wind_spot_strength = FixedNum::from_num(config.wind_spot_strength);
    sim_config.force_source_radius = FixedNum::from_num(config.force_source_radius);
    
    // Spatial hash parallel updates
    sim_config.spatial_hash_parallel_updates = config.spatial_hash_parallel_updates;
    sim_config.spatial_hash_regions_per_axis = config.spatial_hash_regions_per_axis;
    
    // Initialize spatial hash with proper configuration
    spatial_hash.resize(
        sim_config.map_size.get_width(),
        sim_config.map_size.get_height(),
        &config.spatial_hash_entity_radii,
        config.spatial_hash_radius_to_cell_ratio,
        config.spatial_hash_max_entity_count,
        config.spatial_hash_arena_overcapacity_ratio,
    );
    
    // CRITICAL: When spatial hash is resized, all OccupiedCell components become invalid
    // Remove them so entities get re-inserted fresh on next update
    commands.remove_resource::<SpatialHashRebuilt>();
    commands.insert_resource(SpatialHashRebuilt);
    
    info!("SimConfig initialized with map size: {}x{}", 
          sim_config.map_size.get_width().to_num::<f32>(), sim_config.map_size.get_height().to_num::<f32>());
    info!("SpatialHash initialized with {} size classes ", spatial_hash.size_classes().len());
}

/// Handle hot-reloadable runtime configuration
pub fn update_sim_from_runtime_config(
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    mut events: MessageReader<AssetEvent<GameConfig>>,
) {
    for event in events.read() {
        if event.is_modified(config_handle.0.id()) || event.is_loaded_with_dependencies(config_handle.0.id()) {
            if let Some(_config) = game_configs.get(&config_handle.0) {
                info!("Runtime config loaded/updated (controls, camera, debug settings)");
                // The config is stored in the asset and accessed when needed by other systems
                // No need to copy values here since systems read from GameConfig directly
            }
        }
    }
}
