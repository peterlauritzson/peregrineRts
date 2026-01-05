use bevy::prelude::*;
use crate::game::GameState;
use crate::game::unit::{Selected, Health};
use crate::game::simulation::{UnitStopCommand, SimConfig};
use crate::game::control::InputMode;
use crate::game::simulation::SimPosition;
use crate::game::camera::RtsCamera;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::InGame), setup_hud)
           .add_systems(OnExit(GameState::InGame), cleanup_hud)
           .add_systems(Update, (
               update_selection_hud,
               button_system,
               command_handler,
               minimap_system,
               minimap_input_system,
           ).run_if(in_state(GameState::InGame)));
    }
}

#[derive(Component)]
struct HudRoot;

#[derive(Component)]
struct Minimap;

#[derive(Component)]
struct MinimapCameraFrame;

#[derive(Component)]
struct MinimapDot(Entity);

#[derive(Component)]
struct UnitMinimapDot;

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

fn minimap_system(
    mut commands: Commands,
    q_minimap: Query<(Entity, &ComputedNode), (With<Minimap>, Without<MinimapDot>)>,
    q_units: Query<(Entity, &SimPosition, Option<&Selected>), Without<UnitMinimapDot>>,
    mut q_dots: Query<(Entity, &MinimapDot, &mut Node, &mut BackgroundColor), Without<Minimap>>,
    q_units_lookup: Query<(&SimPosition, Option<&Selected>)>,
    q_camera: Query<&Transform, With<RtsCamera>>,
    mut q_camera_frame: Query<&mut Node, (With<MinimapCameraFrame>, Without<MinimapDot>, Without<Minimap>)>,
    sim_config: Res<SimConfig>,
) {
    let Ok((minimap_entity, minimap_node)) = q_minimap.single() else { return };
    
    let map_width = sim_config.map_width.to_num::<f32>();
    let map_height = sim_config.map_height.to_num::<f32>();
    let minimap_w = minimap_node.size().x;
    let minimap_h = minimap_node.size().y;

    // Spawn new dots
    for (unit_entity, pos, selected) in q_units.iter() {
        let x_pct = (pos.0.x.to_num::<f32>() + map_width / 2.0) / map_width;
        let y_pct = (pos.0.y.to_num::<f32>() + map_height / 2.0) / map_height;

        let x = (x_pct * minimap_w).clamp(0.0, minimap_w);
        let y = (y_pct * minimap_h).clamp(0.0, minimap_h);

        let dot = commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(x - 2.0),
                top: Val::Px(y - 2.0),
                width: Val::Px(4.0),
                height: Val::Px(4.0),
                ..default()
            },
            BackgroundColor(if selected.is_some() { Color::srgb(0.0, 1.0, 0.0) } else { Color::srgb(1.0, 0.0, 0.0) }),
            MinimapDot(unit_entity),
        )).id();

        commands.entity(minimap_entity).add_child(dot);
        commands.entity(unit_entity).insert(UnitMinimapDot);
    }

    // Update existing dots
    for (dot_entity, dot_link, mut node, mut bg_color) in q_dots.iter_mut() {
        if let Ok((pos, selected)) = q_units_lookup.get(dot_link.0) {
             let x_pct = (pos.0.x.to_num::<f32>() + map_width / 2.0) / map_width;
             let y_pct = (pos.0.y.to_num::<f32>() + map_height / 2.0) / map_height;
             
             let x = (x_pct * minimap_w).clamp(0.0, minimap_w);
             let y = (y_pct * minimap_h).clamp(0.0, minimap_h);

             node.left = Val::Px(x - 2.0);
             node.top = Val::Px(y - 2.0);
             *bg_color = BackgroundColor(if selected.is_some() { Color::srgb(0.0, 1.0, 0.0) } else { Color::srgb(1.0, 0.0, 0.0) });
        } else {
            // Unit dead
            commands.entity(dot_entity).despawn();
        }
    }

    // Update Camera Frame
    if let Ok(camera_transform) = q_camera.single() {
        if let Ok(mut frame_node) = q_camera_frame.single_mut() {
            let x_pct = (camera_transform.translation.x + map_width / 2.0) / map_width;
            let y_pct = (camera_transform.translation.z + map_height / 2.0) / map_height;

            let x = (x_pct * minimap_w).clamp(0.0, minimap_w);
            let y = (y_pct * minimap_h).clamp(0.0, minimap_h);

            // Assuming frame size 40x30
            frame_node.left = Val::Px(x - 20.0);
            frame_node.top = Val::Px(y - 15.0);
        }
    }
}

use bevy::window::PrimaryWindow;

fn minimap_input_system(
    mouse_button: Res<ButtonInput<MouseButton>>,
    q_window: Query<&Window, With<PrimaryWindow>>,
    q_minimap: Query<(&ComputedNode, &GlobalTransform), With<Minimap>>,
    mut q_camera: Query<&mut Transform, With<RtsCamera>>,
    sim_config: Res<SimConfig>,
) {
    if !mouse_button.pressed(MouseButton::Left) {
        return;
    }

    let Some(window) = q_window.iter().next() else { return };
    let Some(cursor_pos) = window.cursor_position() else { return };
    let Ok((computed_node, transform)) = q_minimap.single() else { return };

    let size = computed_node.size();
    let pos = transform.translation().truncate();
    let rect = Rect::from_center_size(pos, size);

    if rect.contains(cursor_pos) {
        let relative_x = cursor_pos.x - rect.min.x;
        let relative_y = cursor_pos.y - rect.min.y;
        
        let pct_x = relative_x / rect.width();
        let pct_y = relative_y / rect.height();
        
        let map_width = sim_config.map_width.to_num::<f32>();
        let map_height = sim_config.map_height.to_num::<f32>();
        let map_x = pct_x * map_width - map_width / 2.0;
        let map_z = pct_y * map_height - map_height / 2.0;
        
        for mut cam_transform in q_camera.iter_mut() {
            // Simple move. Ideally we'd account for camera angle offset.
            // Assuming camera is looking somewhat down-forward.
            // We just move the camera rig to the target X/Z.
            // But wait, if we move the camera to X/Z, and it's angled, it will look at X/Z + offset.
            // That's fine for now.
            cam_transform.translation.x = map_x;
            cam_transform.translation.z = map_z + 50.0; // Offset to see the point
        }
    }
}

fn cleanup_hud(
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
    mut input_mode: ResMut<InputMode>,
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
                    *input_mode = InputMode::CommandMove;
                }
                CommandAction::Attack => {
                    *input_mode = InputMode::CommandAttack;
                }
            }
        }
    }
}

fn update_selection_hud(
    selected_units: Query<(Entity, Option<&Health>), With<Selected>>,
    mut text_query: Query<&mut Text, With<SelectionText>>,
) {
    let count = selected_units.iter().count();
    for mut text in &mut text_query {
        if count == 0 {
            **text = "No Selection".to_string();
        } else if count == 1 {
            if let Ok((entity, health)) = selected_units.single() {
                let health_str = if let Some(h) = health {
                    format!("HP: {:.0}/{:.0}", h.current, h.max)
                } else {
                    "HP: N/A".to_string()
                };
                **text = format!("Unit ID: {:?}\n{}", entity, health_str);
            }
        } else {
            **text = format!("Selected: {} units", count);
        }
    }
}
