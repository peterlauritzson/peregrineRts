use bevy::prelude::*;

/// Settings for health bar display
#[derive(Resource, Default)]
pub struct HealthBarSettings {
    pub show: bool,
}

/// Shared mesh handles for unit rendering
#[derive(Resource)]
pub struct UnitMesh {
    pub unit: Handle<Mesh>,
    pub circle: Handle<Mesh>,
    pub quad: Handle<Mesh>,
}

/// Shared material handles for unit rendering
#[derive(Resource)]
pub struct UnitMaterials {
    pub normal: Handle<StandardMaterial>,
    pub colliding: Handle<StandardMaterial>,
    pub selection_circle: Handle<StandardMaterial>,
    pub health_bar: Handle<StandardMaterial>,
}

/// Sets up shared unit rendering resources (meshes and materials)
pub(super) fn setup_unit_resources(
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
