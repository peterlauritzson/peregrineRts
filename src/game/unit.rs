use bevy::prelude::*;
use crate::game::simulation::{SimPosition, SimPositionPrev, SimVelocity, SimTarget, SimSet, Colliding};
use crate::game::config::{GameConfig, GameConfigHandle};

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
           .add_systems(FixedUpdate, unit_movement_logic.in_set(SimSet::Steering))
           .add_systems(Update, (spawn_unit_visuals, update_selection_visuals, sync_visuals));
    }
}

fn unit_movement_logic(
    mut commands: Commands,
    mut query: Query<(Entity, &SimPosition, &mut SimVelocity, &SimTarget)>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
) {
    let Some(config) = game_configs.get(&config_handle.0) else { return };
    let speed = config.unit_speed;
    let delta = 1.0 / config.tick_rate as f32;
    let arrival_threshold = config.arrival_threshold;

    for (entity, pos, mut vel, target) in query.iter_mut() {
        let direction = target.0 - pos.0;
        let distance = direction.length();
        
        if distance < arrival_threshold {
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
    mut commands: Commands,
) {
    for x in -2..3 {
        for z in -2..3 {
            let pos_x = x as f32 * 2.0;
            let pos_z = z as f32 * 2.0;
            commands.spawn((
                Unit,
                SimPosition(Vec2::new(pos_x, pos_z)),
                SimPositionPrev(Vec2::new(pos_x, pos_z)),
                SimVelocity(Vec2::ZERO),
            ));
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
        commands.entity(entity).insert((
            Mesh3d(unit_mesh.0.clone()),
            MeshMaterial3d(unit_materials.normal.clone()),
            Transform::from_xyz(pos.0.x, 1.0, pos.0.y),
        ));
    }
}
