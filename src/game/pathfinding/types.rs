use bevy::prelude::*;
use crate::game::fixed_math::{FixedVec2, FixedNum};
use serde::{Serialize, Deserialize};
use std::cmp::Ordering;

/// Fixed cluster size for hierarchical pathfinding (25×25 cells).
///
/// Maps are divided into clusters of this size. Larger clusters reduce graph size
/// but increase intra-cluster pathfinding cost. 25×25 provides good balance.
pub const CLUSTER_SIZE: usize = 25;

#[derive(Event, Message, Debug, Clone)]
pub struct PathRequest {
    pub entity: Entity,
    #[allow(dead_code)]
    pub start: FixedVec2,
    pub goal: FixedVec2,
}

#[derive(Component, Debug, Clone)]
pub enum Path {
    Direct(FixedVec2),
    LocalAStar { waypoints: Vec<FixedVec2>, current_index: usize },
    Hierarchical {
        portals: Vec<usize>,
        final_goal: FixedVec2,
        current_index: usize,
    }
}

impl Default for Path {
    fn default() -> Self {
        Path::Direct(FixedVec2::ZERO)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize, PartialOrd, Ord)]
pub struct Node {
    pub x: usize,
    pub y: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct State {
    pub cost: FixedNum,
    pub node: Node,
}

impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        other.cost.cmp(&self.cost)
            .then_with(|| self.node.x.cmp(&other.node.x))
            .then_with(|| self.node.y.cmp(&other.node.y))
    }
}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct GraphState {
    pub cost: FixedNum,
    pub portal_id: usize,
}

impl Ord for GraphState {
    fn cmp(&self, other: &Self) -> Ordering {
        other.cost.cmp(&self.cost)
            .then_with(|| self.portal_id.cmp(&other.portal_id))
    }
}

impl PartialOrd for GraphState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalFlowField {
    pub width: usize,
    pub height: usize,
    pub vectors: Vec<FixedVec2>, // Row-major, size width * height
    pub integration_field: Vec<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Portal {
    pub id: usize,
    pub node: Node,
    pub range_min: Node,
    pub range_max: Node,
    pub cluster: (usize, usize),
}
