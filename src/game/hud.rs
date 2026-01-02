use bevy::prelude::*;
use crate::game::GameState;
use crate::game::unit::Selected;
use crate::game::simulation::UnitStopCommand;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::InGame), setup_hud)
           .add_systems(OnExit(GameState::InGame), cleanup_hud)
           .add_systems(Update, (
               update_selection_hud,
               button_system,
               command_handler,
           ).run_if(in_state(GameState::InGame)));
    }
}

#[derive(Component)]
struct HudRoot;

#[derive(Component)]
struct SelectionText;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum CommandAction {
    Stop,
    Move,
    Attack,
}

#[derive(Component)]
struct CommandButton(CommandAction);

fn setup_hud(mut commands: Commands) {
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
            )).with_children(|p| {
                 p.spawn((
                    Text::new("Minimap"),
                    TextFont {
                        font_size: 20.0,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    Node {
                        margin: UiRect::all(Val::Auto),
                        ..default()
                    },
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

fn cleanup_hud(mut commands: Commands, query: Query<Entity, With<HudRoot>>) {
    for entity in &query {
        commands.entity(entity).despawn();
    }
}

fn button_system(
    mut interaction_query: Query<
        (&Interaction, &mut BackgroundColor),
        (Changed<Interaction>, With<Button>),
    >,
) {
    for (interaction, mut color) in &mut interaction_query {
        match *interaction {
            Interaction::Pressed => {
                *color = BackgroundColor(Color::srgb(0.1, 0.5, 0.1));
            }
            Interaction::Hovered => {
                *color = BackgroundColor(Color::srgb(0.4, 0.4, 0.4));
            }
            Interaction::None => {
                *color = BackgroundColor(Color::srgb(0.3, 0.3, 0.3));
            }
        }
    }
}

fn command_handler(
    interaction_query: Query<
        (&Interaction, &CommandButton),
        (Changed<Interaction>, With<Button>),
    >,
    mut stop_events: MessageWriter<UnitStopCommand>,
    selected_units: Query<Entity, With<Selected>>,
) {
    for (interaction, command) in &interaction_query {
        if *interaction == Interaction::Pressed {
            match command.0 {
                CommandAction::Stop => {
                    for entity in &selected_units {
                        stop_events.write(UnitStopCommand {
                            player_id: 0, // Local player
                            entity,
                        });
                    }
                }
                CommandAction::Move => {
                    info!("Move command clicked (not implemented via button)");
                }
                CommandAction::Attack => {
                    info!("Attack command clicked (not implemented)");
                }
            }
        }
    }
}

fn update_selection_hud(
    selected_units: Query<Entity, With<Selected>>,
    mut text_query: Query<&mut Text, With<SelectionText>>,
) {
    let count = selected_units.iter().count();
    for mut text in &mut text_query {
        if count == 0 {
            **text = "No Selection".to_string();
        } else if count == 1 {
            let entity = selected_units.single();
            **text = format!("Unit ID: {:?}", entity);
        } else {
            **text = format!("Selected: {} units", count);
        }
    }
}
