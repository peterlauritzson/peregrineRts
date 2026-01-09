use bevy::prelude::*;
use super::components::*;

/// Setup the HUD UI elements
pub fn setup_hud(mut commands: Commands) {
    // Root node for the HUD
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::FlexEnd, // Align items to the bottom
                flex_direction: FlexDirection::Row,
                ..default()
            },
            HudRoot,
        ))
        .with_children(|parent| {
            // Bottom Left: Minimap Placeholder
            parent.spawn((
                Node {
                    width: Val::Px(200.0),
                    height: Val::Px(200.0),
                    border: UiRect::all(Val::Px(2.0)),
                    margin: UiRect::all(Val::Px(10.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.1, 0.1, 0.1)),
                BorderColor::from(Color::WHITE),
                Minimap,
            )).with_children(|p| {
                 p.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        border: UiRect::all(Val::Px(1.0)),
                        width: Val::Px(40.0), 
                        height: Val::Px(30.0),
                        ..default()
                    },
                    BorderColor::from(Color::WHITE),
                    MinimapCameraFrame,
                ));
            });

            // Bottom Center: Selection Info
            parent.spawn((
                Node {
                    width: Val::Px(400.0),
                    height: Val::Px(150.0),
                    border: UiRect::all(Val::Px(2.0)),
                    margin: UiRect::bottom(Val::Px(10.0)),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.1, 0.1, 0.1)),
                BorderColor::from(Color::WHITE),
            )).with_children(|p| {
                p.spawn((
                    Text::new("No Selection"),
                    TextFont {
                        font_size: 20.0,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    Node {
                        margin: UiRect::top(Val::Px(10.0)),
                        ..default()
                    },
                    SelectionText,
                ));
            });

            // Bottom Right: Command Card
            parent.spawn((
                Node {
                    width: Val::Px(200.0),
                    height: Val::Px(200.0),
                    border: UiRect::all(Val::Px(2.0)),
                    margin: UiRect::all(Val::Px(10.0)),
                    display: Display::Grid,
                    grid_template_columns: vec![GridTrack::fr(1.0); 3],
                    grid_template_rows: vec![GridTrack::fr(1.0); 3],
                    row_gap: Val::Px(5.0),
                    column_gap: Val::Px(5.0),
                    padding: UiRect::all(Val::Px(5.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.1, 0.1, 0.1)),
                BorderColor::from(Color::WHITE),
            )).with_children(|p| {
                // Command Buttons
                let commands_list = [
                    ("Move", CommandAction::Move),
                    ("Stop", CommandAction::Stop),
                    ("Attack", CommandAction::Attack),
                ];

                for (label, action) in commands_list {
                    p.spawn((
                        Button,
                        Node {
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.3, 0.3, 0.3)),
                        CommandButton(action),
                    )).with_children(|btn| {
                         btn.spawn((
                            Text::new(label),
                            TextFont {
                                font_size: 15.0,
                                ..default()
                            },
                            TextColor(Color::WHITE),
                        ));
                    });
                }

                // Fill rest with empty slots
                for _ in 0..(9 - commands_list.len()) {
                     p.spawn((
                        Node {
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ));
                }
            });
        });
    
    // Top Bar: Resources (Absolute positioning to stay at top)
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Px(40.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
        HudRoot,
    )).with_children(|parent| {
         parent.spawn((
            Text::new("Resources: 0 | Supply: 0/200"),
            TextFont {
                font_size: 20.0,
                ..default()
            },
            TextColor(Color::WHITE),
        ));
    });
}

/// Cleanup HUD elements when leaving the InGame state
pub fn cleanup_hud(
    mut commands: Commands,
    query: Query<Entity, With<HudRoot>>,
    unit_query: Query<Entity, With<UnitMinimapDot>>,
) {
    for entity in &query {
        commands.entity(entity).despawn();
    }
    for entity in &unit_query {
        commands.entity(entity).remove::<UnitMinimapDot>();
    }
}
