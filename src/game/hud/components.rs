use bevy::prelude::*;

/// Root marker component for HUD elements
#[derive(Component)]
pub struct HudRoot;

/// Minimap UI element
#[derive(Component)]
pub struct Minimap;

/// Camera frame shown on minimap
#[derive(Component)]
pub struct MinimapCameraFrame;

/// Link between a minimap dot and its entity
#[derive(Component)]
pub struct MinimapDot(pub Entity);

/// Marker for units that have a minimap dot
#[derive(Component)]
pub struct UnitMinimapDot;

/// Selection text display
#[derive(Component)]
pub struct SelectionText;

/// Command action types
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandAction {
    Stop,
    Move,
    Attack,
}

/// Command button component
#[derive(Component)]
pub struct CommandButton(pub CommandAction);
