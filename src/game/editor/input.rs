use bevy::prelude::*;
use crate::game::camera::RtsCamera;
use crate::game::fixed_math::{FixedVec2, FixedNum};
use crate::game::simulation::{StaticObstacle, SimPosition, Collider, layers};
use super::components::*;
use super::ui::spawn_generation_dialog;

/// Handles keyboard input for typing in input fields
pub fn keyboard_input_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut editor_state: ResMut<EditorState>,
    mut active_field: ResMut<ActiveInputField>,
    mut commands: Commands,
    dialog_root_query: Query<Entity, With<GenerationDialogRoot>>,
) {
    let Some(field_type) = active_field.field else { return };
    
    // Helper to get mutable reference to the appropriate string field
    let input_str = match field_type {
        InputFieldType::MapWidth => &mut editor_state.input_map_width,
        InputFieldType::MapHeight => &mut editor_state.input_map_height,
        InputFieldType::NumObstacles => &mut editor_state.input_num_obstacles,
        InputFieldType::ObstacleSize => &mut editor_state.input_obstacle_size,
    };
    
    let mut changed = false;
    
    // Handle backspace
    if keys.just_pressed(KeyCode::Backspace) {
        if !input_str.is_empty() {
            input_str.pop();
            changed = true;
            active_field.first_input = false;
        }
    }
    
    // Handle number keys
    for key in [
        KeyCode::Digit0, KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, 
        KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7,
        KeyCode::Digit8, KeyCode::Digit9,
    ] {
        if keys.just_pressed(key) {
            let digit = match key {
                KeyCode::Digit0 => '0',
                KeyCode::Digit1 => '1',
                KeyCode::Digit2 => '2',
                KeyCode::Digit3 => '3',
                KeyCode::Digit4 => '4',
                KeyCode::Digit5 => '5',
                KeyCode::Digit6 => '6',
                KeyCode::Digit7 => '7',
                KeyCode::Digit8 => '8',
                KeyCode::Digit9 => '9',
                _ => continue,
            };
            
            // Clear on first input, then append
            if active_field.first_input {
                input_str.clear();
                active_field.first_input = false;
            }
            
            if input_str.len() < 5 {  // Max 5 digits
                input_str.push(digit);
                changed = true;
            }
        }
    }
    
    // Handle decimal point for obstacle size
    if field_type == InputFieldType::ObstacleSize && keys.just_pressed(KeyCode::Period) {
        if active_field.first_input {
            input_str.clear();
            active_field.first_input = false;
        }
        if !input_str.contains('.') && !input_str.is_empty() {
            input_str.push('.');
            changed = true;
        }
    }
    
    // Handle Enter or Escape to deselect
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Escape) {
        active_field.field = None;
        changed = true;
    }
    
    // If changed, respawn dialog
    if changed {
        for entity in dialog_root_query.iter() {
            commands.entity(entity).despawn();
        }
        if active_field.field.is_some() {
            spawn_generation_dialog(&mut commands, &editor_state, &active_field);
        }
    }
}

/// Handles clicks on input fields to activate them for typing
pub fn handle_input_field_clicks(
    mut interaction_query: Query<(&Interaction, &InputFieldType), Changed<Interaction>>,
    mut active_field: ResMut<ActiveInputField>,
) {
    for (interaction, field_type) in &mut interaction_query {
        if *interaction == Interaction::Pressed {
            // Toggle: if clicking the same field, deselect it; otherwise select the new field
            if active_field.field == Some(*field_type) {
                active_field.field = None;
                active_field.first_input = false;
            } else {
                active_field.field = Some(*field_type);
                active_field.first_input = true;  // First keypress will clear the field
            }
        }
    }
}

/// Handles mouse input for placing obstacles in the editor
pub fn handle_editor_input(
    mut commands: Commands,
    mouse_button_input: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
    editor_state: Res<EditorState>,
    editor_resources: Res<EditorResources>,
    initial_config: Res<crate::game::config::InitialConfig>,
) {
    if !editor_state.placing_obstacle {
        return;
    }

    if mouse_button_input.just_pressed(MouseButton::Left) {
        let Ok((camera, camera_transform)) = camera_q.single() else { return };
        let Ok(window) = windows.single() else { return };

        if let Some(cursor_position) = window.cursor_position() {
            if let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) {
                // Intersect with plane Y=0
                if ray.direction.y.abs() > 0.0001 {
                    let t = -ray.origin.y / ray.direction.y;
                    if t >= 0.0 {
                        let intersection = ray.origin + ray.direction * t;
                        spawn_obstacle(&mut commands, FixedVec2::new(FixedNum::from_num(intersection.x), FixedNum::from_num(intersection.z)), FixedNum::from_num(initial_config.editor_default_obstacle_radius), &editor_resources);
                    }
                }
            }
        }
    }
}

/// Helper function to spawn an obstacle entity
pub fn spawn_obstacle(commands: &mut Commands, position: FixedVec2, radius: FixedNum, resources: &EditorResources) {
    commands.spawn((
        StaticObstacle,
        SimPosition(position),
        Collider {
            radius,
            layer: layers::OBSTACLE,
            mask: layers::ALL,
        },
        Transform::from_translation(Vec3::new(position.x.to_num(), 1.0, position.y.to_num()))
            .with_scale(Vec3::new(radius.to_num::<f32>(), 1.0, radius.to_num::<f32>())),
        GlobalTransform::default(),
        Mesh3d(resources.obstacle_mesh.clone()),
        MeshMaterial3d(resources.obstacle_material.clone()),
    ));
}
