use bevy::prelude::*;

use bevy::window::WindowResolution;

use peregrine::game::GamePlugin;

use bevy::log::LogPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Peregrine RTS".into(),
                resolution: WindowResolution::new(1280, 720),
                resizable: true,
                ..default()
            }),
            ..default()
        }).set(LogPlugin {
            level: bevy::log::Level::WARN,
            filter: "wgpu=error,bevy_render=info,bevy_ecs=info,bevy_diagnostic=info,peregrine=info".to_string(),
            ..default()
        }))
        .add_plugins(GamePlugin)
        .run();
}

