//! Components for inclusion set tracking.

use bevy::prelude::*;

/// Component storing an entity's index in the InclusionSet hot storage.
/// 
/// This is returned by `include()` and must be inserted as a component.
/// It's required by `exclude()` for O(1) removal verification.
/// 
/// Default value of 0 is used as a placeholder until the entity is added to a set.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InclusionIndex(pub usize);
