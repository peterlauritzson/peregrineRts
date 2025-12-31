use bevy::prelude::*;
use crate::game::simulation::{SimPosition, SimPositionPrev, SimVelocity, SimTarget, SimSet, Colliding, SimConfig, SpawnUnitCommand};
use crate::game::math::{FixedVec2, FixedNum};

#[derive(Component)]
pub struct Unit;

#[derive(Component)]
pub struct Selected;

#[derive(Resource)]
pub struct UnitMesh(pub Handle<Mesh>);

#[derive(Resource)]
pub struct UnitMaterials {
    pub normal: Handle<StandardMaterial>,
    pub selected: Handle<StandardMaterial>,
    pub colliding: Handle<StandardMaterial>,
}

pub struct UnitPlugin;

impl Plugin for UnitPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (setup_unit_resources, spawn_test_unit).chain())
           .add_systems(FixedUpdate, (unit_movement_logic, apply_boids_steering).chain().in_set(SimSet::Steering))
           .add_systems(Update, (spawn_unit_visuals, update_selection_visuals, sync_visuals));
    }
}

pub fn unit_movement_logic(
    mut commands: Commands,
    mut query: Query<(Entity, &SimPosition, &mut SimVelocity, &SimTarget)>,
    sim_config: Res<SimConfig>,
) {
    let speed = sim_config.unit_speed;
    let delta = FixedNum::from_num(1.0) / FixedNum::from_num(sim_config.tick_rate);
    let arrival_threshold = sim_config.arrival_threshold;

    for (entity, pos, mut vel, target) in query.iter_mut() {
        let direction = target.0 - pos.0;
        let distance = direction.length();
        
        if distance < arrival_threshold {
            vel.0 = FixedVec2::ZERO;
            commands.entity(entity).remove::<SimTarget>();
        } else if distance <= speed * delta {
            // Arrive in this tick
            if delta > FixedNum::ZERO {
                vel.0 = direction / delta;
            } else {
                vel.0 = FixedVec2::ZERO;
            }
        } else {
            vel.0 = direction.normalize() * speed;
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
    mut query: Query<(Option<&Selected>, Option<&Colliding>, &mut MeshMaterial3d<StandardMaterial>), With<Unit>>,
    unit_materials: Res<UnitMaterials>,
) {
    for (selected, colliding, mut mat_handle) in query.iter_mut() {
        let target_mat = if colliding.is_some() {
            &unit_materials.colliding
        } else if selected.is_some() {
            &unit_materials.selected
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
    commands.insert_resource(UnitMesh(mesh));

    let normal_mat = materials.add(Color::srgb(0.8, 0.7, 0.6));
    let selected_mat = materials.add(Color::srgb(0.2, 0.8, 0.2));
    let colliding_mat = materials.add(Color::srgb(0.8, 0.2, 0.2));

    commands.insert_resource(UnitMaterials {
        normal: normal_mat,
        selected: selected_mat,
        colliding: colliding_mat,
    });
}

fn spawn_test_unit(
    mut spawn_events: MessageWriter<SpawnUnitCommand>,
) {
    for x in -2..3 {
        for z in -2..3 {
            let pos_x = FixedNum::from_num(x) * FixedNum::from_num(2.0);
            let pos_z = FixedNum::from_num(z) * FixedNum::from_num(2.0);
            
            spawn_events.write(SpawnUnitCommand {
                player_id: 0,
                position: FixedVec2::new(pos_x, pos_z),
            });
        }
    }
}

fn spawn_unit_visuals(
    mut commands: Commands,
    query: Query<(Entity, &SimPosition), Added<Unit>>,
    unit_mesh: Res<UnitMesh>,
    unit_materials: Res<UnitMaterials>,
) {
    for (entity, pos) in query.iter() {
        let p = pos.0.to_vec2();
        commands.entity(entity).insert((
            Mesh3d(unit_mesh.0.clone()),
            MeshMaterial3d(unit_materials.normal.clone()),
            Transform::from_xyz(p.x, 1.0, p.y),
        ));
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
