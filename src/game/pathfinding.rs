use bevy::prelude::*;
use crate::game::math::FixedVec2;

#[derive(Event, Message, Debug, Clone)]
pub struct PathRequest {
    pub entity: Entity,
    #[allow(dead_code)]
    pub start: FixedVec2,
    pub goal: FixedVec2,
}

#[derive(Component, Debug, Clone, Default)]
pub struct Path {
    pub waypoints: Vec<FixedVec2>,
    pub current_index: usize,
}

pub struct PathfindingPlugin;

impl Plugin for PathfindingPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PathRequest>();
        app.add_systems(FixedUpdate, process_path_requests);
    }
}

fn process_path_requests(
    mut commands: Commands,
    mut requests: MessageReader<PathRequest>,
    // In Phase 1, we will just do a direct line to test the interface
) {
    for req in requests.read() {
        // Placeholder: Just go straight to goal
        let path = Path {
            waypoints: vec![req.goal],
            current_index: 0,
        };
        commands.entity(req.entity).insert(path);
    }
}
