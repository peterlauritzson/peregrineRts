use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use crate::game::unit::{Unit, Selected};
use crate::game::simulation::SimTarget;
use crate::game::camera::RtsCamera;

pub struct ControlPlugin;

impl Plugin for ControlPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DragState>()
           .add_systems(Startup, setup_selection_box)
           .add_systems(Update, handle_input);
    }
}

#[derive(Resource, Default)]
struct DragState {
    start: Option<Vec2>,
    current: Option<Vec2>,
}

#[derive(Component)]
struct SelectionBox;

fn setup_selection_box(mut commands: Commands) {
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

fn handle_input(
    mut commands: Commands,
    mouse_button: Res<ButtonInput<MouseButton>>,
    q_window: Query<&Window, With<PrimaryWindow>>,
    q_camera: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
    q_units: Query<(Entity, &GlobalTransform), With<Unit>>,
    q_selected: Query<Entity, With<Selected>>,
    mut drag_state: ResMut<DragState>,
    mut q_selection_box: Query<(&mut Node, &mut Visibility), With<SelectionBox>>,
) {
    let Some((camera, camera_transform)) = q_camera.iter().next() else { return };
    let Some(window) = q_window.iter().next() else { return };
    let Some(cursor_position) = window.cursor_position() else { return };

    // Left Click: Selection Logic
    if mouse_button.just_pressed(MouseButton::Left) {
        drag_state.start = Some(cursor_position);
        drag_state.current = Some(cursor_position);
        
        // Show selection box
        if let Ok((_, mut visibility)) = q_selection_box.single_mut() {
            *visibility = Visibility::Visible;
        }
    }

    if mouse_button.pressed(MouseButton::Left) {
        if let Some(start) = drag_state.start {
            drag_state.current = Some(cursor_position);

            // Update Selection Box UI
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

            // Hide selection box
            if let Ok((_, mut visibility)) = q_selection_box.single_mut() {
                *visibility = Visibility::Hidden;
            }

            // Calculate Selection
            let min = start.min(end);
            let max = start.max(end);
            let size = max - min;

            // If drag is small, treat as click
            let is_click = size.length() < 5.0;

            // Deselect all first (unless Shift is held - TODO)
            for (entity, _) in q_units.iter() {
                commands.entity(entity).remove::<Selected>();
            }

            if is_click {
                // Raycast for single click selection
                let Ok(ray) = camera.viewport_to_world(camera_transform, end) else { return };
                
                let mut closest_hit: Option<(Entity, f32)> = None;
                for (entity, unit_transform) in q_units.iter() {
                    let unit_pos = unit_transform.translation();
                    // info!("Checking unit at {:?}", unit_pos);
                    let vector_to_unit = unit_pos - ray.origin;
                    let projection = vector_to_unit.dot(ray.direction.into());
                    if projection < 0.0 { continue; }
                    let closest_point = ray.origin + ray.direction * projection;
                    let distance_sq = closest_point.distance_squared(unit_pos);

                    if distance_sq < 1.0 { // Radius 1.0
                        if closest_hit.is_none() || projection < closest_hit.unwrap().1 {
                            closest_hit = Some((entity, projection));
                        }
                    }
                }
                if let Some((hit_entity, _)) = closest_hit {
                    commands.entity(hit_entity).insert(Selected);
                }
            } else {
                // Box Selection
                for (entity, unit_transform) in q_units.iter() {
                    let unit_pos = unit_transform.translation();
                    // Project unit position to screen space
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

    // Right Click: Movement
    if mouse_button.just_pressed(MouseButton::Right) {
        let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) else { return };
        
        // Intersect with ground plane (y=0)
        let normal = Vec3::Y;
        let denom = ray.direction.dot(normal);

        if denom.abs() > 0.0001 {
            let t = -ray.origin.y / denom;
            if t >= 0.0 {
                let intersection_point = ray.origin + ray.direction * t;
                
                // Command all selected units to move
                for entity in q_selected.iter() {
                    commands.entity(entity).insert(SimTarget(intersection_point.xz()));
                }
            }
        }
    }
}
