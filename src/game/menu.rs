use bevy::prelude::*;
use crate::game::GameState;

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
           .add_systems(Update, settings_action.run_if(in_state(GameState::Settings)));
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

#[derive(Component)]
enum SettingsButtonAction {
    Back,
}

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
) {
    for (interaction, action) in interaction_query.iter() {
        if *interaction == Interaction::Pressed {
            match action {
                MenuButtonAction::Play => {
                    next_state.set(GameState::InGame);
                }
                MenuButtonAction::Editor => {
                    next_state.set(GameState::Editor);
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

use crate::game::config::{GameConfig, GameConfigHandle};

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

fn setup_settings_menu(mut commands: Commands) {
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
            SettingsMenuRoot,
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("Settings"),
                TextFont {
                    font_size: 60.0,
                    ..default()
                },
                TextColor(Color::WHITE),
            ));

            parent.spawn((
                Text::new("TODO: Keybindings & Audio"),
                TextFont {
                    font_size: 30.0,
                    ..default()
                },
                TextColor(Color::srgb(0.5, 0.5, 0.5)),
            ));

            spawn_button!(parent, "Back", SettingsButtonAction::Back);
        });
}

fn cleanup_settings_menu(mut commands: Commands, query: Query<Entity, With<SettingsMenuRoot>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

fn settings_action(
    interaction_query: Query<
        (&Interaction, &SettingsButtonAction),
        (Changed<Interaction>, With<Button>),
    >,
    mut next_state: ResMut<NextState<GameState>>,
) {
    for (interaction, action) in interaction_query.iter() {
        if *interaction == Interaction::Pressed {
            match action {
                SettingsButtonAction::Back => {
                    next_state.set(GameState::MainMenu);
                }
            }
        }
    }
}
