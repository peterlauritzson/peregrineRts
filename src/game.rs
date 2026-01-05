use bevy::prelude::*;

mod camera;
pub mod unit;  // Made public for test access to Unit component
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
mod hud;
pub mod loading;  // Made public for test access to LoadingProgress
mod editor;

use camera::RtsCameraPlugin;
use unit::UnitPlugin;
use control::ControlPlugin;
use simulation::SimulationPlugin;
use config::GameConfigPlugin;
use pathfinding::PathfindingPlugin;
use stress_test::StressTestPlugin;
use menu::MenuPlugin;
use hud::HudPlugin;
use loading::LoadingPlugin;
use editor::EditorPlugin;

#[derive(States, Debug, Clone, Copy, Eq, PartialEq, Hash, Default)]
pub enum GameState {
    #[default]
    MainMenu,
    Settings,
    Loading,
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
            HudPlugin,
            LoadingPlugin,
            EditorPlugin,
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

#[derive(Component)]
pub struct GroundPlane;

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
    sim_config: Res<simulation::SimConfig>,
) {
    info!("Game setup started");

    // Ground Plane - sized to match map dimensions
    let map_width = sim_config.map_width.to_num::<f32>();
    let map_height = sim_config.map_height.to_num::<f32>();
    info!("Creating ground plane: {}x{}", map_width, map_height);
    
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(map_width, map_height))),
        MeshMaterial3d(materials.add(Color::srgb(0.3, 0.5, 0.3))),
        GameEntity,
        GroundPlane,
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
