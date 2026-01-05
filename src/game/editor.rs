use bevy::prelude::*;
use crate::game::{GameState, GroundPlane};
use crate::game::config::{GameConfig, GameConfigHandle, InitialConfig};
use crate::game::map::{MapData, MapObstacle, save_map, MAP_VERSION};
use crate::game::math::{FixedVec2, FixedNum};
use crate::game::simulation::{StaticObstacle, SimPosition, Collider, layers, MapFlowField, SimConfig};
use crate::game::camera::RtsCamera;
use crate::game::flow_field::{FlowField, CELL_SIZE};
use crate::game::pathfinding::{CLUSTER_SIZE, HierarchicalGraph, GraphBuildState, GraphBuildStep};
use crate::game::spatial_hash::SpatialHash;
use rand::Rng;

#[derive(Resource)]
pub struct PendingMapGeneration {
    pub map_width: f32,
    pub map_height: f32,
    pub num_obstacles: usize,
    pub min_radius: f32,
    pub max_radius: f32,
}

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorState>()
           .init_resource::<ActiveInputField>()
           .add_systems(Startup, setup_editor_resources)
           .add_systems(OnEnter(GameState::Editor), setup_editor_ui)
           .add_systems(OnExit(GameState::Editor), cleanup_editor_ui)
           .add_systems(Update, (editor_button_system, handle_editor_input, handle_generation, cleanup_generation_overlay, check_finalization_complete, keyboard_input_system, handle_input_field_clicks).run_if(in_state(GameState::Editor)));
    }
}

#[derive(Resource)]
pub struct EditorResources {
    pub obstacle_mesh: Handle<Mesh>,
    pub obstacle_material: Handle<StandardMaterial>,
}

fn setup_editor_resources(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(EditorResources {
        obstacle_mesh: meshes.add(Circle::new(1.0).mesh()), // 2D circle converted to mesh, scale it later
        obstacle_material: materials.add(Color::srgb(0.5, 0.5, 0.5)),
    });
}

#[derive(Component)]
struct EditorUiRoot;

#[derive(Component)]
enum EditorButtonAction {
    OpenGenerateDialog,
    SaveMap,
    TogglePlaceObstacle,
    ClearMap,
    FinalizeMap,
    
    // Dialog buttons
    DialogGenerate,
    DialogCancel,
    
    // Input adjustment buttons
    IncrementMapWidth,
    DecrementMapWidth,
    IncrementMapHeight,
    DecrementMapHeight,
    IncrementObstacles,
    DecrementObstacles,
    IncrementObstacleSize,
    DecrementObstacleSize,
}

#[derive(Resource, Default)]
struct EditorState {
    placing_obstacle: bool,
    show_generation_dialog: bool,
    is_generating: bool,
    is_finalizing: bool,
    generation_params: GenerationParams,
    current_map_size: Vec2,
    // Input field values (defaults)
    input_map_width: String,
    input_map_height: String,
    input_num_obstacles: String,
    input_obstacle_size: String, // Combined min/max for simplicity
}

#[derive(Default, Clone, Copy)]
struct GenerationParams {
    map_width: f32,
    map_height: f32,
    num_obstacles: usize,
    min_radius: f32,
    max_radius: f32,
}

#[derive(Component)]
struct GenerationDialogRoot;

#[derive(Component)]
struct LoadingOverlayRoot;

#[derive(Component, Clone, Copy, PartialEq)]
enum InputFieldType {
    MapWidth,
    MapHeight,
    NumObstacles,
    ObstacleSize,
}

#[derive(Resource, Default)]
struct ActiveInputField {
    field: Option<InputFieldType>,
    first_input: bool,  // True when field was just selected, false after first keypress
}

fn setup_editor_ui(
    mut commands: Commands,
    mut editor_state: ResMut<EditorState>,
    map_flow_field: Res<MapFlowField>,
    initial_config: Res<InitialConfig>,
) {
    // Initialize map size from flow field or initial config
    let flow_field = &map_flow_field.0;
    if flow_field.width > 0 {
        editor_state.current_map_size = Vec2::new(
            flow_field.width as f32 * flow_field.cell_size.to_num::<f32>(),
            flow_field.height as f32 * flow_field.cell_size.to_num::<f32>()
        );
    } else {
        editor_state.current_map_size = Vec2::new(initial_config.map_width, initial_config.map_height);
    }

    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::FlexStart,
                align_items: AlignItems::FlexStart,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                ..default()
            },
            EditorUiRoot,
        ))
        .with_children(|parent| {
            // Helper macro for buttons
            macro_rules! spawn_button {
                ($text:expr, $action:expr) => {
                    parent.spawn((
                        Button,
                        Node {
                            width: Val::Px(200.0),
                            height: Val::Px(40.0),
                            margin: UiRect::all(Val::Px(5.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.3, 0.3, 0.3)),
                        $action,
                    ))
                    .with_children(|parent| {
                        parent.spawn((
                            Text::new($text),
                            TextFont {
                                font_size: 18.0,
                                ..default()
                            },
                            TextColor(Color::WHITE),
                        ));
                    });
                };
            }

            spawn_button!("Generate Random Map", EditorButtonAction::OpenGenerateDialog);
            spawn_button!("Clear Map", EditorButtonAction::ClearMap);
            spawn_button!("Toggle Place Obstacle", EditorButtonAction::TogglePlaceObstacle);
            spawn_button!("Finalize / Bake Map", EditorButtonAction::FinalizeMap);
            spawn_button!("Save Map", EditorButtonAction::SaveMap);
            
            // Instructions
            parent.spawn((
                Text::new("Press 'Toggle Place Obstacle' then click on map to place obstacles."),
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(Color::WHITE),
                Node {
                    margin: UiRect::top(Val::Px(10.0)),
                    ..default()
                },
            ));
        });
}

fn cleanup_editor_ui(mut commands: Commands, query: Query<Entity, With<EditorUiRoot>>, dialog_query: Query<Entity, With<GenerationDialogRoot>>, loading_query: Query<Entity, With<LoadingOverlayRoot>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
    for entity in dialog_query.iter() {
        commands.entity(entity).despawn();
    }
    for entity in loading_query.iter() {
        commands.entity(entity).despawn();
    }
}

fn spawn_generation_dialog(commands: &mut Commands, editor_state: &EditorState, active_field: &ActiveInputField) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(20.0),
            top: Val::Percent(15.0),
            width: Val::Percent(60.0),
            height: Val::Auto,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(2.0)),
            padding: UiRect::all(Val::Px(20.0)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.2, 0.2, 0.2)),
        BorderColor::from(Color::WHITE),
        GenerationDialogRoot,
    )).with_children(|parent| {
        parent.spawn((
            Text::new("Generate Random Map"),
            TextFont { font_size: 24.0, ..default() },
            TextColor(Color::WHITE),
            Node { margin: UiRect::bottom(Val::Px(20.0)), ..default() },
        ));

        // Helper macro to create adjustable value rows
        macro_rules! create_value_row {
            ($label:expr, $value:expr, $dec:expr, $inc:expr, $field_type:expr) => {
                let is_active = active_field.field == Some($field_type);
                let border_color = if is_active { Color::srgb(0.3, 0.7, 1.0) } else { Color::srgb(0.5, 0.5, 0.5) };
                let display_value = if is_active { format!("{}_", $value) } else { $value.to_string() };
                parent.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        margin: UiRect::bottom(Val::Px(15.0)),
                        width: Val::Percent(100.0),
                        ..default()
                    },
                )).with_children(|row| {
                    // Label
                    row.spawn((
                        Text::new($label),
                        TextFont { font_size: 18.0, ..default() },
                        TextColor(Color::WHITE),
                        Node { width: Val::Px(180.0), ..default() },
                    ));
                    
                    // Controls container
                    row.spawn((
                        Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                    )).with_children(|controls| {
                        // Decrement button
                        controls.spawn((
                            Button,
                            Node {
                                width: Val::Px(40.0),
                                height: Val::Px(35.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                margin: UiRect::right(Val::Px(10.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.5, 0.3, 0.3)),
                            $dec,
                        )).with_children(|btn| {
                            btn.spawn((
                                Text::new("-"),
                                TextFont { font_size: 24.0, ..default() },
                                TextColor(Color::WHITE),
                            ));
                        });
                        
                        // Value display (clickable to type directly)
                        controls.spawn((
                            Button,
                            Node {
                                width: Val::Px(100.0),
                                height: Val::Px(35.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(Val::Px(2.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.1, 0.1, 0.1)),
                            BorderColor::from(border_color),
                            $field_type,
                        )).with_children(|val| {
                            val.spawn((
                                Text::new(display_value),
                                TextFont { font_size: 18.0, ..default() },
                                TextColor(Color::WHITE),
                            ));
                        });
                        
                        // Increment button
                        controls.spawn((
                            Button,
                            Node {
                                width: Val::Px(40.0),
                                height: Val::Px(35.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                margin: UiRect::left(Val::Px(10.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.3, 0.5, 0.3)),
                            $inc,
                        )).with_children(|btn| {
                            btn.spawn((
                                Text::new("+"),
                                TextFont { font_size: 24.0, ..default() },
                                TextColor(Color::WHITE),
                            ));
                        });
                    });
                });
            };
        }

        // Create all the adjustable rows
        create_value_row!("Map Width:", &editor_state.input_map_width, EditorButtonAction::DecrementMapWidth, EditorButtonAction::IncrementMapWidth, InputFieldType::MapWidth);
        create_value_row!("Map Height:", &editor_state.input_map_height, EditorButtonAction::DecrementMapHeight, EditorButtonAction::IncrementMapHeight, InputFieldType::MapHeight);
        create_value_row!("Num Obstacles:", &editor_state.input_num_obstacles, EditorButtonAction::DecrementObstacles, EditorButtonAction::IncrementObstacles, InputFieldType::NumObstacles);
        create_value_row!("Obstacle Radius:", &editor_state.input_obstacle_size, EditorButtonAction::DecrementObstacleSize, EditorButtonAction::IncrementObstacleSize, InputFieldType::ObstacleSize);

        // Info text
        parent.spawn((
            Text::new("Tip: Start small (50x50, 0 obstacles) and increase gradually"),
            TextFont { font_size: 14.0, ..default() },
            TextColor(Color::srgb(0.7, 0.7, 0.7)),
            Node { margin: UiRect::vertical(Val::Px(15.0)), ..default() },
        ));

        // Action buttons
        parent.spawn((
            Node {
                flex_direction: FlexDirection::Row,
                ..default()
            },
        )).with_children(|buttons| {
             // Helper macro for buttons
             macro_rules! spawn_dialog_button {
                ($text:expr, $action:expr, $color:expr) => {
                    buttons.spawn((
                        Button,
                        Node {
                            width: Val::Px(120.0),
                            height: Val::Px(40.0),
                            margin: UiRect::all(Val::Px(10.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor($color),
                        $action,
                    ))
                    .with_children(|btn_parent| {
                        btn_parent.spawn((
                            Text::new($text),
                            TextFont {
                                font_size: 18.0,
                                ..default()
                            },
                            TextColor(Color::WHITE),
                        ));
                    });
                };
            }
            spawn_dialog_button!("Generate", EditorButtonAction::DialogGenerate, Color::srgb(0.3, 0.6, 0.3));
            spawn_dialog_button!("Cancel", EditorButtonAction::DialogCancel, Color::srgb(0.4, 0.4, 0.4));
        });
    });
}

fn spawn_loading_overlay(commands: &mut Commands, text: &str) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
        LoadingOverlayRoot,
    )).with_children(|parent| {
        parent.spawn((
            Text::new(text),
            TextFont { font_size: 40.0, ..default() },
            TextColor(Color::WHITE),
        ));
    });
}

fn editor_button_system(
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
    mut build_state: ResMut<GraphBuildState>,
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
                        build_state.step = GraphBuildStep::Done;

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
                        build_state.step = GraphBuildStep::NotStarted;
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
                        
                        info!("Saving map with {} portals and {} clusters", graph.nodes.len(), graph.clusters.len());
                        
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

fn handle_generation(
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

fn cleanup_generation_overlay(
    mut commands: Commands,
    editor_state: Res<EditorState>,
    loading_query: Query<Entity, With<LoadingOverlayRoot>>,
) {
    // Only run if generation just finished (not currently generating and overlay exists)
    if !editor_state.is_generating && !editor_state.is_finalizing && !loading_query.is_empty() {
        info!("cleanup_generation_overlay: Removing loading overlay after entity processing");
        for entity in loading_query.iter() {
            commands.entity(entity).despawn();
        }
    }
}

fn check_finalization_complete(
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

fn handle_editor_input(
    mut commands: Commands,
    mouse_button_input: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
    editor_state: Res<EditorState>,
    editor_resources: Res<EditorResources>,
    initial_config: Res<InitialConfig>,
) {
    if !editor_state.placing_obstacle {
        return;
    }

    if mouse_button_input.just_pressed(MouseButton::Left) {
        let Ok((camera, camera_transform)) = camera_q.single() else { return };
        let Ok(window) = windows.single() else { return };

        if let Some(cursor_position) = window.cursor_position() {
            if let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) {
                // Intersect with plane Y=0
                if ray.direction.y.abs() > 0.0001 {
                    let t = -ray.origin.y / ray.direction.y;
                    if t >= 0.0 {
                        let intersection = ray.origin + ray.direction * t;
                        spawn_obstacle(&mut commands, FixedVec2::new(FixedNum::from_num(intersection.x), FixedNum::from_num(intersection.z)), FixedNum::from_num(initial_config.editor_default_obstacle_radius), &editor_resources);
                    }
                }
            }
        }
    }
}

fn spawn_obstacle(commands: &mut Commands, position: FixedVec2, radius: FixedNum, resources: &EditorResources) {
    commands.spawn((
        StaticObstacle,
        SimPosition(position),
        Collider {
            radius,
            layer: layers::OBSTACLE,
            mask: layers::ALL,
        },
        Transform::from_translation(Vec3::new(position.x.to_num(), 0.0, position.y.to_num()))
            .with_scale(Vec3::new(radius.to_num::<f32>(), radius.to_num::<f32>(), radius.to_num::<f32>()))
            .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        GlobalTransform::default(),
        Mesh3d(resources.obstacle_mesh.clone()),
        MeshMaterial3d(resources.obstacle_material.clone()),
    ));
}

// Systems for keyboard input handling
fn keyboard_input_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut editor_state: ResMut<EditorState>,
    mut active_field: ResMut<ActiveInputField>,
    mut commands: Commands,
    dialog_root_query: Query<Entity, With<GenerationDialogRoot>>,
) {
    let Some(field_type) = active_field.field else { return };
    
    // Helper to get mutable reference to the appropriate string field
    let input_str = match field_type {
        InputFieldType::MapWidth => &mut editor_state.input_map_width,
        InputFieldType::MapHeight => &mut editor_state.input_map_height,
        InputFieldType::NumObstacles => &mut editor_state.input_num_obstacles,
        InputFieldType::ObstacleSize => &mut editor_state.input_obstacle_size,
    };
    
    let mut changed = false;
    
    // Handle backspace
    if keys.just_pressed(KeyCode::Backspace) {
        if !input_str.is_empty() {
            input_str.pop();
            changed = true;
            active_field.first_input = false;
        }
    }
    
    // Handle number keys
    for key in [
        KeyCode::Digit0, KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, 
        KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7,
        KeyCode::Digit8, KeyCode::Digit9,
    ] {
        if keys.just_pressed(key) {
            let digit = match key {
                KeyCode::Digit0 => '0',
                KeyCode::Digit1 => '1',
                KeyCode::Digit2 => '2',
                KeyCode::Digit3 => '3',
                KeyCode::Digit4 => '4',
                KeyCode::Digit5 => '5',
                KeyCode::Digit6 => '6',
                KeyCode::Digit7 => '7',
                KeyCode::Digit8 => '8',
                KeyCode::Digit9 => '9',
                _ => continue,
            };
            
            // Clear on first input, then append
            if active_field.first_input {
                input_str.clear();
                active_field.first_input = false;
            }
            
            if input_str.len() < 5 {  // Max 5 digits
                input_str.push(digit);
                changed = true;
            }
        }
    }
    
    // Handle decimal point for obstacle size
    if field_type == InputFieldType::ObstacleSize && keys.just_pressed(KeyCode::Period) {
        if active_field.first_input {
            input_str.clear();
            active_field.first_input = false;
        }
        if !input_str.contains('.') && !input_str.is_empty() {
            input_str.push('.');
            changed = true;
        }
    }
    
    // Handle Enter or Escape to deselect
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Escape) {
        active_field.field = None;
        changed = true;
    }
    
    // If changed, respawn dialog
    if changed {
        for entity in dialog_root_query.iter() {
            commands.entity(entity).despawn();
        }
        if active_field.field.is_some() {
            spawn_generation_dialog(&mut commands, &editor_state, &active_field);
        }
    }
}

fn handle_input_field_clicks(
    mut interaction_query: Query<(&Interaction, &InputFieldType), Changed<Interaction>>,
    mut active_field: ResMut<ActiveInputField>,
) {
    for (interaction, field_type) in &mut interaction_query {
        if *interaction == Interaction::Pressed {
            // Toggle: if clicking the same field, deselect it; otherwise select the new field
            if active_field.field == Some(*field_type) {
                active_field.field = None;
                active_field.first_input = false;
            } else {
                active_field.field = Some(*field_type);
                active_field.first_input = true;  // First keypress will clear the field
            }
        }
    }
}
