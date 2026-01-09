use bevy::prelude::*;
use crate::game::GameState;
use crate::game::loading::TargetGameState;
use super::components::*;

/// Main system that handles random map dialog logic
pub fn handle_random_map_dialog(
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

/// Handles clicks on input field buttons to focus them
pub fn handle_random_map_input_clicks(
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

/// Handles keyboard input for the active random map input field
pub fn keyboard_input_random_map(
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

/// Updates the displayed values in the random map dialog
pub fn update_random_map_dialog_values(
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

/// Updates field border colors to highlight the active field
pub fn update_random_map_field_borders(
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

/// Updates button colors based on interaction state
pub fn update_button_colors(
    mut interaction_query: Query<
        (&Interaction, &mut BackgroundColor, Option<&RandomMapDialogAction>),
        Changed<Interaction>,
    >,
) {
    for (interaction, mut color, dialog_action) in interaction_query.iter_mut() {
        let Some(action) = dialog_action else { continue };
        
        let (normal_color, hover_color, pressed_color) = match action {
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
        };
        
        *color = match *interaction {
            Interaction::Pressed => BackgroundColor(pressed_color),
            Interaction::Hovered => BackgroundColor(hover_color),
            Interaction::None => BackgroundColor(normal_color),
        };
    }
}

/// Spawns the random map generation dialog UI
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
