/// Shared data structures used across multiple game modules
///
/// This module contains core data structures that are used by
/// simulation, pathfinding, editor, and other systems.

mod flow_field;

pub use flow_field::{FlowField, CELL_SIZE};
