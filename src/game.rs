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
mod menu;

use camera::RtsCameraPlugin;
use unit::UnitPlugin;
use control::ControlPlugin;
use simulation::SimulationPlugin;
use config::GameConfigPlugin;
use pathfinding::PathfindingPlugin;
use stress_test::StressTestPlugin;
use menu::MenuPlugin;

#[derive(States, Debug, Clone, Copy, Eq, PartialEq, Hash, Default)]
pub enum GameState {
    #[default]
    MainMenu,
    Settings,
    InGame,
    Editor,
    Paused,
}

pub struct GamePlugin {
    pub stress_test: bool,
}

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>();

        app.add_plugins((
            GameConfigPlugin,
            MenuPlugin,
            RtsCameraPlugin,
            UnitPlugin,
            ControlPlugin,
            SimulationPlugin,
            PathfindingPlugin,
        ));

        if self.stress_test {
            app.add_plugins(StressTestPlugin);
        }
        
        // Common setup (UI Camera)
        app.add_systems(Startup, setup_common);

        // Game setup (Map, Lights)
        app.add_systems(OnEnter(GameState::InGame), setup_game);
        app.add_systems(OnEnter(GameState::Editor), setup_game); // Reuse for now
        
        // Cleanup
        app.add_systems(OnExit(GameState::InGame), cleanup_game);
        app.add_systems(OnExit(GameState::Editor), cleanup_game);
    }
}

#[derive(Component)]
pub struct GameEntity;

fn setup_common(mut commands: Commands) {
    // UI Camera - needed for Menu and HUD
    commands.spawn((
        Camera2d::default(),
        Camera {
            order: 1,
            ..default()
        },
    ));
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
        GameEntity,
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
        GameEntity,
    ));
}

fn cleanup_game(mut commands: Commands, query: Query<Entity, With<GameEntity>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}
