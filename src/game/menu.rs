use bevy::prelude::*;
use crate::game::GameState;
use crate::game::loading::TargetGameState;
use crate::game::config::{GameConfig, GameConfigHandle};
use std::fs;
use bevy::window::WindowMode;

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
        app.add_systems(OnEnter(GameState::MainMenu), setup_menu)
           .add_systems(OnExit(GameState::MainMenu), cleanup_menu)
           .add_systems(Update, menu_action.run_if(in_state(GameState::MainMenu)))
           
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
            
            // Editor Button
            spawn_button!(parent, "Map Editor", MenuButtonAction::Editor);

            // Settings Button
            spawn_button!(parent, "Settings", MenuButtonAction::Settings);

            // Quit Button
            spawn_button!(parent, "Quit", MenuButtonAction::Quit);
        });
}

fn cleanup_menu(mut commands: Commands, query: Query<Entity, With<MenuRoot>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

fn menu_action(
    interaction_query: Query<
        (&Interaction, &MenuButtonAction),
        (Changed<Interaction>, With<Button>),
    >,
    mut next_state: ResMut<NextState<GameState>>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
) {
    for (interaction, action) in interaction_query.iter() {
        if *interaction == Interaction::Pressed {
            match action {
                MenuButtonAction::Play => {
                    commands.insert_resource(TargetGameState(GameState::InGame));
                    next_state.set(GameState::Loading);
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
