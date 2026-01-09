use bevy::prelude::*;
use super::components::*;

/// Sets up editor UI when entering editor state
pub fn setup_editor_ui(
    mut commands: Commands,
    mut editor_state: ResMut<EditorState>,
    map_flow_field: Res<crate::game::simulation::MapFlowField>,
    initial_config: Res<crate::game::config::InitialConfig>,
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

/// Cleans up editor UI when exiting editor state
pub fn cleanup_editor_ui(
    mut commands: Commands, 
    query: Query<Entity, With<EditorUiRoot>>, 
    dialog_query: Query<Entity, With<GenerationDialogRoot>>, 
    loading_query: Query<Entity, With<LoadingOverlayRoot>>
) {
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

/// Spawns the map generation parameter dialog
pub fn spawn_generation_dialog(commands: &mut Commands, editor_state: &EditorState, active_field: &ActiveInputField) {
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

/// Spawns a loading overlay with the given text
pub fn spawn_loading_overlay(commands: &mut Commands, text: &str) {
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

/// Cleans up generation overlay after generation completes
pub fn cleanup_generation_overlay(
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
