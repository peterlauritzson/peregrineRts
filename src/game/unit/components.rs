use bevy::prelude::*;

/// Marks an entity as a unit in the game
#[derive(Component)]
pub struct Unit;

/// Health component for units
#[derive(Component)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

/// Marks a unit as currently selected by the player
#[derive(Component)]
pub struct Selected;

/// Marks the child entity that renders the selection circle
#[derive(Component)]
pub struct SelectionCircle;

/// Marks the child entity that renders the health bar
#[derive(Component)]
pub struct HealthBar;
