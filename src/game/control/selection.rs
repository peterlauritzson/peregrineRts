use bevy::prelude::*;
use crate::game::unit::{Unit, Selected};
use crate::game::config::GameConfig;
use super::resources::*;

/// Setup the selection box UI element
pub fn setup_selection_box(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BorderColor::from(Color::WHITE),
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.1)),
        Visibility::Hidden,
        SelectionBox,
    ));
}

/// Handle unit selection via mouse drag or click
pub fn handle_selection(
    commands: &mut Commands,
    mouse_button: &Res<ButtonInput<MouseButton>>,
    cursor_position: Vec2,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    q_units: &Query<(Entity, &GlobalTransform), With<Unit>>,
    drag_state: &mut ResMut<DragState>,
    q_selection_box: &mut Query<(&mut Node, &mut Visibility), With<SelectionBox>>,
    config: &GameConfig,
) {
    // Left Click: Selection Logic
    if mouse_button.just_pressed(MouseButton::Left) {
        drag_state.start = Some(cursor_position);
        drag_state.current = Some(cursor_position);
        
        if let Ok((_, mut visibility)) = q_selection_box.single_mut() {
            *visibility = Visibility::Visible;
        }
    }

    if mouse_button.pressed(MouseButton::Left) {
        if let Some(start) = drag_state.start {
            drag_state.current = Some(cursor_position);

            if let Ok((mut node, _)) = q_selection_box.single_mut() {
                let min = start.min(cursor_position);
                let max = start.max(cursor_position);
                let size = max - min;

                node.left = Val::Px(min.x);
                node.top = Val::Px(min.y);
                node.width = Val::Px(size.x);
                node.height = Val::Px(size.y);
            }
        }
    }

    if mouse_button.just_released(MouseButton::Left) {
        if let Some(start) = drag_state.start {
            let end = cursor_position;
            drag_state.start = None;
            drag_state.current = None;

            if let Ok((_, mut visibility)) = q_selection_box.single_mut() {
                *visibility = Visibility::Hidden;
            }

            let min = start.min(end);
            let max = start.max(end);
            let size = max - min;
            let is_click = size.length() < config.selection_drag_threshold;

            for (entity, _) in q_units.iter() {
                commands.entity(entity).remove::<Selected>();
            }

            if is_click {
                let Ok(ray) = camera.viewport_to_world(camera_transform, end) else { return };
                let mut closest_hit: Option<(Entity, f32)> = None;
                for (entity, unit_transform) in q_units.iter() {
                    let unit_pos = unit_transform.translation();
                    let vector_to_unit = unit_pos - ray.origin;
                    let projection = vector_to_unit.dot(ray.direction.into());
                    if projection < 0.0 { continue; }
                    let closest_point = ray.origin + ray.direction * projection;
                    let distance_sq = closest_point.distance_squared(unit_pos);

                    if distance_sq < config.selection_click_radius * config.selection_click_radius {
                        if closest_hit.is_none() || projection < closest_hit.unwrap().1 {
                            closest_hit = Some((entity, projection));
                        }
                    }
                }
                if let Some((hit_entity, _)) = closest_hit {
                    commands.entity(hit_entity).insert(Selected);
                }
            } else {
                for (entity, unit_transform) in q_units.iter() {
                    let unit_pos = unit_transform.translation();
                    if let Ok(screen_pos) = camera.world_to_viewport(camera_transform, unit_pos) {
                        if screen_pos.x >= min.x && screen_pos.x <= max.x &&
                           screen_pos.y >= min.y && screen_pos.y <= max.y {
                            commands.entity(entity).insert(Selected);
                        }
                    }
                }
            }
        }
    }
}
