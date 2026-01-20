/// Collision detection and resolution systems.
///
/// This module handles:
/// - Collision detection using cached neighbor lists
/// - Unit-unit collision resolution
/// - Unit-obstacle collision resolution

use bevy::prelude::*;
use crate::game::fixed_math::{FixedVec2, FixedNum};
use crate::game::spatial_hash::{SpatialHash, SpatialHashScratch};
use crate::game::profiling::profile;
use super::components::*;
use super::resources::*;

// ============================================================================
// Events
// ============================================================================

/// Event fired when two entities collide
#[derive(Event, Message, Debug, Clone)]
pub struct CollisionEvent {
    pub entity1: Entity,
    pub entity2: Entity,
    pub overlap: FixedNum,
    pub normal: FixedVec2,
}

// ============================================================================
// Collision Detection
// ============================================================================

/// Detect collisions between entities by querying spatial hash directly.
/// 
/// Uses preallocated scratch buffer for zero-allocation spatial queries.
/// No caching - queries fresh position data every frame for accuracy.
#[profile]
pub fn detect_collisions(
    mut query: Query<(Entity, &SimPosition, &Collider, &mut CollisionState)>,
    position_collider_query: Query<(&SimPosition, &Collider)>,
    spatial_hash: Res<SpatialHash>,
    mut scratch: ResMut<SpatialHashScratch>,
    sim_config: Res<SimConfig>,
    mut events: MessageWriter<CollisionEvent>,
    mut colliding_entities: Local<std::collections::HashSet<Entity>>,
) {
    colliding_entities.clear();

    // Query spatial hash directly for each entity (uses preallocated scratch buffer)
    for (entity, pos, collider, _) in query.iter() {
        let search_radius = collider.radius * sim_config.collision_search_radius_multiplier;
        
        // Zero-allocation spatial query via scratch buffer
        spatial_hash.query_radius(
            pos.0,
            search_radius,
            Some(entity),
            &mut scratch
        );
        
        // Check each nearby entity for collision
        for &other_entity in &scratch.query_results {
            // Skip duplicates to avoid double-processing the same collision
            if entity > other_entity {
                continue;
            }
            
            // Fetch current position and collider data
            let Ok((other_pos, other_collider)) = position_collider_query.get(other_entity) else {
                continue;
            };
            
            // Check collision layers
            if (collider.mask & other_collider.layer) == 0 && (other_collider.mask & collider.layer) == 0 {
                continue;
            }
            
            let min_dist = collider.radius + other_collider.radius;
            let min_dist_sq = min_dist * min_dist;

            let delta = pos.0 - other_pos.0;
            let dist_sq = delta.length_squared();
            
            if dist_sq < min_dist_sq {
                colliding_entities.insert(entity);
                colliding_entities.insert(other_entity);
                
                let dist = dist_sq.sqrt();
                let overlap = min_dist - dist;
                let normal = if dist > sim_config.epsilon {
                    delta / dist
                } else {
                    // When entities are at exactly the same position, use entity IDs to generate
                    // a deterministic but different direction for each pair
                    let angle = ((entity.index() ^ other_entity.index()) as f32 * 0.618033988749895) * std::f32::consts::TAU;
                    let cos = FixedNum::from_num(angle.cos());
                    let sin = FixedNum::from_num(angle.sin());
                    FixedVec2::new(cos, sin)
                };

                events.write(CollisionEvent {
                    entity1: entity,
                    entity2: other_entity,
                    overlap,
                    normal,
                });
            }
        }
    }

    // Batch update collision states efficiently
    // Only mutate when state actually changes to avoid triggering change detection unnecessarily
    for (entity, _, _, mut collision_state) in query.iter_mut() {
        let is_colliding = colliding_entities.contains(&entity);
        if collision_state.is_colliding != is_colliding {
            collision_state.is_colliding = is_colliding;
        }
    }
}

// ============================================================================
// Collision Resolution
// ============================================================================

/// Resolve unit-unit collisions by applying repulsion forces
#[profile]
pub fn resolve_collisions(
    mut query: Query<&mut SimAcceleration>,
    sim_config: Res<SimConfig>,
    mut events: MessageReader<CollisionEvent>,
) {
    let repulsion_strength = sim_config.repulsion_force;
    let decay = sim_config.repulsion_decay;
    let max_overlap = FixedNum::from_num(10.0); // Cap overlap to prevent overflow
    
    for event in events.read() {
        // Apply repulsion force based on overlap
        // Force increases as overlap increases
        let capped_overlap = event.overlap.min(max_overlap);
        let force_mag = repulsion_strength * (FixedNum::ONE + capped_overlap * decay);
        let force = event.normal * force_mag;
        
        // Apply to entity 1
        if let Ok(mut acc1) = query.get_mut(event.entity1) {
            acc1.0 = acc1.0 + force;
        }
        
        // Apply to entity 2 (opposite direction)
        if let Ok(mut acc2) = query.get_mut(event.entity2) {
            acc2.0 = acc2.0 - force;
        }
    }
}


/// Resolve collisions between units and static obstacles
#[profile]
pub fn resolve_obstacle_collisions(
    mut units: Query<(Entity, &SimPosition, &mut SimAcceleration, &Collider), Without<StaticObstacle>>,
    obstacle_query: Query<(&SimPosition, &Collider), With<StaticObstacle>>,
    spatial_hash: Res<SpatialHash>,
    mut scratch: ResMut<SpatialHashScratch>,
    map_flow_field: Res<MapFlowField>,
    sim_config: Res<SimConfig>,
) {
    let repulsion_strength = sim_config.repulsion_force;
    let decay = sim_config.repulsion_decay;
    let flow_field = &map_flow_field.0;
    let obstacle_radius = flow_field.cell_size / FixedNum::from_num(2.0);
    
    for (entity, u_pos, mut u_acc, u_collider) in units.iter_mut() {
        let unit_radius = u_collider.radius;
        let min_dist = unit_radius + obstacle_radius;
        let min_dist_sq = min_dist * min_dist;

        if let Some((cx, cy)) = flow_field.world_to_grid(u_pos.0) {
            // Check 3x3 neighbors
            let range = sim_config.obstacle_search_range as usize;
            let min_x = if cx >= range { cx - range } else { 0 };
            let max_x = if cx + range < flow_field.width { cx + range } else { flow_field.width - 1 };
            let min_y = if cy >= range { cy - range } else { 0 };
            let max_y = if cy + range < flow_field.height { cy + range } else { flow_field.height - 1 };

            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    if flow_field.cost_field[flow_field.get_index(x, y)] == 255 {
                        let o_pos = flow_field.grid_to_world(x, y);
                        let delta = u_pos.0 - o_pos;
                        let dist_sq = delta.length_squared();
                        
                        if dist_sq < min_dist_sq {
                            if dist_sq > sim_config.epsilon {
                                let dist = dist_sq.sqrt();
                                let overlap = min_dist - dist;
                                let dir = delta / dist;

                                // Apply force
                                let force_mag = repulsion_strength * (FixedNum::ONE + overlap * decay);
                                u_acc.0 = u_acc.0 + dir * force_mag;
                            }
                        }
                    }
                }
            }
        }

        // Check free obstacles using spatial hash query
        let search_radius = unit_radius * sim_config.collision_search_radius_multiplier;
        spatial_hash.query_radius(u_pos.0, search_radius, Some(entity), &mut scratch);
        
        for &neighbor_entity in &scratch.query_results {
            // Check if this neighbor is a static obstacle
            let Ok((obs_pos, obs_collider)) = obstacle_query.get(neighbor_entity) else {
                continue;
            };
            
            let min_dist_free = unit_radius + obs_collider.radius;
            let min_dist_sq_free = min_dist_free * min_dist_free;

            let delta = u_pos.0 - obs_pos.0;
            let dist_sq = delta.length_squared();
            
            if dist_sq >= min_dist_sq_free || dist_sq <= sim_config.epsilon {
                continue;
            }
            
            let dist = dist_sq.sqrt();
            let overlap = min_dist_free - dist;
            let dir = delta / dist;

            // Apply force
            let force_mag = repulsion_strength * (FixedNum::ONE + overlap * decay);
            u_acc.0 = u_acc.0 + dir * force_mag;
        }
    }
}
