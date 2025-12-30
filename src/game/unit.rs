use bevy::prelude::*;
use crate::game::simulation::{SimPosition, SimPositionPrev, SimVelocity, SimTarget, SimSet};
use crate::game::config::{GameConfig, GameConfigHandle};

#[derive(Component)]
pub struct Unit;

#[derive(Component)]
pub struct Selected;

#[derive(Resource)]
pub struct UnitMaterials {
    pub normal: Handle<StandardMaterial>,
    pub selected: Handle<StandardMaterial>,
}

pub struct UnitPlugin;

impl Plugin for UnitPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_test_unit)
           .add_systems(FixedUpdate, unit_movement_logic.in_set(SimSet::Steering))
           .add_systems(Update, (update_selection_visuals, sync_visuals));
    }
}

fn unit_movement_logic(
    mut commands: Commands,
    mut query: Query<(Entity, &SimPosition, &mut SimVelocity, &SimTarget)>,
    time: Res<Time>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
) {
    let speed = game_configs.get(&config_handle.0).map(|c| c.unit_speed).unwrap_or(5.0);
    let delta = time.delta_secs();
    
    // info!("Unit Logic: delta={}, speed={}", delta, speed);

    for (entity, pos, mut vel, target) in query.iter_mut() {
        let direction = target.0 - pos.0;
        let distance = direction.length();
        
        if distance < 0.01 {
            vel.0 = Vec2::ZERO;
            commands.entity(entity).remove::<SimTarget>();
        } else if distance <= speed * delta {
            // Arrive in this tick
            if delta > 0.0 {
                vel.0 = direction / delta;
            } else {
                vel.0 = Vec2::ZERO;
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
        let interpolated = prev_pos.0.lerp(pos.0, alpha);
        transform.translation.x = interpolated.x;
        transform.translation.z = interpolated.y;
    }
}

fn update_selection_visuals(
    mut query: Query<(Option<&Selected>, &mut MeshMaterial3d<StandardMaterial>), With<Unit>>,
    unit_materials: Res<UnitMaterials>,
) {
    for (selected, mut mat_handle) in query.iter_mut() {
        let target_mat = if selected.is_some() {
            &unit_materials.selected
        } else {
            &unit_materials.normal
        };

        if mat_handle.0 != *target_mat {
            mat_handle.0 = target_mat.clone();
        }
    }
}

fn spawn_test_unit(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Capsule3d::default());
    let normal_mat = materials.add(Color::srgb(0.8, 0.7, 0.6));
    let selected_mat = materials.add(Color::srgb(0.2, 0.8, 0.2));

    commands.insert_resource(UnitMaterials {
        normal: normal_mat.clone(),
        selected: selected_mat.clone(),
    });

    for x in -2..3 {
        for z in -2..3 {
            let pos_x = x as f32 * 2.0;
            let pos_z = z as f32 * 2.0;
            commands.spawn((
                Unit,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(normal_mat.clone()),
                Transform::from_xyz(pos_x, 1.0, pos_z),
                SimPosition(Vec2::new(pos_x, pos_z)),
                SimPositionPrev(Vec2::new(pos_x, pos_z)),
                SimVelocity(Vec2::ZERO),
            ));
        }
    }
}
