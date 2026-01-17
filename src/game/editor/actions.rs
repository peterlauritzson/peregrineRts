use bevy::prelude::*;
use crate::game::structures::{FlowField, CELL_SIZE};
use crate::game::fixed_math::{FixedVec2, FixedNum};
use crate::game::simulation::{StaticObstacle, SimPosition, Collider, MapFlowField};
use crate::game::pathfinding::{CLUSTER_SIZE, HierarchicalGraph};
use crate::game::config::{GameConfig, GameConfigHandle};
use crate::game::map::{MapData, MapObstacle, save_map, MAP_VERSION};
use super::components::*;
use super::ui::spawn_generation_dialog;
use super::ui::spawn_loading_overlay;

/// System that handles all editor button interactions
pub fn editor_button_system(
    mut interaction_query: Query<
        (&Interaction, &mut BackgroundColor, &EditorButtonAction),
        (Changed<Interaction>, With<Button>),
    >,
    mut commands: Commands,
    mut editor_state: ResMut<EditorState>,
    obstacle_query: Query<Entity, With<StaticObstacle>>,
    all_obstacles_query: Query<(&SimPosition, &Collider), With<StaticObstacle>>,
    _editor_resources: Res<EditorResources>,
    mut map_flow_field: ResMut<MapFlowField>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    dialog_query: Query<Entity, With<GenerationDialogRoot>>,
    mut graph: ResMut<HierarchicalGraph>,
    mut active_field: ResMut<ActiveInputField>,
) {
    let Some(_config) = game_configs.get(&config_handle.0) else { return };

    for (interaction, mut color, action) in &mut interaction_query {
        match *interaction {
            Interaction::Pressed => {
                *color = BackgroundColor(Color::srgb(0.1, 0.1, 0.1));
                match action {
                    EditorButtonAction::OpenGenerateDialog => {
                        if !editor_state.show_generation_dialog {
                            // Initialize default values if empty
                            if editor_state.input_map_width.is_empty() {
                                editor_state.input_map_width = "50".to_string();
                            }
                            if editor_state.input_map_height.is_empty() {
                                editor_state.input_map_height = "50".to_string();
                            }
                            if editor_state.input_num_obstacles.is_empty() {
                                editor_state.input_num_obstacles = "0".to_string();
                            }
                            if editor_state.input_obstacle_size.is_empty() {
                                editor_state.input_obstacle_size = "2.0".to_string();
                            }
                            editor_state.show_generation_dialog = true;
                            spawn_generation_dialog(&mut commands, &editor_state, &active_field);
                        }
                    }
                    EditorButtonAction::DialogCancel => {
                        editor_state.show_generation_dialog = false;
                        active_field.field = None;
                        active_field.first_input = false;
                        for entity in dialog_query.iter() {
                            commands.entity(entity).despawn();
                        }
                    }
                    
                    // Increment/Decrement handlers
                    EditorButtonAction::IncrementMapWidth => {
                        let val = editor_state.input_map_width.parse::<i32>().unwrap_or(50);
                        editor_state.input_map_width = (val + 10).to_string();
                        // Respawn dialog to update display
                        for entity in dialog_query.iter() {
                            commands.entity(entity).despawn();
                        }
                        spawn_generation_dialog(&mut commands, &editor_state, &active_field);
                    }
                    EditorButtonAction::DecrementMapWidth => {
                        let val = editor_state.input_map_width.parse::<i32>().unwrap_or(50);
                        editor_state.input_map_width = (val - 10).max(10).to_string();
                        for entity in dialog_query.iter() {
                            commands.entity(entity).despawn();
                        }
                        spawn_generation_dialog(&mut commands, &editor_state, &active_field);
                    }
                    EditorButtonAction::IncrementMapHeight => {
                        let val = editor_state.input_map_height.parse::<i32>().unwrap_or(50);
                        editor_state.input_map_height = (val + 10).to_string();
                        for entity in dialog_query.iter() {
                            commands.entity(entity).despawn();
                        }
                        spawn_generation_dialog(&mut commands, &editor_state, &active_field);
                    }
                    EditorButtonAction::DecrementMapHeight => {
                        let val = editor_state.input_map_height.parse::<i32>().unwrap_or(50);
                        editor_state.input_map_height = (val - 10).max(10).to_string();
                        for entity in dialog_query.iter() {
                            commands.entity(entity).despawn();
                        }
                        spawn_generation_dialog(&mut commands, &editor_state, &active_field);
                    }
                    EditorButtonAction::IncrementObstacles => {
                        let val = editor_state.input_num_obstacles.parse::<i32>().unwrap_or(0);
                        editor_state.input_num_obstacles = (val + 5).to_string();
                        for entity in dialog_query.iter() {
                            commands.entity(entity).despawn();
                        }
                        spawn_generation_dialog(&mut commands, &editor_state, &active_field);
                    }
                    EditorButtonAction::DecrementObstacles => {
                        let val = editor_state.input_num_obstacles.parse::<i32>().unwrap_or(0);
                        editor_state.input_num_obstacles = (val - 5).max(0).to_string();
                        for entity in dialog_query.iter() {
                            commands.entity(entity).despawn();
                        }
                        spawn_generation_dialog(&mut commands, &editor_state, &active_field);
                    }
                    EditorButtonAction::IncrementObstacleSize => {
                        let val = editor_state.input_obstacle_size.parse::<f32>().unwrap_or(2.0);
                        editor_state.input_obstacle_size = format!("{:.1}", (val + 0.5).min(20.0));
                        for entity in dialog_query.iter() {
                            commands.entity(entity).despawn();
                        }
                        spawn_generation_dialog(&mut commands, &editor_state, &active_field);
                    }
                    EditorButtonAction::DecrementObstacleSize => {
                        let val = editor_state.input_obstacle_size.parse::<f32>().unwrap_or(2.0);
                        editor_state.input_obstacle_size = format!("{:.1}", (val - 0.5).max(0.5));
                        for entity in dialog_query.iter() {
                            commands.entity(entity).despawn();
                        }
                        spawn_generation_dialog(&mut commands, &editor_state, &active_field);
                    }
                    
                    EditorButtonAction::DialogGenerate => {
                        editor_state.show_generation_dialog = false;
                        for entity in dialog_query.iter() {
                            commands.entity(entity).despawn();
                        }
                        
                        // Parse input values
                        let map_width = editor_state.input_map_width.parse::<f32>().unwrap_or(50.0);
                        let map_height = editor_state.input_map_height.parse::<f32>().unwrap_or(50.0);
                        let num_obstacles = editor_state.input_num_obstacles.parse::<usize>().unwrap_or(0);
                        let obstacle_radius = editor_state.input_obstacle_size.parse::<f32>().unwrap_or(2.0);
                        
                        info!("Dialog: Requesting map generation - {}x{}, {} obstacles of radius {}", map_width, map_height, num_obstacles, obstacle_radius);
                        
                        // Start generation process
                        editor_state.is_generating = true;
                        editor_state.generation_params = GenerationParams {
                            map_width,
                            map_height,
                            num_obstacles,
                            min_radius: obstacle_radius * 0.8,  // Slight variation
                            max_radius: obstacle_radius * 1.2,
                        };
                        spawn_loading_overlay(&mut commands, "Generating Map...");
                    }
                    EditorButtonAction::ClearMap => {
                        for entity in obstacle_query.iter() {
                            commands.entity(entity).despawn();
                        }
                        
                        graph.reset();


                        // Reset FlowField
                        let map_width = editor_state.current_map_size.x;
                        let map_height = editor_state.current_map_size.y;
                        map_flow_field.0 = FlowField::new(
                            (map_width / CELL_SIZE) as usize, 
                            (map_height / CELL_SIZE) as usize, 
                            FixedNum::from_num(CELL_SIZE), 
                            FixedVec2::new(FixedNum::from_num(-map_width/2.0), FixedNum::from_num(-map_height/2.0))
                        );
                    }
                    EditorButtonAction::TogglePlaceObstacle => {
                        editor_state.placing_obstacle = !editor_state.placing_obstacle;
                        info!("Placing obstacle: {}", editor_state.placing_obstacle);
                    }
                    EditorButtonAction::FinalizeMap => {
                        editor_state.is_finalizing = true;
                        spawn_loading_overlay(&mut commands, "Finalizing Map...");
                        
                        // Synchronous Flow Field Update
                        use crate::game::simulation::apply_obstacle_to_flow_field;
                        let flow_field = &mut map_flow_field.0;
                        flow_field.cost_field.fill(1);
                        for (pos, collider) in all_obstacles_query.iter() {
                            apply_obstacle_to_flow_field(flow_field, pos.0, collider.radius);
                        }
                        
                        // Reset Graph Build State to trigger incremental build
                        graph.reset();

                    }
                    EditorButtonAction::SaveMap => {
                        // Check if graph is initialized before saving
                        if !graph.initialized {
                            warn!("Cannot save map - graph not finalized yet! Click 'Finalize / Bake Map' and wait for completion.");
                            return;
                        }
                        
                        let mut obstacles = Vec::new();
                        for (pos, collider) in all_obstacles_query.iter() {
                            obstacles.push(MapObstacle {
                                position: pos.0,
                                radius: collider.radius,
                            });
                        }
                        
                        let stats = graph.get_stats();
                        info!("Saving map with {} regions in {} clusters", stats.region_count, stats.cluster_count);
                        
                        let map_data = MapData {
                            version: MAP_VERSION,
                            map_width: FixedNum::from_num(editor_state.current_map_size.x),
                            map_height: FixedNum::from_num(editor_state.current_map_size.y),
                            cell_size: FixedNum::from_num(CELL_SIZE),
                            cluster_size: CLUSTER_SIZE,
                            obstacles,
                            start_locations: vec![], // TODO: Add start locations
                            cost_field: map_flow_field.0.cost_field.clone(),
                            graph: graph.clone(), // Save the built graph!
                        };
                        
                        if let Err(e) = save_map("assets/maps/default.pmap", &map_data) {
                            error!("Failed to save map: {}", e);
                        } else {
                            info!("Map saved to assets/maps/default.pmap");
                        }
                    }
                }
            }
            Interaction::Hovered => {
                *color = BackgroundColor(Color::srgb(0.25, 0.25, 0.25));
            }
            Interaction::None => {
                *color = BackgroundColor(Color::srgb(0.3, 0.3, 0.3));
            }
        }
    }
}
