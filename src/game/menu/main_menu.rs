use bevy::prelude::*;
use crate::game::GameState;
use crate::game::loading::TargetGameState;
use super::components::*;
use super::ui_utils::spawn_button;

/// Sets up the main menu UI
pub fn setup_menu(mut commands: Commands) {
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

/// Cleans up main menu entities
pub fn cleanup_menu(
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

/// Handles main menu button interactions
pub fn menu_action(
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
