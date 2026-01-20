mod components;
mod resources;
mod visuals;
mod boids;

use bevy::prelude::*;
use crate::game::GameState;
use crate::game::simulation::SimSet;
use crate::game::pathfinding::follow_path;

// Re-export public types
pub use components::{Unit, Health, Selected, SelectionCircle, HealthBar};
pub use resources::{HealthBarSettings, UnitMesh, UnitMaterials};
pub use boids::apply_boids_steering;

use resources::setup_unit_resources;
use visuals::{spawn_unit_visuals, sync_visuals, update_selection_visuals, 
              update_selection_circle_visibility, update_unit_lod,
              toggle_health_bars, update_health_bars};

/// Plugin that manages unit entities, their visuals, and behaviors
pub struct UnitPlugin;

impl Plugin for UnitPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HealthBarSettings>()
           .add_systems(Startup, setup_unit_resources)
           // Boids steering runs in FixedUpdate after pathfinding
           .add_systems(FixedUpdate, 
               apply_boids_steering
                   .in_set(SimSet::Steering)
                   .after(follow_path))
           // Visual systems run in Update for smooth rendering
           .add_systems(Update, (
               spawn_unit_visuals,
               update_selection_visuals,
               update_selection_circle_visibility,
               update_health_bars,
               toggle_health_bars,
               sync_visuals,
               update_unit_lod,
           ).run_if(in_state(GameState::InGame).or(in_state(GameState::Editor))));
    }
}
