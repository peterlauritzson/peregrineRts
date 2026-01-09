use bevy::prelude::*;
use crate::game::GameState;

mod resources;
mod selection;
mod commands;
mod debug;

use resources::*;
use selection::*;
use commands::*;
use debug::*;

pub use resources::InputMode;

pub struct ControlPlugin;

impl Plugin for ControlPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DragState>()
           .init_resource::<InputMode>()
           .add_systems(Startup, setup_selection_box)
           .add_systems(Update, (handle_input, handle_debug_spawning, clear_force_sources).run_if(in_state(GameState::InGame).or(in_state(GameState::Editor))));
    }
}
