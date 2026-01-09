use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use crate::game::unit::Selected;
use crate::game::simulation::SimPosition;
use crate::game::simulation::SimConfig;
use crate::game::camera::RtsCamera;
use super::components::*;

/// Update minimap dots to reflect unit positions and camera frame
pub fn minimap_system(
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

/// Handle minimap click to move camera
pub fn minimap_input_system(
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
