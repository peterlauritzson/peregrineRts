use bevy::prelude::*;
use crate::game::GroundPlane;
use crate::game::structures::{FlowField, CELL_SIZE};
use crate::game::fixed_math::{FixedVec2, FixedNum};
use crate::game::simulation::{StaticObstacle, MapFlowField, SimConfig};
use crate::game::camera::RtsCamera;
use crate::game::pathfinding::{HierarchicalGraph, GraphBuildState, GraphBuildStep};
use crate::game::spatial_hash::SpatialHash;
use crate::game::config::{GameConfig, GameConfigHandle};
use rand::Rng;
use super::components::*;
use super::input::spawn_obstacle;

/// Handles the async map generation process
pub fn handle_generation(
    mut commands: Commands,
    mut editor_state: ResMut<EditorState>,
    obstacle_query: Query<Entity, With<StaticObstacle>>,
    unit_query: Query<Entity, With<crate::game::unit::Unit>>,
    editor_resources: Res<EditorResources>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    loading_query: Query<Entity, With<LoadingOverlayRoot>>,
    mut map_flow_field: ResMut<MapFlowField>,
    mut graph: ResMut<HierarchicalGraph>,
    mut build_state: ResMut<GraphBuildState>,
    mut camera_query: Query<&mut Transform, With<RtsCamera>>,
    mut sim_config: ResMut<SimConfig>,
    mut spatial_hash: ResMut<SpatialHash>,
    mut meshes: ResMut<Assets<Mesh>>,
    ground_plane_query: Query<(Entity, &Mesh3d), With<GroundPlane>>,
) {
    if !editor_state.is_generating {
        return;
    }

    // Wait for loading overlay to be spawned (next frame)
    if loading_query.is_empty() {
        return;
    }
    
    let Some(_config) = game_configs.get(&config_handle.0) else {
        error!("Game config not found during map generation!");
        editor_state.is_generating = false;
        for entity in loading_query.iter() {
            commands.entity(entity).despawn();
        }
        return;
    };

    let params = editor_state.generation_params;
    let gen_start = std::time::Instant::now();
    info!("=== MAP GENERATION START ===");
    info!("Generating map: {}x{} with {} obstacles...", params.map_width, params.map_height, params.num_obstacles);

    // Clear existing obstacles
    let clear_start = std::time::Instant::now();
    let obstacle_count = obstacle_query.iter().count();
    for entity in obstacle_query.iter() {
        commands.entity(entity).despawn();
    }
    info!("Cleared {} existing obstacles in {:?}", obstacle_count, clear_start.elapsed());
    
    // Clear existing units to prevent bounds issues when map size changes
    let unit_count = unit_query.iter().count();
    if unit_count > 0 {
        for entity in unit_query.iter() {
            commands.entity(entity).despawn();
        }
        info!("Cleared {} existing units (prevents bounds issues with new map size)", unit_count);
    }
    
    // Reset Graph and Build State
    let graph_reset_start = std::time::Instant::now();
    graph.reset();
    build_state.step = GraphBuildStep::Done;
    info!("Reset graph in {:?}", graph_reset_start.elapsed());
    
    // Use params from dialog
    let map_width = params.map_width;
    let map_height = params.map_height;
    
    editor_state.current_map_size = Vec2::new(map_width, map_height);
    
    // Update SimConfig with new map dimensions
    sim_config.map_width = FixedNum::from_num(map_width);
    sim_config.map_height = FixedNum::from_num(map_height);
    info!("Updated SimConfig: map size = {}x{}", map_width, map_height);
    
    // Update SpatialHash with new map dimensions
    spatial_hash.resize(
        FixedNum::from_num(map_width),
        FixedNum::from_num(map_height),
        FixedNum::from_num(2.0)
    );
    info!("Updated SpatialHash for new map size");

    // Adjust camera to view the entire map
    if let Ok(mut camera_transform) = camera_query.single_mut() {
        let max_dimension = map_width.max(map_height);
        let camera_height = max_dimension * 0.8;  // 0.8x the map size for good viewing angle
        let camera_distance = max_dimension * 0.6; // Distance back from center
        camera_transform.translation = Vec3::new(0.0, camera_height, camera_distance);
        camera_transform.look_at(Vec3::ZERO, Vec3::Y);
        info!("Adjusted camera to view {}x{} map (height: {}, distance: {})", 
              map_width, map_height, camera_height, camera_distance);
    }

    let ff_width = (map_width / CELL_SIZE) as usize;
    let ff_height = (map_height / CELL_SIZE) as usize;
    info!("Creating FlowField: {} x {} cells ({}x{} world units)", 
          ff_width, ff_height, map_width, map_height);

    let ff_start = std::time::Instant::now();
    map_flow_field.0 = FlowField::new(
        ff_width, 
        ff_height, 
        FixedNum::from_num(CELL_SIZE), 
        FixedVec2::new(FixedNum::from_num(-map_width/2.0), FixedNum::from_num(-map_height/2.0))
    );
    info!("FlowField created successfully in {:?} (total cells: {})", ff_start.elapsed(), ff_width * ff_height);
    
    // Update ground plane mesh to match new map size
    for (entity, _mesh3d) in ground_plane_query.iter() {
        let new_mesh = meshes.add(Plane3d::default().mesh().size(map_width, map_height));
        commands.entity(entity).insert(Mesh3d(new_mesh));
        info!("Updated ground plane mesh to {}x{}", map_width, map_height);
    }

    // CRITICAL: Build hierarchical graph BEFORE spawning obstacles
    // This ensures clusters and portals are valid for the new map size
    // before apply_new_obstacles tries to regenerate cluster flow fields
    info!("Building hierarchical graph for new map...");
    let graph_start = std::time::Instant::now();
    graph.build_graph_sync(&map_flow_field.0);
    graph.initialized = true;
    info!("Graph built in {:?} - {} clusters, {} portals", 
        graph_start.elapsed(), 
        graph.clusters.len(), 
        graph.nodes.len()
    );

    // Spawn obstacles (if any)
    let num_obstacles = params.num_obstacles;
    if num_obstacles > 0 {
        info!("Starting to spawn {} obstacles...", num_obstacles);
        let spawn_start = std::time::Instant::now();
        let mut rng = rand::rng();
        
        for i in 0..num_obstacles {
            if i % 100 == 0 && i > 0 {
                info!("  Spawned {}/{} obstacles ({:.1}%) - elapsed: {:?}", 
                      i, num_obstacles, (i as f32 / num_obstacles as f32) * 100.0, spawn_start.elapsed());
            }
            let x = rng.random_range(-map_width/2.0..map_width/2.0);
            let y = rng.random_range(-map_height/2.0..map_height/2.0);
            let radius = rng.random_range(params.min_radius..params.max_radius);
            
            spawn_obstacle(&mut commands, FixedVec2::new(FixedNum::from_num(x), FixedNum::from_num(y)), FixedNum::from_num(radius), &editor_resources);
        }
        info!("Finished spawning all {} obstacles in {:?}", num_obstacles, spawn_start.elapsed());
    } else {
        info!("No obstacles to spawn.");
    }

    editor_state.is_generating = false;
    info!("=== MAP GENERATION COMPLETE in {:?} ===", gen_start.elapsed());
    info!("Map generation complete - NOT removing overlay yet, letting Bevy process entities first.");
    
    // DON'T remove loading overlay immediately - let it be removed on the next frame
    // This gives Bevy time to process the spawned entities before we return control
    // The overlay will be removed by a separate system or manual cleanup
}

/// Checks if map finalization is complete
pub fn check_finalization_complete(
    mut commands: Commands,
    mut editor_state: ResMut<EditorState>,
    loading_query: Query<Entity, With<LoadingOverlayRoot>>,
    graph: Res<HierarchicalGraph>,
) {
    if !editor_state.is_finalizing {
        return;
    }

    if graph.initialized {
        editor_state.is_finalizing = false;
        info!("Map finalization COMPLETE! Graph has {} portals and {} clusters", 
              graph.nodes.len(), graph.clusters.len());
        info!("You can now save the map.");
        // Remove loading overlay
        for entity in loading_query.iter() {
            commands.entity(entity).despawn();
        }
    }
}
