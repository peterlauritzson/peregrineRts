use bevy::prelude::*;
use crate::game::GameState;
use crate::game::config::{GameConfig, GameConfigHandle};
use crate::game::map::{MapData, MapObstacle, save_map, MAP_VERSION};
use crate::game::math::{FixedVec2, FixedNum};
use crate::game::simulation::{StaticObstacle, SimPosition, Collider, layers, MapFlowField};
use crate::game::camera::RtsCamera;
use crate::game::flow_field::{FlowField, CELL_SIZE};
use crate::game::pathfinding::{CLUSTER_SIZE, HierarchicalGraph, GraphBuildState, GraphBuildStep};
use rand::Rng;

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorState>()
           .add_systems(Startup, setup_editor_resources)
           .add_systems(OnEnter(GameState::Editor), setup_editor_ui)
           .add_systems(OnExit(GameState::Editor), cleanup_editor_ui)
           .add_systems(Update, (editor_button_system, handle_editor_input, handle_generation, check_finalization_complete).run_if(in_state(GameState::Editor)));
    }
}

#[derive(Resource)]
struct EditorResources {
    obstacle_mesh: Handle<Mesh>,
    obstacle_material: Handle<StandardMaterial>,
}

fn setup_editor_resources(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(EditorResources {
        obstacle_mesh: meshes.add(Circle::new(1.0)), // Unit circle, scale it later
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
}

#[derive(Resource, Default)]
struct EditorState {
    placing_obstacle: bool,
    show_generation_dialog: bool,
    is_generating: bool,
    is_finalizing: bool,
    generation_params: GenerationParams,
    current_map_size: Vec2,
}

#[derive(Default, Clone, Copy)]
struct GenerationParams {
    num_obstacles: usize,
    min_radius: f32,
    max_radius: f32,
}

#[derive(Component)]
struct GenerationDialogRoot;

#[derive(Component)]
struct LoadingOverlayRoot;

fn setup_editor_ui(
    mut commands: Commands,
    mut editor_state: ResMut<EditorState>,
    map_flow_field: Res<MapFlowField>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>
) {
    // Initialize map size
    let flow_field = &map_flow_field.0;
    if flow_field.width > 0 {
        editor_state.current_map_size = Vec2::new(
            flow_field.width as f32 * flow_field.cell_size.to_num::<f32>(),
            flow_field.height as f32 * flow_field.cell_size.to_num::<f32>()
        );
    } else if let Some(config) = game_configs.get(&config_handle.0) {
         editor_state.current_map_size = Vec2::new(config.map_width, config.map_height);
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

fn spawn_generation_dialog(commands: &mut Commands, config: &GameConfig) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(30.0),
            top: Val::Percent(30.0),
            width: Val::Percent(40.0),
            height: Val::Percent(40.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(2.0)),
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

        // Info text (could be inputs in future)
        parent.spawn((
            Text::new(format!("Obstacles: {}\nRadius: {} - {}", config.editor_num_obstacles, config.editor_obstacle_min_radius, config.editor_obstacle_max_radius)),
            TextFont { font_size: 18.0, ..default() },
            TextColor(Color::WHITE),
            Node { margin: UiRect::bottom(Val::Px(20.0)), ..default() },
        ));

        parent.spawn((
            Node {
                flex_direction: FlexDirection::Row,
                ..default()
            },
        )).with_children(|buttons| {
             // Helper macro for buttons
             macro_rules! spawn_dialog_button {
                ($text:expr, $action:expr) => {
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
                        BackgroundColor(Color::srgb(0.4, 0.4, 0.4)),
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
            spawn_dialog_button!("Generate", EditorButtonAction::DialogGenerate);
            spawn_dialog_button!("Cancel", EditorButtonAction::DialogCancel);
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
) {
    let Some(config) = game_configs.get(&config_handle.0) else { return };

    for (interaction, mut color, action) in &mut interaction_query {
        match *interaction {
            Interaction::Pressed => {
                *color = BackgroundColor(Color::srgb(0.1, 0.1, 0.1));
                match action {
                    EditorButtonAction::OpenGenerateDialog => {
                        if !editor_state.show_generation_dialog {
                            editor_state.show_generation_dialog = true;
                            spawn_generation_dialog(&mut commands, config);
                        }
                    }
                    EditorButtonAction::DialogCancel => {
                        editor_state.show_generation_dialog = false;
                        for entity in dialog_query.iter() {
                            commands.entity(entity).despawn();
                        }
                    }
                    EditorButtonAction::DialogGenerate => {
                        editor_state.show_generation_dialog = false;
                        for entity in dialog_query.iter() {
                            commands.entity(entity).despawn();
                        }
                        
                        // Start generation process
                        editor_state.is_generating = true;
                        editor_state.generation_params = GenerationParams {
                            num_obstacles: config.editor_num_obstacles,
                            min_radius: config.editor_obstacle_min_radius,
                            max_radius: config.editor_obstacle_max_radius,
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
                        let mut obstacles = Vec::new();
                        for (pos, collider) in all_obstacles_query.iter() {
                            obstacles.push(MapObstacle {
                                position: pos.0,
                                radius: collider.radius,
                            });
                        }
                        
                        // Create MapData
                        // Note: We need to get map dimensions and other config from somewhere.
                        // For now, hardcoding or using defaults.
                        let map_data = MapData {
                            version: MAP_VERSION,
                            map_width: FixedNum::from_num(editor_state.current_map_size.x),
                            map_height: FixedNum::from_num(editor_state.current_map_size.y),
                            cell_size: FixedNum::from_num(CELL_SIZE),
                            cluster_size: CLUSTER_SIZE,
                            obstacles,
                            start_locations: vec![], // TODO: Add start locations
                            cost_field: map_flow_field.0.cost_field.clone(), // Use current flow field cost
                            graph: Default::default(), // Will be generated on load
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
    editor_resources: Res<EditorResources>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    loading_query: Query<Entity, With<LoadingOverlayRoot>>,
    mut map_flow_field: ResMut<MapFlowField>,
    mut graph: ResMut<HierarchicalGraph>,
    mut build_state: ResMut<GraphBuildState>,
) {
    if !editor_state.is_generating {
        return;
    }

    // Wait for loading overlay to be spawned (next frame)
    if loading_query.is_empty() {
        return;
    }
    
    let Some(config) = game_configs.get(&config_handle.0) else {
        error!("Game config not found during map generation!");
        editor_state.is_generating = false;
        for entity in loading_query.iter() {
            commands.entity(entity).despawn();
        }
        return;
    };

    info!("Generating random map with {} obstacles...", editor_state.generation_params.num_obstacles);

    // Clear existing
    for entity in obstacle_query.iter() {
        commands.entity(entity).despawn();
    }
    
    // Reset Graph and Build State
    graph.reset();
    build_state.step = GraphBuildStep::Done;
    
    // Reset FlowField (but don't compute yet)
    let map_width = config.editor_map_size_x;
    let map_height = config.editor_map_size_y;
    
    editor_state.current_map_size = Vec2::new(map_width, map_height);

    map_flow_field.0 = FlowField::new(
        (map_width / CELL_SIZE) as usize, 
        (map_height / CELL_SIZE) as usize, 
        FixedNum::from_num(CELL_SIZE), 
        FixedVec2::new(FixedNum::from_num(-map_width/2.0), FixedNum::from_num(-map_height/2.0))
    );

    // Generate new
    let mut rng = rand::rng();
    let num_obstacles = editor_state.generation_params.num_obstacles;
    
    for _ in 0..num_obstacles {
        let x = rng.random_range(-map_width/2.0..map_width/2.0);
        let y = rng.random_range(-map_height/2.0..map_height/2.0);
        let radius = rng.random_range(editor_state.generation_params.min_radius..editor_state.generation_params.max_radius);
        
        spawn_obstacle(&mut commands, FixedVec2::new(FixedNum::from_num(x), FixedNum::from_num(y)), FixedNum::from_num(radius), &editor_resources);
    }

    editor_state.is_generating = false;
    info!("Map generation complete.");
    
    // Remove loading overlay
    for entity in loading_query.iter() {
        commands.entity(entity).despawn();
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
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
) {
    if !editor_state.placing_obstacle {
        return;
    }

    let Some(config) = game_configs.get(&config_handle.0) else { return };

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
                        spawn_obstacle(&mut commands, FixedVec2::new(FixedNum::from_num(intersection.x), FixedNum::from_num(intersection.z)), FixedNum::from_num(config.editor_default_obstacle_radius), &editor_resources);
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
