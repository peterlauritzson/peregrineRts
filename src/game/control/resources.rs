use bevy::prelude::*;

/// State for tracking mouse drag operations
#[derive(Resource, Default)]
pub struct DragState {
    pub start: Option<Vec2>,
    pub current: Option<Vec2>,
}

/// Current input mode for player commands
#[derive(Resource, Default, PartialEq, Eq, Clone, Copy, Debug)]
pub enum InputMode {
    #[default]
    Selection,
    CommandMove,
    CommandAttack,
}

/// Marker component for the selection box UI element
#[derive(Component)]
pub struct SelectionBox;
