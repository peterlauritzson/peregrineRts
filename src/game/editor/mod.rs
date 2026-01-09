mod components;
mod ui;
mod input;
mod generation;
mod actions;

use bevy::prelude::*;
use crate::game::GameState;

pub use components::*;
use ui::*;
use input::*;
use generation::*;
use actions::*;

/// Plugin for the map editor functionality
pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorState>()
           .init_resource::<ActiveInputField>()
           .add_systems(Startup, setup_editor_resources)
           .add_systems(OnEnter(GameState::Editor), setup_editor_ui)
           .add_systems(OnExit(GameState::Editor), cleanup_editor_ui)
           .add_systems(Update, (
               editor_button_system, 
               handle_editor_input, 
               handle_generation, 
               cleanup_generation_overlay, 
               check_finalization_complete, 
               keyboard_input_system, 
               handle_input_field_clicks
           ).run_if(in_state(GameState::Editor)));
    }
}

/// Sets up editor resources at startup
fn setup_editor_resources(
    mut commands: Commands, 
    mut meshes: ResMut<Assets<Mesh>>, 
    mut materials: ResMut<Assets<StandardMaterial>>
) {
    commands.insert_resource(EditorResources {
        obstacle_mesh: meshes.add(Cylinder::new(1.0, 2.0)), // Cylinder with radius 1.0 and height 2.0, scale it later
        obstacle_material: materials.add(Color::srgb(0.5, 0.5, 0.5)),
    });
}
