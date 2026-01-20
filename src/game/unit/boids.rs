use bevy::prelude::*;
use crate::game::fixed_math::{FixedVec2, FixedNum};
use crate::game::simulation::{SimPosition, SimVelocity, SimConfig, SimTick};
use crate::game::spatial_hash::{SpatialHash, SpatialHashScratch};
use peregrine_macros::profile;
use crate::profile_log;

use super::components::Unit;

/// Applies boids-based steering behaviors (separation, alignment, cohesion) to units
/// 
/// Queries spatial hash directly for neighbors and applies steering forces
/// to create flocking behavior. The three behaviors are:
/// - **Separation**: Avoid crowding neighbors that are too close
/// - **Alignment**: Steer toward the average heading of neighbors
/// - **Cohesion**: Steer toward the average position (center of mass) of neighbors
#[profile(2)]
pub fn apply_boids_steering(
    units_query: Query<(Entity, &SimPosition), With<Unit>>,
    all_positions: Query<(Entity, &SimPosition)>,
    mut velocities: Query<(Entity, &mut SimVelocity)>,
    spatial_hash: Res<SpatialHash>,
    mut scratch: ResMut<SpatialHashScratch>,
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
    let neighbor_radius_sq = sim_config.neighbor_radius * sim_config.neighbor_radius;

    // Collect position data
    let position_map: std::collections::HashMap<Entity, FixedVec2> = 
        all_positions.iter().map(|(e, p)| (e, p.0)).collect();
    
    // Collect velocity data (read from mutable query, will write later)
    let velocity_map: std::collections::HashMap<Entity, FixedVec2> = 
        velocities.iter().map(|(e, vel)| (e, vel.0)).collect();
    
    // Preallocated buffer for steering forces
    let mut steering_forces = Vec::with_capacity(units_query.iter().count());

    for (entity, pos) in units_query.iter() {
        // Get this unit's velocity from the map
        let vel = if let Some(&v) = velocity_map.get(&entity) {
            v
        } else {
            continue;
        };
        
        // Query spatial hash directly for nearby neighbors
        spatial_hash.query_radius(pos.0, sim_config.neighbor_radius, Some(entity), &mut scratch);
        
        // Early exit if no neighbors found
        if scratch.query_results.is_empty() {
            continue;
        }
        
        // Get closest N neighbors using partial sort (same logic as old cache update)
        let mut neighbors_with_dist: Vec<_> = scratch.query_results.iter()
            .filter_map(|&neighbor_entity| {
                if let Some(&neighbor_pos) = position_map.get(&neighbor_entity) {
                    let dist_sq = (pos.0 - neighbor_pos).length_squared();
                    Some((neighbor_entity, neighbor_pos, dist_sq))
                } else {
                    None
                }
            })
            .collect();
        
        // Use partial sort to get closest N neighbors
        let max_neighbors = sim_config.boids_max_neighbors.min(neighbors_with_dist.len());
        if max_neighbors > 0 && neighbors_with_dist.len() > max_neighbors {
            neighbors_with_dist.select_nth_unstable_by(
                max_neighbors - 1,
                |a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal)
            );
        }
        
        // Accumulate forces (unnormalized for efficiency)
        let mut separation_accum = FixedVec2::ZERO;
        let mut alignment_accum = FixedVec2::ZERO;
        let mut cohesion_accum = FixedVec2::ZERO;
        
        let mut neighbor_count = 0;
        let mut separation_count = 0;

        // Process the closest N neighbors
        for (other_entity, other_pos, dist_sq) in neighbors_with_dist.iter().take(max_neighbors) {
            // Skip self (shouldn't happen with query exclusion, but check anyway)
            if entity == *other_entity {
                continue;
            }

            // Work with squared distances to avoid sqrt
            let diff = pos.0 - *other_pos;
            
            // Only consider neighbors within the neighbor_radius
            if *dist_sq > neighbor_radius_sq {
                continue; // Skip neighbors outside the radius
            }

            // Get velocity for alignment calculation
            let other_vel = if let Some(&v) = velocity_map.get(other_entity) {
                v
            } else {
                continue;
            };

            // All neighbors within radius affect alignment & cohesion
            alignment_accum = alignment_accum + other_vel;
            cohesion_accum = cohesion_accum + *other_pos;
            neighbor_count += 1;

            // Separation: only for very close neighbors
            // Use squared distance math - no sqrt needed!
            if *dist_sq < separation_radius_sq {
                // Guard against division by zero or near-zero distances
                // Use a larger epsilon to prevent numeric overflow
                let min_dist_sq = FixedNum::from_num(0.25); // 0.5 units minimum distance
                if *dist_sq > min_dist_sq {
                    // Inverse-square falloff for separation strength
                    let strength = separation_radius_sq / *dist_sq;
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
            let alignment_force = desired - vel;
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
            let cohesion_force = desired - vel;
            total_force = total_force + cohesion_force * cohesion_weight;
        }

        // Separation: steer away from crowded neighbors
        if separation_weight > FixedNum::ZERO && separation_count > 0 {
            // Normalize the accumulated separation vector
            let separation_force = if separation_accum.length_squared() > FixedNum::ZERO {
                separation_accum.normalize() * max_speed - vel
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
        if let Ok((_, mut vel)) = velocities.get_mut(entity) {
            vel.0 = vel.0 + force * delta;
            
            // Only clamp if exceeded max speed
            let speed_sq = vel.0.length_squared();
            let max_speed_sq = max_speed * max_speed;
            if speed_sq > max_speed_sq {
                vel.0 = vel.0.normalize() * max_speed;
            }
        }
    }
    
    profile_log!(tick, "[BOIDS_STEERING] Units: {}", units_query.iter().count());
}

#[cfg(test)]
#[path = "boids_tests.rs"]
mod tests;
