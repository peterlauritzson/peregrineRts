//! Generic collections and data structures for high-performance game systems.
//!
//! This module contains reusable data structures optimized for large-scale entity management,
//! particularly useful for RTS games with millions of entities.
//!
//! # Example: Quick Start
//!
//! ```rust
//! use bevy::prelude::Entity;
//! use peregrine::game::collections::{InclusionSet, SetConfig};
//!
//! // Use default configuration (10M max, 100K hot capacity)
//! let set = InclusionSet::<Entity>::new(SetConfig::default());
//!
//! // Or customize for your needs
//! let config = SetConfig {
//!     max_capacity: 5_000_000,      // Adjust based on your game scale
//!     hot_capacity: Some(500_000),   // Tune based on expected active entities
//!     ..Default::default()
//! };
//! let set = InclusionSet::<Entity>::new(config);
//! ```

pub mod components;
pub mod inclusion_set;

#[cfg(test)]
mod tests;

pub use components::InclusionIndex;
pub use inclusion_set::{SetConfig, InclusionSet, IncludeResult, IndexUpdate, SetStats};
