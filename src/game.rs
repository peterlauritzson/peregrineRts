use bevy::prelude::*;

mod camera;
mod unit;
mod control;
pub mod simulation;
pub mod config;
pub mod math;
pub mod flow_field;
pub mod spatial_hash;
pub mod pathfinding;

use camera::RtsCameraPlugin;
use unit::UnitPlugin;
use control::ControlPlugin;
use simulation::{SimulationPlugin, SimPosition, StaticObstacle, Collider, layers};
use config::GameConfigPlugin;
use pathfinding::PathfindingPlugin;
use math::{FixedVec2, FixedNum};

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            RtsCameraPlugin,
            UnitPlugin,
            ControlPlugin,
            SimulationPlugin,
            GameConfigPlugin,
            PathfindingPlugin,
        ))
        .add_systems(Startup, setup_game);
    }
}

fn setup_game(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    info!("Game setup started");

    // Ground Plane
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(50.0, 50.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.3, 0.5, 0.3))),
    ));

    // Light
    commands.spawn((
        PointLight {
            shadows_enabled: true,
            intensity: 10_000_000.0,
            range: 100.0,
            ..default()
        },
        Transform::from_xyz(8.0, 16.0, 8.0),
    ));

    // UI Camera
    commands.spawn((
        Camera2d::default(),
        Camera {
            order: 1,
            ..default()
        },
    ));

    // Obstacle
    let obstacle_pos = Vec2::new(5.0, 5.0);
    let obstacle_radius = 2.0;
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(obstacle_radius, 2.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.5, 0.5, 0.5))),
        Transform::from_xyz(obstacle_pos.x, 1.0, obstacle_pos.y),
        SimPosition(FixedVec2::from_f32(obstacle_pos.x, obstacle_pos.y)),
        StaticObstacle { radius: FixedNum::from_num(obstacle_radius) },
        Collider {
            radius: FixedNum::from_num(obstacle_radius),
            layer: layers::OBSTACLE,
            mask: layers::UNIT,
        },
    ));
}
