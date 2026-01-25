/// Physics integration and movement systems.
///
/// This module handles:
/// - Velocity integration
/// - Position updates
/// - Friction application
/// - Force sources
/// - Map bounds constraints

use bevy::prelude::*;
use crate::game::fixed_math::{FixedVec2, FixedNum};
use super::components::*;
use super::resources::*;
use peregrine_macros::profile;
use crate::profile_log;

// ============================================================================
// State Caching
// ============================================================================

/// Cache previous state for interpolation
#[profile(2)]
pub fn cache_previous_state(
    mut query: Query<(&mut SimPositionPrev, &SimPosition)>,
    #[allow(unused_variables)] tick: Res<SimTick>,
) {
    for (mut prev, pos) in query.iter_mut() {
        prev.0 = pos.0;
    }
    
    profile_log!(tick, "[CACHE_PREV_STATE] Entities: {}", query.iter().len());
}

// ============================================================================
// Physics Integration
// ============================================================================

/// Apply velocity to position
#[profile(2)]
pub fn apply_velocity(
    sim_config: Res<SimConfig>,
    mut query: Query<(&mut SimPosition, &mut SimVelocity, &mut SimAcceleration)>,
    #[allow(unused_variables)] tick: Res<SimTick>,
) {
    let delta = FixedNum::from_num(1.0) / FixedNum::from_num(sim_config.tick_rate);
    let max_velocity = sim_config.max_velocity;
    let max_velocity_sq = max_velocity * max_velocity;
    let max_acceleration = sim_config.max_acceleration;
    let max_acceleration_sq = max_acceleration * max_acceleration;
    let half_w = sim_config.map_size.get_width() / FixedNum::from_num(2.0);
    let half_h = sim_config.map_size.get_height() / FixedNum::from_num(2.0);

    for (mut pos, mut vel, mut acc) in query.iter_mut() {
        // Clamp acceleration to max_acceleration to prevent runaway forces
        let acc_sq = acc.0.length_squared();
        if acc_sq > max_acceleration_sq {
            acc.0 = acc.0.normalize() * max_acceleration;
        }
        
        // Apply acceleration
        if acc.0.length_squared() > FixedNum::ZERO {
            vel.0 = vel.0 + acc.0 * delta;
            acc.0 = FixedVec2::ZERO;
        }
        
        // Clamp velocity to max_velocity to prevent physics explosions
        let vel_sq = vel.0.length_squared();
        if vel_sq > max_velocity_sq {
            vel.0 = vel.0.normalize() * max_velocity;
        }

        // Update position
        if vel.0.length_squared() > FixedNum::ZERO {
            pos.0 = pos.0 + vel.0 * delta;
        }
        
        // Immediately constrain to map bounds after position update
        let was_out_of_bounds = pos.0.x < -half_w || pos.0.x > half_w || 
                                 pos.0.y < -half_h || pos.0.y > half_h;
        
        // Clamp position to map bounds
        if pos.0.x < -half_w { pos.0.x = -half_w; }
        if pos.0.x > half_w { pos.0.x = half_w; }
        if pos.0.y < -half_h { pos.0.y = -half_h; }
        if pos.0.y > half_h { pos.0.y = half_h; }
        
        // Zero velocity against walls
        if was_out_of_bounds {
            if pos.0.x <= -half_w && vel.0.x < FixedNum::ZERO { vel.0.x = FixedNum::ZERO; }
            if pos.0.x >= half_w && vel.0.x > FixedNum::ZERO { vel.0.x = FixedNum::ZERO; }
            if pos.0.y <= -half_h && vel.0.y < FixedNum::ZERO { vel.0.y = FixedNum::ZERO; }
            if pos.0.y >= half_h && vel.0.y > FixedNum::ZERO { vel.0.y = FixedNum::ZERO; }
        }
    }
    
    profile_log!(tick, "[APPLY_VELOCITY] Entities: {}", query.iter().len());
}

/// Apply friction to slow down entities
#[profile(2)]
pub fn apply_friction(
    mut query: Query<&mut SimVelocity>,
    sim_config: Res<SimConfig>,
    #[allow(unused_variables)] tick: Res<SimTick>,
) {
    let friction = sim_config.friction;
    let min_velocity_sq = sim_config.min_velocity * sim_config.min_velocity;
    for mut vel in query.iter_mut() {
        vel.0 = vel.0 * friction;
        if vel.0.length_squared() < min_velocity_sq {
            vel.0 = FixedVec2::ZERO;
        }
    }
    
    profile_log!(tick, "[APPLY_FRICTION] Entities: {}", query.iter().len());
}

// ============================================================================
// Force Sources
// ============================================================================

/// Apply force sources (black holes, wind, etc.)
#[profile(2)]
pub fn apply_forces(
    mut units: Query<(&SimPosition, &mut SimAcceleration)>,
    sources: Query<(&SimPosition, &ForceSource)>,
    #[allow(unused_variables)] tick: Res<SimTick>,
) {
    for (u_pos, mut u_acc) in units.iter_mut() {
        for (s_pos, source) in sources.iter() {
             let delta = s_pos.0 - u_pos.0;
             let dist_sq = delta.length_squared();
             
             // Check radius
             if source.radius > FixedNum::ZERO {
                 let r_sq = source.radius * source.radius;
                 if dist_sq > r_sq { continue; }
             }

             match source.force_type {
                 ForceType::Radial(strength) => {
                     let dist = dist_sq.sqrt();
                     if dist > FixedNum::from_num(0.1) {
                         let dir = delta / dist;
                         u_acc.0 = u_acc.0 + dir * strength;
                     }
                 },
                 ForceType::Directional(dir) => {
                     u_acc.0 = u_acc.0 + dir;
                 }
             }
        }
    }
    
    profile_log!(tick, "[APPLY_FORCES] Units: {}", units.iter().len());
}

// ============================================================================
// Map Constraints
// ============================================================================

// Note: Map bounds constraint is now integrated into apply_velocity system
// to ensure positions are always valid before spatial hash updates

// ============================================================================
// Steering Helpers
// ============================================================================

/// Seek steering behavior - accelerate towards a target
pub fn seek(pos: FixedVec2, target: FixedVec2, vel: FixedVec2, acc: &mut FixedVec2, speed: FixedNum, max_force: FixedNum) {
    let delta = target - pos;
    let dist_sq = delta.length_squared();
    if dist_sq > FixedNum::ZERO {
        let desired_vel = delta.normalize() * speed;
        let steer = desired_vel - vel;
        let steer_len_sq = steer.length_squared();
        let final_steer = if steer_len_sq > max_force * max_force {
            steer.normalize() * max_force
        } else {
            steer
        };
        *acc = *acc + final_steer;
    }
}
