use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use crate::game::unit::Selected;
use crate::game::simulation::UnitMoveCommand;
use crate::game::math::FixedVec2;
use crate::game::camera::RtsCamera;
use super::resources::*;
use super::selection::*;
use crate::game::unit::Unit;
use crate::game::config::{GameConfig, GameConfigHandle};

/// Main input handler - routes to appropriate handler based on input mode
pub fn handle_input(
    mut commands: Commands,
    mouse_button: Res<ButtonInput<MouseButton>>,
    q_window: Query<&Window, With<PrimaryWindow>>,
    q_camera: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
    q_units: Query<(Entity, &GlobalTransform), With<Unit>>,
    q_selected: Query<Entity, With<Selected>>,
    mut drag_state: ResMut<DragState>,
    mut q_selection_box: Query<(&mut Node, &mut Visibility), With<SelectionBox>>,
    mut move_events: MessageWriter<UnitMoveCommand>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    mut input_mode: ResMut<InputMode>,
) {
    let Some((camera, camera_transform)) = q_camera.iter().next() else { return };
    let Some(window) = q_window.iter().next() else { return };
    let Some(cursor_position) = window.cursor_position() else { return };
    let Some(config) = game_configs.get(&config_handle.0) else { return };

    match *input_mode {
        InputMode::Selection => {
            handle_selection(
                &mut commands,
                &mouse_button,
                cursor_position,
                camera,
                camera_transform,
                &q_units,
                &mut drag_state,
                &mut q_selection_box,
                config,
            );

            // Right Click: Movement (Smart Command)
            if mouse_button.just_pressed(MouseButton::Right) {
                issue_move_command(
                    cursor_position,
                    camera,
                    camera_transform,
                    &q_selected,
                    &mut move_events,
                );
            }
        }
        InputMode::CommandMove => {
            if mouse_button.just_pressed(MouseButton::Left) {
                issue_move_command(
                    cursor_position,
                    camera,
                    camera_transform,
                    &q_selected,
                    &mut move_events,
                );
                *input_mode = InputMode::Selection;
            } else if mouse_button.just_pressed(MouseButton::Right) {
                *input_mode = InputMode::Selection;
            }
        }
        InputMode::CommandAttack => {
             if mouse_button.just_pressed(MouseButton::Left) {
                info!("Attack command issued (placeholder)");
                *input_mode = InputMode::Selection;
            } else if mouse_button.just_pressed(MouseButton::Right) {
                *input_mode = InputMode::Selection;
            }
        }
    }
}

/// Issue a move command to selected units
fn issue_move_command(
    cursor_position: Vec2,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    q_selected: &Query<Entity, With<Selected>>,
    move_events: &mut MessageWriter<UnitMoveCommand>,
) {
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) else { return };
    let normal = Vec3::Y;
    let denom = ray.direction.dot(normal);

    if denom.abs() > 0.0001 {
        let t = -ray.origin.y / denom;
        if t >= 0.0 {
            let intersection_point = ray.origin + ray.direction * t;
            for entity in q_selected.iter() {
                move_events.write(UnitMoveCommand {
                    player_id: 0,
                    entity,
                    target: FixedVec2::from_f32(intersection_point.x, intersection_point.z),
                });
            }
        }
    }
}
