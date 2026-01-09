/// Menu system - handles main menu, pause menu, settings, and random map dialog
/// 
/// This module is organized into:
/// - components: Shared UI component types and resources
/// - ui_utils: Common UI utilities like the spawn_button macro
/// - main_menu: Main menu screen logic
/// - pause: Pause menu overlay logic
/// - settings: Settings screen with keybinding and options
/// - random_map: Random map generation dialog

mod components;
mod ui_utils;
mod main_menu;
mod pause;
mod settings;
mod random_map;

use bevy::prelude::*;
use crate::game::GameState;

pub use components::{RandomMapState, ActiveRandomMapField};

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app
            // Resources
            .init_resource::<RandomMapState>()
            .init_resource::<ActiveRandomMapField>()
            
            // Main Menu
            .add_systems(OnEnter(GameState::MainMenu), main_menu::setup_menu)
            .add_systems(OnExit(GameState::MainMenu), main_menu::cleanup_menu)
            .add_systems(
                Update,
                (
                    main_menu::menu_action,
                    random_map::handle_random_map_dialog,
                    random_map::handle_random_map_input_clicks,
                    random_map::keyboard_input_random_map,
                    random_map::update_random_map_dialog_values,
                    random_map::update_random_map_field_borders,
                    random_map::update_button_colors,
                ).run_if(in_state(GameState::MainMenu))
            )
            
            // Pause Menu
            .add_systems(
                Update,
                pause::toggle_pause.run_if(
                    in_state(GameState::InGame)
                        .or(in_state(GameState::Paused))
                        .or(in_state(GameState::Editor))
                )
            )
            .add_systems(OnEnter(GameState::Paused), pause::setup_pause_menu)
            .add_systems(OnExit(GameState::Paused), pause::cleanup_pause_menu)
            .add_systems(Update, pause::pause_menu_action.run_if(in_state(GameState::Paused)))
            
            // Settings Menu
            .add_systems(OnEnter(GameState::Settings), settings::setup_settings_menu)
            .add_systems(OnExit(GameState::Settings), settings::cleanup_settings_menu)
            .add_systems(
                Update,
                (settings::settings_action, settings::handle_rebinding)
                    .run_if(in_state(GameState::Settings))
            );
    }
}
