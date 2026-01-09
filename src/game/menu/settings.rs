use bevy::prelude::*;
use bevy::window::WindowMode;
use std::fs;
use crate::game::GameState;
use crate::game::config::{GameConfig, GameConfigHandle};
use super::components::*;
use super::ui_utils::spawn_button;

/// Sets up the settings menu UI
pub fn setup_settings_menu(
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

/// Cleans up settings menu entities
pub fn cleanup_settings_menu(mut commands: Commands, query: Query<Entity, With<SettingsMenuRoot>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

/// Handles settings menu button interactions
pub fn settings_action(
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

/// Handles keyboard input when rebinding keys
pub fn handle_rebinding(
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
