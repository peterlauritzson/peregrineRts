use bevy::prelude::*;
use crate::game::GameState;

mod components;
mod setup;
mod minimap;
mod selection;
mod commands;

use setup::*;
use minimap::*;
use selection::*;
use commands::*;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::InGame), setup_hud)
           .add_systems(OnExit(GameState::InGame), cleanup_hud)
           .add_systems(Update, (
               update_selection_hud,
               button_system,
               command_handler,
               minimap_system,
               minimap_input_system,
           ).run_if(in_state(GameState::InGame)));
    }
}
