use bevy::prelude::*;
use crate::game::fixed_math::{FixedVec2, FixedNum};
use crate::game::simulation::{SimPosition, SimVelocity, SimConfig, BoidsNeighborCache, SimTick};
use peregrine_macros::profile;
use crate::profile_log;

use super::components::Unit;

/// Applies boids-based steering behaviors (separation, alignment, cohesion) to units
/// 
/// This system reads from cached neighbor lists (computed by the simulation module)
/// and applies steering forces to create flocking behavior. The three behaviors are:
/// - **Separation**: Avoid crowding neighbors that are too close
/// - **Alignment**: Steer toward the average heading of neighbors
/// - **Cohesion**: Steer toward the average position (center of mass) of neighbors
#[profile(2)]
pub(super) fn apply_boids_steering(
    mut query: Query<(Entity, &SimPosition, &mut SimVelocity, &BoidsNeighborCache), With<Unit>>,
    sim_config: Res<SimConfig>,
    #[allow(unused_variables)] tick: Res<SimTick>,
) {
    
    let separation_weight = sim_config.separation_weight;
    let alignment_weight = sim_config.alignment_weight;
    let cohesion_weight = sim_config.cohesion_weight;
    let separation_radius = sim_config.separation_radius;
    let max_speed = sim_config.unit_speed;

    // Early exit if all weights are zero
    if separation_weight == FixedNum::ZERO && alignment_weight == FixedNum::ZERO && cohesion_weight == FixedNum::ZERO {
        return;
    }

    let separation_radius_sq = separation_radius * separation_radius;

    // No HashMap allocation! Read directly from cached neighbor lists
    let mut steering_forces = Vec::with_capacity(query.iter().count());

    for (entity, pos, vel, boids_cache) in query.iter() {
        // Early exit if no cached neighbors
        if boids_cache.neighbors.is_empty() {
            continue;
        }
        
        // Accumulate forces (unnormalized for efficiency)
        let mut separation_accum = FixedVec2::ZERO;
        let mut alignment_accum = FixedVec2::ZERO;
        let mut cohesion_accum = FixedVec2::ZERO;
        
        let mut neighbor_count = 0;
        let mut separation_count = 0;

        // Read from cached neighbor list - no spatial hash query, no HashMap lookups!
        for &(other_entity, other_pos, other_vel) in &boids_cache.neighbors {
            // Skip self (shouldn't be in cache, but check anyway)
            if entity == other_entity {
                continue;
            }

            // Work with squared distances to avoid sqrt
            let diff = pos.0 - other_pos;
            let dist_sq = diff.length_squared();
            
            // CRITICAL: Only consider neighbors within the neighbor_radius
            // The cache may include up to N closest neighbors, but some might be far away
            let neighbor_radius_sq = sim_config.neighbor_radius * sim_config.neighbor_radius;
            if dist_sq > neighbor_radius_sq {
                continue; // Skip neighbors outside the radius
            }

            // All neighbors within radius affect alignment & cohesion
            alignment_accum = alignment_accum + other_vel;
            cohesion_accum = cohesion_accum + other_pos;
            neighbor_count += 1;

            // Separation: only for very close neighbors
            // Use squared distance math - no sqrt needed!
            if dist_sq < separation_radius_sq {
                // Guard against division by zero or near-zero distances
                // Use a larger epsilon to prevent numeric overflow
                let min_dist_sq = FixedNum::from_num(0.25); // 0.5 units minimum distance
                if dist_sq > min_dist_sq {
                    // Inverse-square falloff for separation strength
                    let strength = separation_radius_sq / dist_sq;
                    // Cap the maximum strength to prevent overflow
                    let capped_strength = strength.min(FixedNum::from_num(100.0));
                    separation_accum = separation_accum + diff * capped_strength;
                    separation_count += 1;
                } else {
                    // Units too close - use maximum separation force in normalized direction
                    if diff.length_squared() > FixedNum::ZERO {
                        let normalized_diff = diff.normalize();
                        separation_accum = separation_accum + normalized_diff * FixedNum::from_num(100.0);
                        separation_count += 1;
                    }
                }
            }
        }

        // Skip if no neighbors affected this unit
        if neighbor_count == 0 {
            continue;
        }

        // Calculate final steering forces
        let mut total_force = FixedVec2::ZERO;

        // Alignment: steer toward average heading
        if alignment_weight > FixedNum::ZERO && neighbor_count > 0 {
            let avg_vel = alignment_accum / FixedNum::from_num(neighbor_count);
            let desired = if avg_vel.length_squared() > FixedNum::ZERO {
                avg_vel.normalize() * max_speed
            } else {
                FixedVec2::ZERO
            };
            let alignment_force = desired - vel.0;
            total_force = total_force + alignment_force * alignment_weight;
        }

        // Cohesion: steer toward center of mass
        if cohesion_weight > FixedNum::ZERO && neighbor_count > 0 {
            let center_of_mass = cohesion_accum / FixedNum::from_num(neighbor_count);
            let direction = center_of_mass - pos.0;
            let desired = if direction.length_squared() > FixedNum::ZERO {
                direction.normalize() * max_speed
            } else {
                FixedVec2::ZERO
            };
            let cohesion_force = desired - vel.0;
            total_force = total_force + cohesion_force * cohesion_weight;
        }

        // Separation: steer away from crowded neighbors
        if separation_weight > FixedNum::ZERO && separation_count > 0 {
            // Normalize the accumulated separation vector
            let separation_force = if separation_accum.length_squared() > FixedNum::ZERO {
                separation_accum.normalize() * max_speed - vel.0
            } else {
                FixedVec2::ZERO
            };
            total_force = total_force + separation_force * separation_weight;
        }

        steering_forces.push((entity, total_force));
    }

    // Apply forces
    let delta = FixedNum::from_num(1.0) / FixedNum::from_num(sim_config.tick_rate);
    for (entity, force) in steering_forces {
        if let Ok((_, _, mut vel, _)) = query.get_mut(entity) {
            vel.0 = vel.0 + force * delta;
            
            // Only clamp if exceeded max speed
            let speed_sq = vel.0.length_squared();
            let max_speed_sq = max_speed * max_speed;
            if speed_sq > max_speed_sq {
                vel.0 = vel.0.normalize() * max_speed;
            }
        }
    }
    
    profile_log!(tick, "[BOIDS_STEERING] Units: {}", query.iter().len());
}

#[cfg(test)]
#[path = "boids_tests.rs"]
mod tests;
