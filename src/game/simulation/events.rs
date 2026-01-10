/// Events and commands for simulation control.
///
/// This module defines all events used to control simulation entities,
/// including movement commands, spawning, and stopping.

use bevy::prelude::*;
use crate::game::fixed_math::FixedVec2;

// ============================================================================
// Unit Commands
// ============================================================================

/// Command to move a unit to a target position
#[derive(Event, Message, Debug, Clone)]
pub struct UnitMoveCommand {
    pub player_id: u8,
    pub entity: Entity,
    pub target: FixedVec2,
}

/// Command to stop a unit's movement
#[derive(Event, Message, Debug, Clone)]
pub struct UnitStopCommand {
    pub player_id: u8,
    pub entity: Entity,
}

/// Command to spawn a new unit
#[derive(Event, Message, Debug, Clone)]
pub struct SpawnUnitCommand {
    pub player_id: u8,
    pub position: FixedVec2,
}
