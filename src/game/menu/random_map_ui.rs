/// UI construction for random map generation dialog

use bevy::prelude::*;
use crate::game::menu::components::*;

/// Spawns the random map generation dialog UI
pub(super) fn spawn_random_map_dialog(
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
