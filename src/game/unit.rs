use bevy::prelude::*;
use crate::game::simulation::{SimPosition, SimPositionPrev, SimVelocity, SimSet, Colliding, SimConfig, follow_path};
use crate::game::math::{FixedVec2, FixedNum};
use crate::game::GameState;

use crate::game::config::{GameConfig, GameConfigHandle};

#[derive(Component)]
pub struct Unit;

#[derive(Component)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

#[derive(Component)]
pub struct Selected;

#[derive(Component)]
pub struct SelectionCircle;

#[derive(Component)]
pub struct HealthBar;

#[derive(Resource, Default)]
pub struct HealthBarSettings {
    pub show: bool,
}

#[derive(Resource)]
pub struct UnitMesh {
    pub unit: Handle<Mesh>,
    pub circle: Handle<Mesh>,
    pub quad: Handle<Mesh>,
}

#[derive(Resource)]
pub struct UnitMaterials {
    pub normal: Handle<StandardMaterial>,
    pub colliding: Handle<StandardMaterial>,
    pub selection_circle: Handle<StandardMaterial>,
    pub health_bar: Handle<StandardMaterial>,
}

use crate::game::camera::RtsCamera;

pub struct UnitPlugin;

impl Plugin for UnitPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HealthBarSettings>()
           .add_systems(Startup, (setup_unit_resources).chain())
           // unit_movement_logic is replaced by follow_flow_field in simulation.rs
           .add_systems(FixedUpdate, (apply_boids_steering).chain().in_set(SimSet::Steering).after(follow_path))
           .add_systems(Update, (spawn_unit_visuals, update_selection_visuals, update_selection_circle_visibility, update_health_bars, toggle_health_bars, sync_visuals, update_unit_lod).run_if(in_state(GameState::InGame).or(in_state(GameState::Editor))));
    }
}

fn toggle_health_bars(
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

fn update_health_bars(
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

fn update_selection_circle_visibility(
    q_added: Query<&Children, (With<Unit>, Added<Selected>)>,
    q_children_lookup: Query<&Children>,
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
        if let Ok(children) = q_children_lookup.get(entity) {
            for child in children.iter() {
                if let Ok(mut vis) = q_vis.get_mut(child) {
                    *vis = Visibility::Hidden;
                }
            }
        }
    }
}

fn update_unit_lod(
    mut query: Query<(&mut Visibility, &Transform), With<Unit>>,
    q_camera: Query<(&GlobalTransform, &Camera), With<RtsCamera>>,
    mut gizmos: Gizmos,
) {
    let Ok((camera_transform, _camera)) = q_camera.single() else { return };
    let camera_pos = camera_transform.translation();
    
    // Simple LOD: If camera is high up, hide mesh and draw simple gizmo
    let lod_height_threshold = 50.0;
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

fn sync_visuals(
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

fn update_selection_visuals(
    mut query: Query<(Option<&Colliding>, &mut MeshMaterial3d<StandardMaterial>), With<Unit>>,
    unit_materials: Res<UnitMaterials>,
) {
    for (colliding, mut mat_handle) in query.iter_mut() {
        let target_mat = if colliding.is_some() {
            &unit_materials.colliding
        } else {
            &unit_materials.normal
        };

        if mat_handle.0 != *target_mat {
            mat_handle.0 = target_mat.clone();
        }
    }
}

fn setup_unit_resources(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Capsule3d::default());
    let circle_mesh = meshes.add(Annulus::new(0.6, 0.7)); // Inner radius 0.6, Outer 0.7
    let quad_mesh = meshes.add(Rectangle::new(1.0, 0.15));

    commands.insert_resource(UnitMesh {
        unit: mesh,
        circle: circle_mesh,
        quad: quad_mesh,
    });

    let normal_mat = materials.add(Color::srgb(0.8, 0.7, 0.6));
    let colliding_mat = materials.add(Color::srgb(0.8, 0.2, 0.2));
    let circle_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 1.0, 0.2),
        unlit: true,
        ..default()
    });
    let health_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.0, 1.0, 0.0),
        unlit: true,
        cull_mode: None, // Double sided
        ..default()
    });

    commands.insert_resource(UnitMaterials {
        normal: normal_mat,
        colliding: colliding_mat,
        selection_circle: circle_mat,
        health_bar: health_mat,
    });
}

fn spawn_unit_visuals(
    mut commands: Commands,
    query: Query<(Entity, &SimPosition), Added<Unit>>,
    unit_mesh: Res<UnitMesh>,
    unit_materials: Res<UnitMaterials>,
    settings: Res<HealthBarSettings>,
) {
    for (entity, pos) in query.iter() {
        let p = pos.0.to_vec2();
        commands.entity(entity).insert((
            Mesh3d(unit_mesh.unit.clone()),
            MeshMaterial3d(unit_materials.normal.clone()),
            Transform::from_xyz(p.x, 1.0, p.y),
        )).with_children(|parent| {
            parent.spawn((
                Mesh3d(unit_mesh.circle.clone()),
                MeshMaterial3d(unit_materials.selection_circle.clone()),
                Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
                    .with_translation(Vec3::new(0.0, -0.95, 0.0)),
                Visibility::Hidden,
                SelectionCircle,
            ));
            // Health Bar
            parent.spawn((
                Mesh3d(unit_mesh.quad.clone()),
                MeshMaterial3d(unit_materials.health_bar.clone()),
                Transform::from_xyz(0.0, 1.5, 0.0),
                if settings.show { Visibility::Visible } else { Visibility::Hidden },
                HealthBar,
            ));
        });
    }
}

fn apply_boids_steering(
    mut query: Query<(Entity, &SimPosition, &mut SimVelocity), With<Unit>>,
    sim_config: Res<SimConfig>,
) {
    let separation_weight = sim_config.separation_weight;
    let alignment_weight = sim_config.alignment_weight;
    let cohesion_weight = sim_config.cohesion_weight;
    let neighbor_radius = sim_config.neighbor_radius;
    let separation_radius = sim_config.separation_radius;
    let max_speed = sim_config.unit_speed;

    let neighbor_radius_sq = neighbor_radius * neighbor_radius;
    let separation_radius_sq = separation_radius * separation_radius;

    // Collect data to avoid borrowing issues
    let units: Vec<(Entity, FixedVec2, FixedVec2)> = query.iter().map(|(e, p, v)| (e, p.0, v.0)).collect();
    let mut steering_forces = Vec::with_capacity(units.len());

    for (entity, pos, vel) in &units {
        let mut separation = FixedVec2::ZERO;
        let mut alignment = FixedVec2::ZERO;
        let mut cohesion = FixedVec2::ZERO;
        
        let mut neighbor_count = 0;
        let mut separation_count = 0;
        let mut center_of_mass = FixedVec2::ZERO;

        for (other_entity, other_pos, other_vel) in &units {
            if entity == other_entity { continue; }

            let dist_sq = (*pos - *other_pos).length_squared();

            if dist_sq < neighbor_radius_sq {
                // Alignment
                alignment = alignment + *other_vel;
                
                // Cohesion
                center_of_mass = center_of_mass + *other_pos;
                
                neighbor_count += 1;

                // Separation
                if dist_sq < separation_radius_sq {
                    let dist = dist_sq.sqrt();
                    let strength = if dist > FixedNum::from_num(0.001) { FixedNum::from_num(1.0) / dist } else { FixedNum::from_num(100.0) }; 
                    let dir = *pos - *other_pos;
                    // normalize_or_zero is not on FixedVec2, need to implement or check
                    let dir_norm = if dir.length_squared() > FixedNum::ZERO { dir.normalize() } else { FixedVec2::ZERO };
                    separation = separation + dir_norm * strength;
                    separation_count += 1;
                }
            }
        }

        if neighbor_count > 0 {
            let nc = FixedNum::from_num(neighbor_count);
            let align_norm = alignment / nc ;
            let align_norm = if align_norm.length_squared() > FixedNum::ZERO { align_norm.normalize() } else { FixedVec2::ZERO };
            alignment = align_norm * max_speed;
            alignment = alignment - *vel;
            
            center_of_mass = center_of_mass / nc;
            let direction_to_com = center_of_mass - *pos;
            let cohesion_norm = if direction_to_com.length_squared() > FixedNum::ZERO { direction_to_com.normalize() } else { FixedVec2::ZERO };
            cohesion = cohesion_norm * max_speed;
            cohesion = cohesion - *vel;
        }

        if separation_count > 0 {
             let sc = FixedNum::from_num(separation_count);
             let sep_norm = separation / sc ;
             let sep_norm = if sep_norm.length_squared() > FixedNum::ZERO { sep_norm.normalize() } else { FixedVec2::ZERO };
             separation = sep_norm * max_speed;
             separation = separation - *vel;
        }

        let total_force = (separation * separation_weight) + 
                          (alignment * alignment_weight) + 
                          (cohesion * cohesion_weight);
        
        steering_forces.push((*entity, total_force));
    }

    // Apply forces
    let delta = FixedNum::from_num(1.0) / FixedNum::from_num(sim_config.tick_rate);
    for (entity, force) in steering_forces {
        if let Ok((_, _, mut vel)) = query.get_mut(entity) {
            vel.0 = vel.0 + force * delta;
            
            if vel.0.length_squared() > max_speed * max_speed {
                vel.0 = vel.0.normalize() * max_speed;
            }
        }
    }
}
