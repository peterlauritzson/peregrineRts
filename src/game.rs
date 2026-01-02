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
pub mod stress_test;
pub mod map;

use camera::RtsCameraPlugin;
use unit::UnitPlugin;
use control::ControlPlugin;
use simulation::SimulationPlugin;
use config::GameConfigPlugin;
use pathfinding::PathfindingPlugin;
use stress_test::StressTestPlugin;

pub struct GamePlugin {
    pub stress_test: bool,
}

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            RtsCameraPlugin,
            UnitPlugin,
            ControlPlugin,
            SimulationPlugin,
            GameConfigPlugin,
            PathfindingPlugin,
        ));

        if self.stress_test {
            app.add_plugins(StressTestPlugin);
        }
        
        app.add_systems(Startup, setup_game);
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
        Mesh3d(meshes.add(Plane3d::default().mesh().size(2048.0, 2048.0))),
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
}
