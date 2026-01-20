use bevy::prelude::*;
use crate::game::camera::RtsCamera;
use crate::game::config::{GameConfig, GameConfigHandle};
use crate::game::simulation::{SimPosition, SimPositionPrev, CollisionState};

use super::components::{Unit, Selected, SelectionCircle, HealthBar, Health};
use super::resources::{UnitMesh, UnitMaterials, HealthBarSettings};

/// Spawns visual representations for newly created units
/// 
/// Note: Only runs on Added<Unit> - NOT a hot path (only processes new spawns)
pub(super) fn spawn_unit_visuals(
    mut commands: Commands,
    query: Query<(Entity, &SimPosition), Added<Unit>>,
    unit_mesh: Res<UnitMesh>,
    unit_materials: Res<UnitMaterials>,
    settings: Res<HealthBarSettings>,
) {
    for (entity, pos) in query.iter() {
        let p = pos.0.to_vec2();
        commands.entity(entity).insert((
            // NOLINT: Handle::clone() is cheap (Arc-based ref count)
            Mesh3d(unit_mesh.unit.clone()),
            // NOLINT: Handle::clone() is cheap (Arc-based ref count)
            MeshMaterial3d(unit_materials.normal.clone()),
            Transform::from_xyz(p.x, 1.0, p.y),
        )).with_children(|parent| {
            parent.spawn((
                // NOLINT: Handle::clone() is cheap (Arc-based ref count)
                Mesh3d(unit_mesh.circle.clone()),
                // NOLINT: Handle::clone() is cheap (Arc-based ref count)
                MeshMaterial3d(unit_materials.selection_circle.clone()),
                Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
                    .with_translation(Vec3::new(0.0, -0.95, 0.0)),
                Visibility::Hidden,
                SelectionCircle,
            ));
            // Health Bar
            parent.spawn((
                // NOLINT: Handle::clone() is cheap (Arc-based ref count)
                Mesh3d(unit_mesh.quad.clone()),
                // NOLINT: Handle::clone() is cheap (Arc-based ref count)
                MeshMaterial3d(unit_materials.health_bar.clone()),
                Transform::from_xyz(0.0, 1.5, 0.0),
                if settings.show { Visibility::Visible } else { Visibility::Hidden },
                HealthBar,
            ));
        });
    }
}

/// Synchronizes visual transforms with simulation positions (with interpolation)
pub(super) fn sync_visuals(
    mut query: Query<(&mut Transform, &SimPosition, &SimPositionPrev)>,
    fixed_time: Res<Time<Fixed>>,
) {
    let alpha = fixed_time.overstep_fraction();
    for (mut transform, pos, prev_pos) in query.iter_mut() {
        let prev = prev_pos.0.to_vec2();
        let curr = pos.0.to_vec2();
        let interpolated = prev.lerp(curr, alpha);
        transform.translation.x = interpolated.x;
        transform.translation.z = interpolated.y;
    }
}

/// Updates unit materials based on collision state
/// 
/// Optimized: Only processes entities whose collision state CHANGED (not all units every frame)
pub(super) fn update_selection_visuals(
    mut query: Query<(&mut MeshMaterial3d<StandardMaterial>, &CollisionState), (With<Unit>, Changed<CollisionState>)>,
    unit_materials: Res<UnitMaterials>,
) {
    // Only processes entities where CollisionState changed (leverages Bevy's change detection)
    for (mut mat_handle, collision_state) in query.iter_mut() {
        // NOLINT: Handle::clone() is cheap (Arc-based ref count)
        if collision_state.is_colliding {
            mat_handle.0 = unit_materials.colliding.clone();
        } else {
            mat_handle.0 = unit_materials.normal.clone();
        }
    }
}

/// Shows/hides selection circles based on Selected component
pub(super) fn update_selection_circle_visibility(
    q_added: Query<&Children, (With<Unit>, Added<Selected>)>,
    q_children_lookup: Query<&Children>,
    q_selected: Query<Entity, With<Selected>>,
    mut q_vis: Query<&mut Visibility, With<SelectionCircle>>,
    mut removed_selected: RemovedComponents<Selected>,
) {
    // Handle Added Selected
    for children in q_added.iter() {
        for child in children.iter() {
            if let Ok(mut vis) = q_vis.get_mut(child) {
                *vis = Visibility::Visible;
            }
        }
    }

    // Handle Removed Selected
    for entity in removed_selected.read() {
        if q_selected.contains(entity) {
            continue;
        }
        if let Ok(children) = q_children_lookup.get(entity) {
            for child in children.iter() {
                if let Ok(mut vis) = q_vis.get_mut(child) {
                    *vis = Visibility::Hidden;
                }
            }
        }
    }
}

/// Implements level-of-detail for units based on camera distance
pub(super) fn update_unit_lod(
    mut query: Query<(&mut Visibility, &Transform), With<Unit>>,
    q_camera: Query<(&GlobalTransform, &Camera), With<RtsCamera>>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    mut gizmos: Gizmos,
) {
    let Some(config) = game_configs.get(&config_handle.0) else { return };
    let Ok((camera_transform, _camera)) = q_camera.single() else { return };
    let camera_pos = camera_transform.translation();
    
    // Simple LOD: If camera is high up, hide mesh and draw simple gizmo
    let lod_height_threshold = config.debug_unit_lod_height_threshold;
    let use_lod = camera_pos.y > lod_height_threshold;

    // Also cull if far away from center of view?
    // Bevy does frustum culling for meshes, but we can help by disabling visibility if we want to draw icons instead.

    for (mut visibility, transform) in query.iter_mut() {
        if use_lod {
            *visibility = Visibility::Hidden;
            // Draw simple icon (circle)
            gizmos.circle(
                Isometry3d::new(
                    transform.translation,
                    Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                ),
                0.5,
                Color::srgb(0.8, 0.7, 0.6),
            );
        } else {
            *visibility = Visibility::Visible;
        }
    }
}

/// Toggles health bar visibility when user presses configured key
pub(super) fn toggle_health_bars(
    keys: Res<ButtonInput<KeyCode>>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    mut settings: ResMut<HealthBarSettings>,
    mut q_bars: Query<&mut Visibility, With<HealthBar>>,
) {
    let Some(config) = game_configs.get(&config_handle.0) else { return };

    if keys.just_pressed(config.key_toggle_health_bars) {
        settings.show = !settings.show;
        let vis = if settings.show { Visibility::Visible } else { Visibility::Hidden };
        for mut visibility in q_bars.iter_mut() {
            *visibility = vis;
        }
    }
}

/// Updates health bar visuals based on current health
pub(super) fn update_health_bars(
    q_units: Query<(&Children, &Health), Changed<Health>>,
    mut q_bars: Query<&mut Transform, With<HealthBar>>,
) {
    for (children, health) in q_units.iter() {
        let pct = (health.current / health.max).clamp(0.0, 1.0);
        for child in children.iter() {
            if let Ok(mut transform) = q_bars.get_mut(child) {
                transform.scale.x = pct;
                // Center is 0.0. Width is 1.0.
                // If scale is 1.0, left is -0.5, right is 0.5.
                // If scale is 0.5, left is -0.25, right is 0.25.
                // We want left to stay at -0.5.
                // New center = -0.5 + (width * scale / 2.0) = -0.5 + (1.0 * pct / 2.0)
                transform.translation.x = -0.5 + (pct * 0.5);
            }
        }
    }
}
