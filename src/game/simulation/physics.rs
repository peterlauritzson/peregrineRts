/// Physics integration and movement systems.
///
/// This module handles:
/// - Velocity integration
/// - Position updates
/// - Friction application
/// - Force sources
/// - Map bounds constraints

use bevy::prelude::*;
use crate::game::math::{FixedVec2, FixedNum};
use super::components::*;
use super::resources::*;

// ============================================================================
// State Caching
// ============================================================================

/// Cache previous state for interpolation
pub fn cache_previous_state(
    mut query: Query<(&mut SimPositionPrev, &SimPosition)>,
    time: Res<Time<Fixed>>,
) {
    let start_time = std::time::Instant::now();
    let entity_count = query.iter().count();
    
    for (mut prev, pos) in query.iter_mut() {
        prev.0 = pos.0;
    }
    
    let duration = start_time.elapsed();
    let tick = (time.elapsed_secs() * 30.0) as u64;
    if duration.as_millis() > 2 || tick % 100 == 0 {
        info!("[CACHE_PREV_STATE] {:?} | Entities: {}", duration, entity_count);
    }
}

// ============================================================================
// Physics Integration
// ============================================================================

/// Apply velocity to position
pub fn apply_velocity(
    sim_config: Res<SimConfig>,
    mut query: Query<(&mut SimPosition, &mut SimVelocity, &mut SimAcceleration)>,
    time: Res<Time<Fixed>>,
) {
    let start_time = std::time::Instant::now();
    let delta = FixedNum::from_num(1.0) / FixedNum::from_num(sim_config.tick_rate);
    let entity_count = query.iter().count();

    for (mut pos, mut vel, mut acc) in query.iter_mut() {
        // Apply acceleration
        if acc.0.length_squared() > FixedNum::ZERO {
            vel.0 = vel.0 + acc.0 * delta;
            // Limit velocity to max speed? Or let drag handle it?
            // Let's clamp it for safety, though drag is better.
            // Actually, let's not clamp here, let steering/drag handle it.
            // But we should reset acceleration.
            acc.0 = FixedVec2::ZERO;
        }

        if vel.0.length_squared() > FixedNum::ZERO {
            pos.0 = pos.0 + vel.0 * delta;
        }
    }
    
    let duration = start_time.elapsed();
    let tick = (time.elapsed_secs() * 30.0) as u64;
    if duration.as_millis() > 2 || tick % 100 == 0 {
        info!("[APPLY_VELOCITY] {:?} | Entities: {}", duration, entity_count);
    }
}

/// Apply friction to slow down entities
pub fn apply_friction(
    mut query: Query<&mut SimVelocity>,
    sim_config: Res<SimConfig>,
    time: Res<Time<Fixed>>,
) {
    let start_time = std::time::Instant::now();
    let entity_count = query.iter().count();
    
    let friction = sim_config.friction;
    let min_velocity_sq = sim_config.min_velocity * sim_config.min_velocity;
    for mut vel in query.iter_mut() {
        vel.0 = vel.0 * friction;
        if vel.0.length_squared() < min_velocity_sq {
            vel.0 = FixedVec2::ZERO;
        }
    }
    
    let duration = start_time.elapsed();
    let tick = (time.elapsed_secs() * 30.0) as u64;
    if duration.as_millis() > 2 || tick % 100 == 0 {
        warn!("[APPLY_FRICTION] {:?} | Entities: {}", duration, entity_count);
    }
}

// ============================================================================
// Force Sources
// ============================================================================

/// Apply force sources (black holes, wind, etc.)
pub fn apply_forces(
    mut units: Query<(&SimPosition, &mut SimAcceleration)>,
    sources: Query<(&SimPosition, &ForceSource)>,
    time: Res<Time<Fixed>>,
) {
    let start_time = std::time::Instant::now();
    let unit_count = units.iter().count();
    
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
    
    let duration = start_time.elapsed();
    let tick = (time.elapsed_secs() * 30.0) as u64;
    if duration.as_millis() > 2 || tick % 100 == 0 {
        info!("[APPLY_FORCES] {:?} | Units: {}", duration, unit_count);
    }
}

// ============================================================================
// Map Constraints
// ============================================================================

/// Constrain entities to map bounds
pub fn constrain_to_map_bounds(
    mut query: Query<(Entity, &mut SimPosition, &mut SimVelocity)>,
    sim_config: Res<SimConfig>,
) {
    let half_w = sim_config.map_width / FixedNum::from_num(2.0);
    let half_h = sim_config.map_height / FixedNum::from_num(2.0);
    
    let mut escaped_count = 0;

    for (entity, mut pos, mut vel) in query.iter_mut() {
        let was_out_of_bounds = pos.0.x < -half_w || pos.0.x > half_w || 
                                 pos.0.y < -half_h || pos.0.y > half_h;
        
        // 1. Clamp Position
        if pos.0.x < -half_w { pos.0.x = -half_w; }
        if pos.0.x > half_w { pos.0.x = half_w; }
        if pos.0.y < -half_h { pos.0.y = -half_h; }
        if pos.0.y > half_h { pos.0.y = half_h; }
        
        if was_out_of_bounds {
            escaped_count += 1;
            if escaped_count <= 3 {
                warn!("[BOUNDS] Entity {:?} was outside map bounds! Pos: {:?}, Bounds: ±{} x ±{}", 
                      entity, pos.0, half_w, half_h);
            }
        }

        // 2. Zero Velocity against walls
        if pos.0.x <= -half_w && vel.0.x < FixedNum::ZERO { vel.0.x = FixedNum::ZERO; }
        if pos.0.x >= half_w && vel.0.x > FixedNum::ZERO { vel.0.x = FixedNum::ZERO; }
        if pos.0.y <= -half_h && vel.0.y < FixedNum::ZERO { vel.0.y = FixedNum::ZERO; }
        if pos.0.y >= half_h && vel.0.y > FixedNum::ZERO { vel.0.y = FixedNum::ZERO; }
    }
    
    if escaped_count > 3 {
        warn!("[BOUNDS] {} total entities escaped map bounds this tick!", escaped_count);
    }
}

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
