use bevy::prelude::*;
use crate::game::unit::{Selected, Health};
use super::components::*;

/// Update the selection HUD to show selected unit info
pub fn update_selection_hud(
    selected_units: Query<(Entity, Option<&Health>), With<Selected>>,
    mut text_query: Query<&mut Text, With<SelectionText>>,
) {
    let count = selected_units.iter().count();
    for mut text in &mut text_query {
        if count == 0 {
            **text = "No Selection".to_string();
        } else if count == 1 {
            if let Ok((entity, health)) = selected_units.single() {
                let health_str = if let Some(h) = health {
                    format!("HP: {:.0}/{:.0}", h.current, h.max)
                } else {
                    "HP: N/A".to_string()
                };
                **text = format!("Unit ID: {:?}\n{}", entity, health_str);
            }
        } else {
            **text = format!("Selected: {} units", count);
        }
    }
}
