/// Collision detection and resolution systems.
///
/// This module handles:
/// - Collision detection using cached neighbor lists
/// - Unit-unit collision resolution
/// - Unit-obstacle collision resolution

use bevy::prelude::*;
use crate::game::fixed_math::{FixedVec2, FixedNum};
use crate::game::spatial_hash::SpatialHash;
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
// Neighbor Cache Update Systems
// ============================================================================

/// Update cached neighbor lists for entities based on movement and velocity.
/// 
/// Uses velocity-aware caching: fast-moving entities update more frequently.
/// This dramatically reduces spatial hash queries (90%+ reduction).
pub fn update_neighbor_cache(
    mut query: Query<(Entity, &SimPosition, &SimVelocity, &mut CachedNeighbors, &Collider)>,
    spatial_hash: Res<SpatialHash>,
    sim_config: Res<SimConfig>,
    time: Res<Time<Fixed>>,
    _obstacles_query: Query<Entity, (With<StaticObstacle>, With<SimPosition>, With<Collider>)>,
    _all_entities: Query<(Entity, Option<&StaticObstacle>, Option<&SimPosition>, Option<&Collider>)>,
) {
    let start_time = std::time::Instant::now();
    
    // Thresholds for cache invalidation
    let fast_mover_speed_threshold = FixedNum::from_num(8.0); // units/sec
    let normal_update_threshold = FixedNum::from_num(0.5);    // units moved
    let fast_mover_update_threshold = FixedNum::from_num(0.2); // units moved
    const MAX_FRAMES_NORMAL: u32 = 10;  // Force refresh every 10 frames for slow movers
    const MAX_FRAMES_FAST: u32 = 2;      // Force refresh every 2 frames for fast movers
    
    let mut total_entities = 0;
    let mut cache_hits = 0;
    let mut cache_misses = 0;
    let mut fast_movers = 0;
    let mut total_obstacles_in_all_caches = 0;
    
    // Count total units for conditional logging
    let total_units = query.iter().count();
    
    for (entity, pos, velocity, mut cache, collider) in query.iter_mut() {
        total_entities += 1;
        cache.frames_since_update += 1;
        
        // Classify entity by speed
        let speed = velocity.0.length();
        cache.is_fast_mover = speed > fast_mover_speed_threshold;
        
        if cache.is_fast_mover {
            fast_movers += 1;
        }
        
        // Use different thresholds based on movement speed
        let (distance_threshold, max_frames) = if cache.is_fast_mover {
            (fast_mover_update_threshold, MAX_FRAMES_FAST)
        } else {
            (normal_update_threshold, MAX_FRAMES_NORMAL)
        };
        
        let moved_distance = (pos.0 - cache.last_query_pos).length();
        let needs_update = moved_distance > distance_threshold 
                        || cache.frames_since_update >= max_frames;
        
        if needs_update {
            // Cache MISS - perform full spatial query
            cache_misses += 1;
            let search_radius = collider.radius * sim_config.collision_search_radius_multiplier;
            
            // Use detailed logging version if we have very few units
            cache.neighbors = if total_units <= 5 {
                spatial_hash.get_potential_collisions(
                    pos.0, 
                    search_radius, 
                    Some(entity)
                )
            } else {
                spatial_hash.get_potential_collisions(
                    pos.0, 
                    search_radius, 
                    Some(entity)
                )
            };
            
            cache.last_query_pos = pos.0;
            cache.frames_since_update = 0;
        } else {
            // Cache HIT - reuse previous neighbor list
            cache_hits += 1;
        }
        
        // Count obstacles in this cache for debugging
        total_obstacles_in_all_caches += cache.neighbors.len();
    }
    
    let duration = start_time.elapsed();
    let tick = (time.elapsed_secs() * 30.0) as u64;
    
    if duration.as_millis() > 1 || tick % 100 == 0 {
        let cache_hit_rate = if total_entities > 0 {
            (cache_hits as f32 / total_entities as f32) * 100.0
        } else {
            0.0
        };
        
        let avg_neighbors = if total_entities > 0 {
            total_obstacles_in_all_caches as f32 / total_entities as f32
        } else {
            0.0
        };
        
        info!(
            "[NEIGHBOR_CACHE] {:?} | Entities: {} | Cache hits: {} ({:.1}%) | Misses: {} | Fast movers: {} | Avg neighbors/cache: {:.1}",
            duration, total_entities, cache_hits, cache_hit_rate, cache_misses, fast_movers, avg_neighbors
        );
    }
}

/// Update cached neighbor lists for boids steering.
/// Runs less frequently than collision cache (every 3-5 frames) since boids is visual-only.
///
/// # Performance Strategy
///
/// Current: Partial sort with `select_nth_unstable` - O(n) average case
/// - Finds actual closest N neighbors without sorting all
/// - Deterministic and unbiased
/// - Good for up to ~100 neighbors per query
///
/// # Alternative for extreme densities (commented below):
/// Deterministic sampling using entity ID hash
/// - O(n) but with lower constant factor (no comparisons)
/// - Unbiased if hash is good
/// - Trade: neighbors are "random nearby" not "closest nearby"
/// - Enable if profiling shows this function >5% of frame time
pub fn update_boids_neighbor_cache(
    mut query: Query<(Entity, &SimPosition, &SimVelocity, &mut BoidsNeighborCache)>,
    spatial_hash: Res<SpatialHash>,
    sim_config: Res<SimConfig>,
    all_units: Query<(&SimPosition, &SimVelocity)>,
    time: Res<Time<Fixed>>,
) {
    let start_time = std::time::Instant::now();
    
    // Boids can tolerate stale data - update every 3-5 frames depending on movement
    const MOVEMENT_THRESHOLD: f32 = 1.0;  // More lenient than collision (0.5)
    const MAX_FRAMES: u32 = 5;            // Slower than collision (2-10)
    
    let mut total_entities = 0;
    let mut cache_hits = 0;
    let mut cache_misses = 0;
    let mut total_neighbors = 0;
    
    for (entity, pos, _vel, mut cache) in query.iter_mut() {
        total_entities += 1;
        cache.frames_since_update += 1;
        
        let moved_distance = (pos.0 - cache.last_query_pos).length();
        let needs_update = moved_distance.to_num::<f32>() > MOVEMENT_THRESHOLD 
                        || cache.frames_since_update >= MAX_FRAMES;
        
        if needs_update {
            // Cache MISS - rebuild neighbor list
            cache_misses += 1;
            
            // Query spatial hash with boids neighbor radius (5.0 units)
            let nearby_entities = spatial_hash.query_radius(entity, pos.0, sim_config.neighbor_radius);
            
            // Get closest N neighbors efficiently using partial sort
            // Query SimPosition and SimVelocity components for each nearby entity
            let mut neighbors_with_dist: Vec<_> = nearby_entities.iter()
                .filter_map(|&neighbor_entity| {
                    if let Ok((neighbor_pos, neighbor_vel)) = all_units.get(neighbor_entity) {
                        let dist_sq = (pos.0 - neighbor_pos.0).length_squared();
                        Some((neighbor_entity, neighbor_pos.0, neighbor_vel.0, dist_sq))
                    } else {
                        None
                    }
                })
                .collect();
            
            // Use partial sort: O(n) average instead of O(n log n) full sort
            // Only partitions to find the top N closest, doesn't sort the rest
            let max_neighbors = sim_config.boids_max_neighbors.min(neighbors_with_dist.len());
            if max_neighbors > 0 && neighbors_with_dist.len() > max_neighbors {
                neighbors_with_dist.select_nth_unstable_by(
                    max_neighbors - 1,
                    |a, b| a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal)
                );
            }
            
            // Take only the closest N neighbors (they're now in the first N slots)
            cache.neighbors.clear();
            for (neighbor_entity, neighbor_pos, neighbor_vel, _dist_sq) in neighbors_with_dist.iter().take(max_neighbors) {
                cache.neighbors.push((*neighbor_entity, *neighbor_pos, *neighbor_vel));
            }
            
            // ALTERNATIVE: Deterministic sampling (faster for >100 neighbors, commented for reference)
            // Uncomment if profiling shows this function is bottleneck
            /*
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            
            cache.neighbors.clear();
            let target_count = sim_config.boids_max_neighbors.min(neighbors_with_dist.len());
            let skip_rate = if target_count > 0 {
                neighbors_with_dist.len() / target_count
            } else {
                1
            };
            
            // Deterministic shuffle using entity ID as seed
            let mut hasher = DefaultHasher::new();
            entity.index().hash(&mut hasher);
            let seed = hasher.finish() as usize;
            
            for (i, (neighbor_entity, neighbor_pos, neighbor_vel, _)) in neighbors_with_dist.iter().enumerate() {
                // Deterministic selection using hash
                let hash_idx = (i.wrapping_add(seed)) % neighbors_with_dist.len();
                if hash_idx % skip_rate == 0 && cache.neighbors.len() < target_count {
                    cache.neighbors.push((*neighbor_entity, *neighbor_pos, *neighbor_vel));
                }
                if cache.neighbors.len() >= target_count {
                    break;
                }
            }
            */
            
            total_neighbors += cache.neighbors.len();
            cache.last_query_pos = pos.0;
            cache.frames_since_update = 0;
        } else {
            // Cache HIT - reuse old neighbor list
            cache_hits += 1;
            total_neighbors += cache.neighbors.len();
        }
    }
    
    let duration = start_time.elapsed();
    let tick = (time.elapsed_secs() * 30.0) as u64;
    
    if duration.as_millis() > 1 || tick % 100 == 0 {
        let cache_hit_rate = if total_entities > 0 {
            (cache_hits as f32 / total_entities as f32) * 100.0
        } else {
            0.0
        };
        
        let avg_neighbors = if total_entities > 0 {
            total_neighbors as f32 / total_entities as f32
        } else {
            0.0
        };
        
        info!(
            "[BOIDS_CACHE] {:?} | Entities: {} | Cache hits: {} ({:.1}%) | Misses: {} | Avg neighbors: {:.1}",
            duration, total_entities, cache_hits, cache_hit_rate, cache_misses, avg_neighbors
        );
    }
}

// ============================================================================
// Collision Detection
// ============================================================================

/// Detect collisions between entities using cached neighbor lists
pub fn detect_collisions(
    mut commands: Commands,
    query: Query<(Entity, &SimPosition, &Collider, &CachedNeighbors)>,
    position_lookup: Query<(&SimPosition, &Collider)>,
    sim_config: Res<SimConfig>,
    mut events: MessageWriter<CollisionEvent>,
    time: Res<Time<Fixed>>,
) {
    let start_time = std::time::Instant::now();
    let mut colliding_entities = std::collections::HashSet::new();
    let total_entities = query.iter().count();
    let mut total_potential_checks = 0;
    let mut actual_collision_count = 0;
    let mut total_duplicate_skips = 0;
    let mut total_layer_filtered = 0;
    let mut max_neighbors_found = 0;
    let mut total_neighbors_found = 0;

    // Use cached neighbor lists instead of querying spatial hash
    // Cache is updated by update_neighbor_cache system which runs before this
    
    for (entity, pos, collider, cache) in query.iter() {
        // Use cached neighbor list (no spatial hash query needed!)
        let neighbors_count = cache.neighbors.len();
        total_potential_checks += neighbors_count;
        total_neighbors_found += neighbors_count;
        max_neighbors_found = max_neighbors_found.max(neighbors_count);
        
        for &other_entity in &cache.neighbors {
            if entity > other_entity { 
                total_duplicate_skips += 1;
                continue; 
            } // Avoid duplicates (self already excluded)
            
            // Get current position from SimPosition component (not cached)
            if let Ok((other_pos, other_collider)) = position_lookup.get(other_entity) {
                // Check layers
                if (collider.mask & other_collider.layer) == 0 && (other_collider.mask & collider.layer) == 0 {
                    total_layer_filtered += 1;
                    continue;
                }

                let min_dist = collider.radius + other_collider.radius;
                let min_dist_sq = min_dist * min_dist;

                let delta = pos.0 - other_pos.0;
                let dist_sq = delta.length_squared();
                
                if dist_sq < min_dist_sq {
                    colliding_entities.insert(entity);
                    colliding_entities.insert(other_entity);
                    actual_collision_count += 1;
                    
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
    }

    // Sync component state
    for (entity, _, _, _) in query.iter() {
        if colliding_entities.contains(&entity) {
            commands.entity(entity).insert(Colliding);
        } else {
            commands.entity(entity).remove::<Colliding>();
        }
    }
    
    let duration = start_time.elapsed();
    let tick = (time.elapsed_secs() * 30.0) as u64; // Assuming 30 tick rate
    
    // Log detailed metrics every 100 ticks or if collision detection is slow (> 5ms in release)
    let should_log = duration.as_millis() > 5 || tick % 100 == 0;
    
    if should_log {
        let avg_neighbors = if total_entities > 0 {
            total_neighbors_found as f32 / total_entities as f32
        } else {
            0.0
        };
        
        let useful_check_ratio = if total_potential_checks > 0 {
            (actual_collision_count as f32 / total_potential_checks as f32) * 100.0
        } else {
            0.0
        };
        
        info!(
            "[COLLISION_DETECT] {:?} | Entities: {} | Neighbors: {} (avg: {:.1}, max: {}) | \
             Potential checks: {} | Duplicate skips: {} | Layer filtered: {} | \
             Actual collisions: {} | Hit ratio: {:.2}% | Search radius multiplier: {:.1}x",
            duration,
            total_entities,
            total_neighbors_found,
            avg_neighbors,
            max_neighbors_found,
            total_potential_checks,
            total_duplicate_skips,
            total_layer_filtered,
            actual_collision_count,
            useful_check_ratio,
            sim_config.collision_search_radius_multiplier.to_num::<f32>()
        );
    }
}

// ============================================================================
// Collision Resolution
// ============================================================================

/// Resolve unit-unit collisions by applying repulsion forces
pub fn resolve_collisions(
    mut query: Query<&mut SimAcceleration>,
    sim_config: Res<SimConfig>,
    mut events: MessageReader<CollisionEvent>,
    time: Res<Time<Fixed>>,
) {
    let start_time = std::time::Instant::now();
    let repulsion_strength = sim_config.repulsion_force;
    let decay = sim_config.repulsion_decay;
    let mut event_count = 0;
    
    for event in events.read() {
        event_count += 1;
        // Apply repulsion force based on overlap
        // Force increases as overlap increases
        let force_mag = repulsion_strength * (FixedNum::ONE + event.overlap * decay);
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
    
    let duration = start_time.elapsed();
    let tick = (time.elapsed_secs() * 30.0) as u64;
    if duration.as_millis() > 2 || tick % 100 == 0 {
        info!("[COLLISION_RESOLVE] {:?} | Collision events processed: {}", duration, event_count);
    }
}

/// Resolve collisions between units and static obstacles
pub fn resolve_obstacle_collisions(
    mut units: Query<(Entity, &SimPosition, &mut SimAcceleration, &Collider, &CachedNeighbors), Without<StaticObstacle>>,
    obstacle_query: Query<(&SimPosition, &Collider), With<StaticObstacle>>,
    map_flow_field: Res<MapFlowField>,
    sim_config: Res<SimConfig>,
    time: Res<Time<Fixed>>,
) {
    let start_time = std::time::Instant::now();
    let repulsion_strength = sim_config.repulsion_force;
    let decay = sim_config.repulsion_decay;
    let flow_field = &map_flow_field.0;
    let obstacle_radius = flow_field.cell_size / FixedNum::from_num(2.0);
    let mut total_units = 0;
    let mut total_grid_checks = 0;
    let mut total_grid_collisions = 0;
    let mut total_free_obstacle_checks = 0;
    let mut total_free_obstacle_collisions = 0;
    let mut total_neighbors_checked = 0;
    let mut total_obstacle_query_matches = 0;
    
    for (_entity, u_pos, mut u_acc, u_collider, cache) in units.iter_mut() {
        total_units += 1;
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
                    total_grid_checks += 1;
                    if flow_field.cost_field[flow_field.get_index(x, y)] == 255 {
                        let o_pos = flow_field.grid_to_world(x, y);
                        let delta = u_pos.0 - o_pos;
                        let dist_sq = delta.length_squared();
                        
                        if dist_sq < min_dist_sq && dist_sq > sim_config.epsilon {
                            total_grid_collisions += 1;
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

        // Check free obstacles using cached neighbors (from spatial hash)
        // Obstacles are already in the spatial hash, so they appear in cached neighbors
        total_neighbors_checked += cache.neighbors.len();
        
        for &neighbor_entity in &cache.neighbors {
            // Check if this neighbor is a static obstacle
            let Ok((obs_pos, obs_collider)) = obstacle_query.get(neighbor_entity) else {
                continue;
            };
            
            total_obstacle_query_matches += 1;
            total_free_obstacle_checks += 1;
            let min_dist_free = unit_radius + obs_collider.radius;
            let min_dist_sq_free = min_dist_free * min_dist_free;

            let delta = u_pos.0 - obs_pos.0;
            let dist_sq = delta.length_squared();
            
            if dist_sq >= min_dist_sq_free || dist_sq <= sim_config.epsilon {
                continue;
            }
            
            total_free_obstacle_collisions += 1;
            let dist = dist_sq.sqrt();
            let overlap = min_dist_free - dist;
            let dir = delta / dist;

            // Apply force
            let force_mag = repulsion_strength * (FixedNum::ONE + overlap * decay);
            u_acc.0 = u_acc.0 + dir * force_mag;
        }
    }
    
    let duration = start_time.elapsed();
    let tick = (time.elapsed_secs() * 30.0) as u64;
    if duration.as_millis() > 2 || tick % 100 == 0 {
        let avg_grid_checks = if total_units > 0 { total_grid_checks as f32 / total_units as f32 } else { 0.0 };
        let avg_free_checks = if total_units > 0 { total_free_obstacle_checks as f32 / total_units as f32 } else { 0.0 };
        let avg_neighbors = if total_units > 0 { total_neighbors_checked as f32 / total_units as f32 } else { 0.0 };
        
        info!(
            "[OBSTACLE_RESOLVE] {:?} | Units: {} | Grid checks: {} (avg: {:.1}, collisions: {}) | \
             Cached neighbors checked: {} (avg: {:.1}) | Obstacles matched: {} | \
             Spatial obstacle checks: {} (avg: {:.1}, collisions: {}) | [Using cached neighbors]",
            duration, total_units, total_grid_checks, avg_grid_checks, total_grid_collisions,
            total_neighbors_checked, avg_neighbors, total_obstacle_query_matches,
            total_free_obstacle_checks, avg_free_checks, total_free_obstacle_collisions
        );
    }
}
