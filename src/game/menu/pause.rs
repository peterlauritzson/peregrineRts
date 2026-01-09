use bevy::prelude::*;
use crate::game::GameState;
use crate::game::config::{GameConfig, GameConfigHandle};
use super::components::*;
use super::ui_utils::spawn_button;

/// Toggles pause state when pause key is pressed
pub fn toggle_pause(
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

/// Sets up the pause menu UI overlay
pub fn setup_pause_menu(mut commands: Commands) {
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

/// Cleans up pause menu entities
pub fn cleanup_pause_menu(mut commands: Commands, query: Query<Entity, With<PauseMenuRoot>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

/// Handles pause menu button interactions
pub fn pause_menu_action(
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
