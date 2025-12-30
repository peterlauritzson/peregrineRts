use bevy::prelude::*;

#[derive(Component)]
pub struct Unit;

#[derive(Component)]
pub struct Selected;

#[derive(Component)]
pub struct MoveTarget(pub Vec3);

#[derive(Resource)]
pub struct UnitMaterials {
    pub normal: Handle<StandardMaterial>,
    pub selected: Handle<StandardMaterial>,
}

pub struct UnitPlugin;

impl Plugin for UnitPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_test_unit)
           .add_systems(Update, (update_selection_visuals, move_units));
    }
}

fn move_units(
    mut commands: Commands,
    mut query: Query<(Entity, &mut Transform, &MoveTarget)>,
    time: Res<Time>,
) {
    let speed = 5.0;
    for (entity, mut transform, target) in query.iter_mut() {
        let mut target_pos = target.0;
        target_pos.y = transform.translation.y;

        let direction = target_pos - transform.translation;
        let distance = direction.length();

        if distance < 0.1 {
            commands.entity(entity).remove::<MoveTarget>();
        } else {
            let move_dir = direction.normalize();
            transform.translation += move_dir * speed * time.delta_secs();
            
            // Optional: Rotate to face target
            // transform.look_at(target.0, Vec3::Y); 
            // Note: look_at might be unstable if target is too close or directly up/down.
        }
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
            commands.spawn((
                Unit,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(normal_mat.clone()),
                Transform::from_xyz(x as f32 * 2.0, 1.0, z as f32 * 2.0),
            ));
        }
    }
}
