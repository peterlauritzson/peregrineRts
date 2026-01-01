use bevy::prelude::*;

use bevy::window::WindowResolution;

use peregrine::game::GamePlugin;

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
        }))
        .add_plugins(GamePlugin)
        .run();
}

