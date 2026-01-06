use bevy::prelude::*;
use crate::game::GameState;
use crate::game::loading::TargetGameState;
use crate::game::config::{GameConfig, GameConfigHandle};
use std::fs;
use bevy::window::WindowMode;

#[derive(Resource, Default)]
struct RandomMapState {
    show_dialog: bool,
    map_width: String,
    map_height: String,
    num_obstacles: String,
    obstacle_size: String,
}

#[derive(Component, Clone, Copy, PartialEq)]
enum RandomMapInputField {
    MapWidth,
    MapHeight,
    NumObstacles,
    ObstacleSize,
}

#[derive(Resource, Default)]
struct ActiveRandomMapField {
    field: Option<RandomMapInputField>,
    first_input: bool,
}

pub struct MenuPlugin;

macro_rules! spawn_button {
    ($parent:expr, $text:expr, $action:expr) => {
        $parent.spawn((
            Button,
            Node {
                width: Val::Px(200.0),
                height: Val::Px(50.0),
                border: UiRect::all(Val::Px(2.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BorderColor::from(Color::BLACK),
            BackgroundColor(Color::srgb(0.2, 0.2, 0.2)),
            $action,
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new($text),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
        });
    };
}

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RandomMapState>()
           .init_resource::<ActiveRandomMapField>()
           .add_systems(OnEnter(GameState::MainMenu), setup_menu)
           .add_systems(OnExit(GameState::MainMenu), cleanup_menu)
           .add_systems(Update, (menu_action, handle_random_map_dialog, handle_random_map_input_clicks, keyboard_input_random_map, update_random_map_dialog_values, update_random_map_field_borders, update_button_colors).run_if(in_state(GameState::MainMenu)))
           
           // Pause Logic
           .add_systems(Update, toggle_pause.run_if(in_state(GameState::InGame).or(in_state(GameState::Paused)).or(in_state(GameState::Editor))))
           .add_systems(OnEnter(GameState::Paused), setup_pause_menu)
           .add_systems(OnExit(GameState::Paused), cleanup_pause_menu)
           .add_systems(Update, pause_menu_action.run_if(in_state(GameState::Paused)))
           
           // Settings Menu
           .add_systems(OnEnter(GameState::Settings), setup_settings_menu)
           .add_systems(OnExit(GameState::Settings), cleanup_settings_menu)
           .add_systems(Update, (settings_action, handle_rebinding).run_if(in_state(GameState::Settings)));
    }
}

#[derive(Component)]
struct MenuRoot;

#[derive(Component)]
enum MenuButtonAction {
    Play,
    PlayRandomMap,
    Editor,
    Settings,
    Quit,
}

#[derive(Component)]
struct PauseMenuRoot;

#[derive(Component)]
enum PauseButtonAction {
    Resume,
    MainMenu,
    Quit,
}

#[derive(Component)]
struct SettingsMenuRoot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BindableAction {
    CameraForward,
    CameraBackward,
    CameraLeft,
    CameraRight,
    DebugFlow,
    DebugGraph,
    DebugPath,
    SpawnBlackHole,
    SpawnWindSpot,
    SpawnUnit,
    SpawnBatch,
    Pause,
    ToggleHealthBars,
}

impl BindableAction {
    fn to_string(&self) -> String {
        match self {
            BindableAction::CameraForward => "Camera Forward".to_string(),
            BindableAction::CameraBackward => "Camera Backward".to_string(),
            BindableAction::CameraLeft => "Camera Left".to_string(),
            BindableAction::CameraRight => "Camera Right".to_string(),
            BindableAction::DebugFlow => "Debug Flow".to_string(),
            BindableAction::DebugGraph => "Debug Graph".to_string(),
            BindableAction::DebugPath => "Debug Path".to_string(),
            BindableAction::SpawnBlackHole => "Spawn Black Hole".to_string(),
            BindableAction::SpawnWindSpot => "Spawn Wind Spot".to_string(),
            BindableAction::SpawnUnit => "Spawn Unit".to_string(),
            BindableAction::SpawnBatch => "Spawn Batch".to_string(),
            BindableAction::Pause => "Pause".to_string(),
            BindableAction::ToggleHealthBars => "Toggle Health Bars".to_string(),
        }
    }
}

#[derive(Component)]
enum SettingsButtonAction {
    Back,
    Rebind(BindableAction),
    ToggleFullscreen,
    Save,
}

#[derive(Component)]
enum RandomMapDialogAction {
    Generate,
    Cancel,
    IncrementMapWidth,
    DecrementMapWidth,
    IncrementMapHeight,
    DecrementMapHeight,
    IncrementObstacles,
    DecrementObstacles,
    IncrementObstacleSize,
    DecrementObstacleSize,
}

#[derive(Component)]
struct RandomMapDialogRoot;

#[derive(Component, Clone, Copy, PartialEq)]
enum RandomMapValueText {
    MapWidth,
    MapHeight,
    NumObstacles,
    ObstacleSize,
}

#[derive(Component)]
struct Rebinding;


fn setup_menu(mut commands: Commands) {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(20.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.1, 0.1, 0.1)),
            MenuRoot,
        ))
        .with_children(|parent| {
            // Title
            parent.spawn((
                Text::new("Peregrine RTS"),
                TextFont {
                    font_size: 60.0,
                    ..default()
                },
                TextColor(Color::WHITE),
            ));

            // Play Button
            spawn_button!(parent, "Play Game", MenuButtonAction::Play);
            
            // Play Random Map Button
            spawn_button!(parent, "Play Random Map", MenuButtonAction::PlayRandomMap);
            
            // Editor Button
            spawn_button!(parent, "Map Editor", MenuButtonAction::Editor);

            // Settings Button
            spawn_button!(parent, "Settings", MenuButtonAction::Settings);

            // Quit Button
            spawn_button!(parent, "Quit", MenuButtonAction::Quit);
        });
}

fn cleanup_menu(
    mut commands: Commands,
    query: Query<Entity, With<MenuRoot>>,
    dialog_query: Query<Entity, With<RandomMapDialogRoot>>,
    mut random_map_state: ResMut<RandomMapState>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
    for entity in dialog_query.iter() {
        commands.entity(entity).despawn();
    }
    random_map_state.show_dialog = false;
}

fn menu_action(
    interaction_query: Query<
        (&Interaction, &MenuButtonAction),
        (Changed<Interaction>, With<Button>),
    >,
    mut next_state: ResMut<NextState<GameState>>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
    mut random_map_state: ResMut<RandomMapState>,
) {
    for (interaction, action) in interaction_query.iter() {
        if *interaction == Interaction::Pressed {
            match action {
                MenuButtonAction::Play => {
                    commands.insert_resource(TargetGameState(GameState::InGame));
                    next_state.set(GameState::Loading);
                }
                MenuButtonAction::PlayRandomMap => {
                    // Initialize default values if empty
                    if random_map_state.map_width.is_empty() {
                        random_map_state.map_width = "500".to_string();
                        random_map_state.map_height = "500".to_string();
                        random_map_state.num_obstacles = "50".to_string();
                        random_map_state.obstacle_size = "20".to_string();
                    }
                    random_map_state.show_dialog = true;
                }
                MenuButtonAction::Editor => {
                    commands.insert_resource(TargetGameState(GameState::Editor));
                    next_state.set(GameState::Loading);
                }
                MenuButtonAction::Settings => {
                    next_state.set(GameState::Settings);
                }
                MenuButtonAction::Quit => {
                    exit.write(AppExit::Success);
                }
            }
        }
    }
}

fn toggle_pause(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameState>>,
    mut next_state: ResMut<NextState<GameState>>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
) {
    let Some(config) = game_configs.get(&config_handle.0) else { return };

    if keys.just_pressed(config.key_pause) {
        match state.get() {
            GameState::InGame | GameState::Editor => next_state.set(GameState::Paused),
            GameState::Paused => next_state.set(GameState::InGame), // Or return to previous state? For now InGame.
            _ => {}
        }
    }
}

fn setup_pause_menu(mut commands: Commands) {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(20.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)), // Semi-transparent black
            PauseMenuRoot,
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("Paused"),
                TextFont {
                    font_size: 60.0,
                    ..default()
                },
                TextColor(Color::WHITE),
            ));

            spawn_button!(parent, "Resume", PauseButtonAction::Resume);
            spawn_button!(parent, "Main Menu", PauseButtonAction::MainMenu);
            spawn_button!(parent, "Quit", PauseButtonAction::Quit);
        });
}

fn cleanup_pause_menu(mut commands: Commands, query: Query<Entity, With<PauseMenuRoot>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

fn pause_menu_action(
    interaction_query: Query<
        (&Interaction, &PauseButtonAction),
        (Changed<Interaction>, With<Button>),
    >,
    mut next_state: ResMut<NextState<GameState>>,
    mut exit: MessageWriter<AppExit>,
) {
    for (interaction, action) in interaction_query.iter() {
        if *interaction == Interaction::Pressed {
            match action {
                PauseButtonAction::Resume => {
                    next_state.set(GameState::InGame);
                }
                PauseButtonAction::MainMenu => {
                    next_state.set(GameState::MainMenu);
                }
                PauseButtonAction::Quit => {
                    exit.write(AppExit::Success);
                }
            }
        }
    }
}

fn setup_settings_menu(
    mut commands: Commands,
    config_handle: Res<GameConfigHandle>,
    config_assets: Res<Assets<GameConfig>>,
    windows: Query<&Window>,
) {
    let config = if let Some(config) = config_assets.get(&config_handle.0) {
        config
    } else {
        return;
    };

    let window = windows.single().unwrap();
    let fullscreen_text = match window.mode {
        WindowMode::Windowed => "Fullscreen: Off",
        _ => "Fullscreen: On",
    };

    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(10.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.1, 0.1, 0.1)),
            SettingsMenuRoot,
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("Settings"),
                TextFont {
                    font_size: 40.0,
                    ..default()
                },
                TextColor(Color::WHITE),
            ));

            let actions = [
                (BindableAction::CameraForward, config.key_camera_forward),
                (BindableAction::CameraBackward, config.key_camera_backward),
                (BindableAction::CameraLeft, config.key_camera_left),
                (BindableAction::CameraRight, config.key_camera_right),
                (BindableAction::DebugFlow, config.key_debug_flow),
                (BindableAction::DebugGraph, config.key_debug_graph),
                (BindableAction::DebugPath, config.key_debug_path),
                (BindableAction::SpawnBlackHole, config.key_spawn_black_hole),
                (BindableAction::SpawnWindSpot, config.key_spawn_wind_spot),
                (BindableAction::SpawnUnit, config.key_spawn_unit),
                (BindableAction::SpawnBatch, config.key_spawn_batch),
                (BindableAction::Pause, config.key_pause),
                (BindableAction::ToggleHealthBars, config.key_toggle_health_bars),
            ];

            for (action, key) in actions {
                parent.spawn((
                    Node {
                        width: Val::Px(500.0),
                        height: Val::Px(30.0),
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                )).with_children(|row| {
                    row.spawn((
                        Text::new(action.to_string()),
                        TextFont {
                            font_size: 18.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));

                    row.spawn((
                        Button,
                        Node {
                            width: Val::Px(200.0),
                            height: Val::Px(28.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        BorderColor::from(Color::WHITE),
                        BackgroundColor(Color::srgb(0.2, 0.2, 0.2)),
                        SettingsButtonAction::Rebind(action),
                    )).with_children(|btn| {
                        btn.spawn((
                            Text::new(format!("{:?}", key)),
                            TextFont {
                                font_size: 18.0,
                                ..default()
                            },
                            TextColor(Color::WHITE),
                        ));
                    });
                });
            }

            spawn_button!(parent, fullscreen_text, SettingsButtonAction::ToggleFullscreen);
            spawn_button!(parent, "Save Settings", SettingsButtonAction::Save);
            spawn_button!(parent, "Back", SettingsButtonAction::Back);
        });
}

fn cleanup_settings_menu(mut commands: Commands, query: Query<Entity, With<SettingsMenuRoot>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

fn settings_action(
    mut commands: Commands,
    interaction_query: Query<
        (Entity, &Interaction, &SettingsButtonAction, &Children),
        (Changed<Interaction>, With<Button>),
    >,
    mut text_query: Query<&mut Text>,
    mut next_state: ResMut<NextState<GameState>>,
    rebinding_query: Query<Entity, With<Rebinding>>,
    mut windows: Query<&mut Window>,
    config_assets: Res<Assets<GameConfig>>,
    config_handle: Res<GameConfigHandle>,
) {
    if !rebinding_query.is_empty() {
        return;
    }

    for (entity, interaction, action, children) in interaction_query.iter() {
        if *interaction == Interaction::Pressed {
            match action {
                SettingsButtonAction::Back => {
                    next_state.set(GameState::MainMenu);
                }
                SettingsButtonAction::Rebind(_) => {
                    commands.entity(entity).insert(Rebinding);
                    for child in children.iter() {
                        if let Ok(mut text) = text_query.get_mut(child) {
                            text.0 = "Press any key...".to_string();
                        }
                    }
                }
                SettingsButtonAction::ToggleFullscreen => {
                    let mut window = windows.single_mut().unwrap();
                    window.mode = match window.mode {
                        WindowMode::Windowed => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
                        _ => WindowMode::Windowed,
                    };
                    
                    let new_text = match window.mode {
                        WindowMode::Windowed => "Fullscreen: Off",
                        _ => "Fullscreen: On",
                    };
                    for child in children.iter() {
                        if let Ok(mut text) = text_query.get_mut(child) {
                            text.0 = new_text.to_string();
                        }
                    }
                }
                SettingsButtonAction::Save => {
                    if let Some(config) = config_assets.get(&config_handle.0) {
                        match ron::ser::to_string_pretty(config, ron::ser::PrettyConfig::default()) {
                            Ok(ron_string) => {
                                if let Err(e) = fs::write("assets/game_config.ron", ron_string) {
                                    error!("Failed to save config: {}", e);
                                } else {
                                    info!("Config saved!");
                                }
                            }
                            Err(e) => {
                                error!("Failed to serialize config: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn handle_rebinding(
    mut commands: Commands,
    mut keys: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut config_assets: ResMut<Assets<GameConfig>>,
    config_handle: Res<GameConfigHandle>,
    rebinding_query: Query<(Entity, &SettingsButtonAction, &Children), With<Rebinding>>,
    mut text_query: Query<&mut Text>,
) {
    let Ok((entity, action, children)) = rebinding_query.single().map_err(|_| ()) else {
        return;
    };

    for event in keys.read() {
        if event.state.is_pressed() {
            let new_key = event.key_code;
            
            if let Some(config) = config_assets.get_mut(&config_handle.0) {
                if let SettingsButtonAction::Rebind(bindable) = action {
                    match bindable {
                        BindableAction::CameraForward => config.key_camera_forward = new_key,
                        BindableAction::CameraBackward => config.key_camera_backward = new_key,
                        BindableAction::CameraLeft => config.key_camera_left = new_key,
                        BindableAction::CameraRight => config.key_camera_right = new_key,
                        BindableAction::DebugFlow => config.key_debug_flow = new_key,
                        BindableAction::DebugGraph => config.key_debug_graph = new_key,
                        BindableAction::DebugPath => config.key_debug_path = new_key,
                        BindableAction::SpawnBlackHole => config.key_spawn_black_hole = new_key,
                        BindableAction::SpawnWindSpot => config.key_spawn_wind_spot = new_key,
                        BindableAction::SpawnUnit => config.key_spawn_unit = new_key,
                        BindableAction::SpawnBatch => config.key_spawn_batch = new_key,
                        BindableAction::Pause => config.key_pause = new_key,
                        BindableAction::ToggleHealthBars => config.key_toggle_health_bars = new_key,
                    }
                }
            }

            for child in children.iter() {
                if let Ok(mut text) = text_query.get_mut(child) {
                    text.0 = format!("{:?}", new_key);
                }
            }

            commands.entity(entity).remove::<Rebinding>();
            break;
        }
    }
}

// Random Map Dialog Systems

fn handle_random_map_dialog(
    mut commands: Commands,
    mut random_map_state: ResMut<RandomMapState>,
    active_field: Res<ActiveRandomMapField>,
    existing_dialog: Query<Entity, With<RandomMapDialogRoot>>,
    interaction_query: Query<
        (&Interaction, &RandomMapDialogAction),
        (Changed<Interaction>, With<Button>),
    >,
    mut next_state: ResMut<NextState<GameState>>,
) {
    // Spawn or despawn dialog based on state
    if random_map_state.show_dialog && existing_dialog.is_empty() {
        spawn_random_map_dialog(&mut commands, &random_map_state, &active_field);
    } else if !random_map_state.show_dialog && !existing_dialog.is_empty() {
        for entity in existing_dialog.iter() {
            commands.entity(entity).despawn();
        }
    }
    
    // Handle button interactions
    for (interaction, action) in interaction_query.iter() {
        if *interaction == Interaction::Pressed {
            match action {
                RandomMapDialogAction::Generate => {
                    // Store the generation params in a resource for the loading system to use
                    if let (Ok(width), Ok(height), Ok(obstacles), Ok(size)) = (
                        random_map_state.map_width.parse::<f32>(),
                        random_map_state.map_height.parse::<f32>(),
                        random_map_state.num_obstacles.parse::<usize>(),
                        random_map_state.obstacle_size.parse::<f32>(),
                    ) {
                        commands.insert_resource(crate::game::editor::PendingMapGeneration {
                            map_width: width,
                            map_height: height,
                            num_obstacles: obstacles,
                            min_radius: size * 0.5,
                            max_radius: size * 1.5,
                        });
                        commands.insert_resource(TargetGameState(GameState::InGame));
                        next_state.set(GameState::Loading);
                        random_map_state.show_dialog = false;
                    }
                }
                RandomMapDialogAction::Cancel => {
                    random_map_state.show_dialog = false;
                }
                RandomMapDialogAction::IncrementMapWidth => {
                    if let Ok(mut val) = random_map_state.map_width.parse::<i32>() {
                        val += 100;
                        random_map_state.map_width = val.to_string();
                    }
                }
                RandomMapDialogAction::DecrementMapWidth => {
                    if let Ok(mut val) = random_map_state.map_width.parse::<i32>() {
                        val = (val - 100).max(100);
                        random_map_state.map_width = val.to_string();
                    }
                }
                RandomMapDialogAction::IncrementMapHeight => {
                    if let Ok(mut val) = random_map_state.map_height.parse::<i32>() {
                        val += 100;
                        random_map_state.map_height = val.to_string();
                    }
                }
                RandomMapDialogAction::DecrementMapHeight => {
                    if let Ok(mut val) = random_map_state.map_height.parse::<i32>() {
                        val = (val - 100).max(100);
                        random_map_state.map_height = val.to_string();
                    }
                }
                RandomMapDialogAction::IncrementObstacles => {
                    if let Ok(mut val) = random_map_state.num_obstacles.parse::<i32>() {
                        val += 10;
                        random_map_state.num_obstacles = val.to_string();
                    }
                }
                RandomMapDialogAction::DecrementObstacles => {
                    if let Ok(mut val) = random_map_state.num_obstacles.parse::<i32>() {
                        val = (val - 10).max(0);
                        random_map_state.num_obstacles = val.to_string();
                    }
                }
                RandomMapDialogAction::IncrementObstacleSize => {
                    if let Ok(mut val) = random_map_state.obstacle_size.parse::<i32>() {
                        val += 5;
                        random_map_state.obstacle_size = val.to_string();
                    }
                }
                RandomMapDialogAction::DecrementObstacleSize => {
                    if let Ok(mut val) = random_map_state.obstacle_size.parse::<i32>() {
                        val = (val - 5).max(5);
                        random_map_state.obstacle_size = val.to_string();
                    }
                }
            }
        }
    }
}

fn handle_random_map_input_clicks(
    interaction_query: Query<
        (&Interaction, &RandomMapInputField),
        (Changed<Interaction>, With<Button>),
    >,
    mut active_field: ResMut<ActiveRandomMapField>,
) {
    for (interaction, field_type) in interaction_query.iter() {
        if *interaction == Interaction::Pressed {
            active_field.field = Some(*field_type);
            active_field.first_input = true;
        }
    }
}

fn keyboard_input_random_map(
    mut keys: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut active_field: ResMut<ActiveRandomMapField>,
    mut random_map_state: ResMut<RandomMapState>,
) {
    let Some(field) = active_field.field else { return };
    
    for event in keys.read() {
        if event.state.is_pressed() {
            let key = event.key_code;
            
            let target_string = match field {
                RandomMapInputField::MapWidth => &mut random_map_state.map_width,
                RandomMapInputField::MapHeight => &mut random_map_state.map_height,
                RandomMapInputField::NumObstacles => &mut random_map_state.num_obstacles,
                RandomMapInputField::ObstacleSize => &mut random_map_state.obstacle_size,
            };
            
            // Clear on first input
            if active_field.first_input {
                target_string.clear();
                active_field.first_input = false;
            }
            
            match key {
                KeyCode::Digit0 => target_string.push('0'),
                KeyCode::Digit1 => target_string.push('1'),
                KeyCode::Digit2 => target_string.push('2'),
                KeyCode::Digit3 => target_string.push('3'),
                KeyCode::Digit4 => target_string.push('4'),
                KeyCode::Digit5 => target_string.push('5'),
                KeyCode::Digit6 => target_string.push('6'),
                KeyCode::Digit7 => target_string.push('7'),
                KeyCode::Digit8 => target_string.push('8'),
                KeyCode::Digit9 => target_string.push('9'),
                KeyCode::Backspace => { target_string.pop(); },
                KeyCode::Enter | KeyCode::Escape => {
                    active_field.field = None;
                }
                _ => {}
            }
        }
    }
}

fn update_random_map_dialog_values(
    random_map_state: Res<RandomMapState>,
    active_field: Res<ActiveRandomMapField>,
    mut value_text_query: Query<(&mut Text, &RandomMapValueText)>,
) {
    if !random_map_state.is_changed() && !active_field.is_changed() {
        return;
    }
    
    for (mut text, value_type) in value_text_query.iter_mut() {
        let (value, field_type) = match value_type {
            RandomMapValueText::MapWidth => (&random_map_state.map_width, RandomMapInputField::MapWidth),
            RandomMapValueText::MapHeight => (&random_map_state.map_height, RandomMapInputField::MapHeight),
            RandomMapValueText::NumObstacles => (&random_map_state.num_obstacles, RandomMapInputField::NumObstacles),
            RandomMapValueText::ObstacleSize => (&random_map_state.obstacle_size, RandomMapInputField::ObstacleSize),
        };
        
        let is_active = active_field.field == Some(field_type);
        let display_value = if is_active { 
            format!("{}_", value) 
        } else { 
            value.to_string() 
        };
        
        text.0 = display_value;
    }
}

fn update_random_map_field_borders(
    active_field: Res<ActiveRandomMapField>,
    mut field_query: Query<(&RandomMapInputField, &mut BorderColor)>,
) {
    if !active_field.is_changed() {
        return;
    }
    
    for (field_type, mut border_color) in field_query.iter_mut() {
        let is_active = active_field.field == Some(*field_type);
        let color = if is_active { 
            Color::srgb(0.3, 0.7, 1.0) 
        } else { 
            Color::srgb(0.5, 0.5, 0.5) 
        };
        *border_color = BorderColor::from(color);
    }
}

fn update_button_colors(
    mut interaction_query: Query<
        (&Interaction, &mut BackgroundColor, Option<&MenuButtonAction>, Option<&PauseButtonAction>, Option<&RandomMapDialogAction>),
        Changed<Interaction>,
    >,
) {
    for (interaction, mut color, menu_action, pause_action, dialog_action) in interaction_query.iter_mut() {
        // Determine the base color based on button type
        let (normal_color, hover_color, pressed_color) = if dialog_action.is_some() {
            match dialog_action.unwrap() {
                RandomMapDialogAction::Generate => (
                    Color::srgb(0.3, 0.6, 0.3),
                    Color::srgb(0.35, 0.7, 0.35),
                    Color::srgb(0.25, 0.5, 0.25),
                ),
                RandomMapDialogAction::Cancel => (
                    Color::srgb(0.6, 0.3, 0.3),
                    Color::srgb(0.7, 0.35, 0.35),
                    Color::srgb(0.5, 0.25, 0.25),
                ),
                RandomMapDialogAction::IncrementMapWidth | RandomMapDialogAction::IncrementMapHeight | 
                RandomMapDialogAction::IncrementObstacles | RandomMapDialogAction::IncrementObstacleSize => (
                    Color::srgb(0.3, 0.5, 0.3),
                    Color::srgb(0.35, 0.6, 0.35),
                    Color::srgb(0.25, 0.4, 0.25),
                ),
                _ => (
                    Color::srgb(0.5, 0.3, 0.3),
                    Color::srgb(0.6, 0.35, 0.35),
                    Color::srgb(0.4, 0.25, 0.25),
                ),
            }
        } else if menu_action.is_some() || pause_action.is_some() {
            (
                Color::srgb(0.2, 0.2, 0.2),
                Color::srgb(0.3, 0.3, 0.3),
                Color::srgb(0.15, 0.15, 0.15),
            )
        } else {
            continue;
        };
        
        *color = match *interaction {
            Interaction::Pressed => BackgroundColor(pressed_color),
            Interaction::Hovered => BackgroundColor(hover_color),
            Interaction::None => BackgroundColor(normal_color),
        };
    }
}

fn spawn_random_map_dialog(
    commands: &mut Commands,
    random_map_state: &RandomMapState,
    active_field: &ActiveRandomMapField,
) {
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
        RandomMapDialogRoot,
    )).with_children(|parent| {
        parent.spawn((
            Text::new("Generate Random Map"),
            TextFont { font_size: 24.0, ..default() },
            TextColor(Color::WHITE),
            Node { margin: UiRect::bottom(Val::Px(20.0)), ..default() },
        ));

        // Helper macro to create adjustable value rows
        macro_rules! create_value_row {
            ($label:expr, $value:expr, $dec:expr, $inc:expr, $field_type:expr, $value_marker:expr) => {
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
                    row.spawn((
                        Text::new($label),
                        TextFont { font_size: 18.0, ..default() },
                        TextColor(Color::WHITE),
                        Node { width: Val::Px(180.0), ..default() },
                    ));
                    
                    row.spawn((
                        Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                    )).with_children(|controls| {
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
                                $value_marker,
                            ));
                        });
                        
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

        create_value_row!("Map Width:", &random_map_state.map_width, RandomMapDialogAction::DecrementMapWidth, RandomMapDialogAction::IncrementMapWidth, RandomMapInputField::MapWidth, RandomMapValueText::MapWidth);
        create_value_row!("Map Height:", &random_map_state.map_height, RandomMapDialogAction::DecrementMapHeight, RandomMapDialogAction::IncrementMapHeight, RandomMapInputField::MapHeight, RandomMapValueText::MapHeight);
        create_value_row!("Num Obstacles:", &random_map_state.num_obstacles, RandomMapDialogAction::DecrementObstacles, RandomMapDialogAction::IncrementObstacles, RandomMapInputField::NumObstacles, RandomMapValueText::NumObstacles);
        create_value_row!("Obstacle Radius:", &random_map_state.obstacle_size, RandomMapDialogAction::DecrementObstacleSize, RandomMapDialogAction::IncrementObstacleSize, RandomMapInputField::ObstacleSize, RandomMapValueText::ObstacleSize);

        parent.spawn((
            Text::new("Tip: Start small (500x500, 50 obstacles) and increase gradually"),
            TextFont { font_size: 14.0, ..default() },
            TextColor(Color::srgb(0.7, 0.7, 0.7)),
            Node { margin: UiRect::vertical(Val::Px(15.0)), ..default() },
        ));

        parent.spawn((
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(10.0),
                ..default()
            },
        )).with_children(|buttons| {
            buttons.spawn((
                Button,
                Node {
                    width: Val::Px(120.0),
                    height: Val::Px(40.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.3, 0.6, 0.3)),
                RandomMapDialogAction::Generate,
            )).with_children(|btn| {
                btn.spawn((
                    Text::new("Generate"),
                    TextFont { font_size: 18.0, ..default() },
                    TextColor(Color::WHITE),
                ));
            });
            
            buttons.spawn((
                Button,
                Node {
                    width: Val::Px(120.0),
                    height: Val::Px(40.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.6, 0.3, 0.3)),
                RandomMapDialogAction::Cancel,
            )).with_children(|btn| {
                btn.spawn((
                    Text::new("Cancel"),
                    TextFont { font_size: 18.0, ..default() },
                    TextColor(Color::WHITE),
                ));
            });
        });
    });
}
