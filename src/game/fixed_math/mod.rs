//! Deterministic fixed-point mathematics library.
//!
//! This module provides deterministic math types and operations using fixed-point
//! arithmetic to ensure identical behavior across different platforms and architectures.
//! This is critical for multiplayer lockstep networking where all clients must simulate
//! identically.

use fixed::types::I48F16;

pub use vec2::FixedVec2;

mod vec2;

/// Fixed-point number type used throughout the simulation.
/// 
/// Uses I48F16 format: 48 bits for the integer part, 16 bits for the fractional part.
/// This provides a range of approximately ±140 trillion with a precision of ~0.000015.
pub type FixedNum = I48F16;
