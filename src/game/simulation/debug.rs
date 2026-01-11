/// Debug visualization systems for the simulation.
///
/// This module handles all debug rendering including:
/// - Flow field visualization
/// - Path visualization for selected units
/// - Force source visualization

use bevy::prelude::*;
use crate::game::fixed_math::{FixedVec2, FixedNum};
use crate::game::config::{GameConfig, GameConfigHandle};
use crate::game::pathfinding::{Path, HierarchicalGraph, CLUSTER_SIZE};
use crate::game::structures::CELL_SIZE;
use super::components::ForceSource;
use super::resources::{DebugConfig, MapFlowField};

// ============================================================================
// Debug Toggle
// ============================================================================

/// Toggle debug visualization modes with keyboard
pub fn toggle_debug(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut debug_config: ResMut<DebugConfig>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    graph: Res<HierarchicalGraph>,
    map_flow_field: Res<MapFlowField>,
) {
    let Some(config) = game_configs.get(&config_handle.0) else { return };

    if keyboard.just_pressed(config.key_debug_flow) {
        debug_config.show_flow_field = !debug_config.show_flow_field;
        if debug_config.show_flow_field {
            info!("Flow field debug ENABLED - rendering flow field gizmos");
            info!("  Graph initialized: {}", graph.initialized);
            info!("  Graph clusters: {}", graph.clusters.len());
            info!("  Flow field size: {}x{}", map_flow_field.0.width, map_flow_field.0.height);
        } else {
            info!("Flow field debug disabled");
        }
    }
    if keyboard.just_pressed(config.key_debug_graph) {
        debug_config.show_pathfinding_graph = !debug_config.show_pathfinding_graph;
        if debug_config.show_pathfinding_graph {
            info!("Pathfinding graph debug ENABLED");
            info!("  Graph initialized: {}", graph.initialized);
            info!("  Total portals: {}", graph.nodes.len());
            info!("  Total clusters: {}", graph.clusters.len());
        } else {
            info!("Pathfinding graph debug disabled");
        }
    }
    if keyboard.just_pressed(config.key_debug_path) {
        debug_config.show_paths = !debug_config.show_paths;
        info!("Path debug: {}", debug_config.show_paths);
    }
}

// ============================================================================
// Flow Field Visualization
// ============================================================================

/// Draw flow field arrows for visible clusters
pub fn draw_flow_field_gizmos(
    graph: Res<HierarchicalGraph>,
    map_flow_field: Res<MapFlowField>,
    debug_config: Res<DebugConfig>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    mut gizmos: Gizmos,
    q_camera: Query<(&Camera, &GlobalTransform), With<crate::game::camera::RtsCamera>>,
) {
    if !debug_config.show_flow_field {
        return;
    }
    
    if !graph.initialized {
        warn!("[DEBUG] Cannot draw flow field - graph not initialized!");
        return;
    }
    
    if graph.clusters.is_empty() {
        warn!("[DEBUG] Cannot draw flow field - no clusters in graph!");
        return;
    }

    let Some(config) = game_configs.get(&config_handle.0) else { return };

    let Ok((camera, camera_transform)) = q_camera.single() else { return };
    let flow_field = &map_flow_field.0;

    // Get camera view bounds roughly
    let camera_pos = camera_transform.translation();
    // Simple distance check for now. 
    // A better way would be to project the frustum to the ground plane.
    // But for debug gizmos, a radius around the camera look-at point is fine.
    
    // Raycast to ground to find center of view
    let center_pos = if let Ok(ray) = camera.viewport_to_world(camera_transform, Vec2::new(640.0, 360.0)) { // Center of screen approx
         if ray.direction.y.abs() > 0.001 {
             let t = -ray.origin.y / ray.direction.y;
             if t >= 0.0 {
                 ray.origin + ray.direction * t
             } else {
                 camera_pos
             }
         } else {
             camera_pos
         }
    } else {
        camera_pos
    };

    let view_radius = config.debug_flow_field_view_radius;

    for ((cx, cy), cluster) in &graph.clusters {
        let min_x = cx * CLUSTER_SIZE;
        let min_y = cy * CLUSTER_SIZE;
        
        let center_x = flow_field.origin.x.to_num::<f32>() + (min_x as f32 + CLUSTER_SIZE as f32 / 2.0) * CELL_SIZE;
        let center_y = flow_field.origin.y.to_num::<f32>() + (min_y as f32 + CLUSTER_SIZE as f32 / 2.0) * CELL_SIZE;
        
        let cluster_center = Vec2::new(center_x, center_y);
        let camera_center = Vec2::new(center_pos.x, center_pos.z);
        
        // Use helper function for culling and LOD
        let (should_draw, step) = should_draw_cluster(cluster_center, camera_center, view_radius);
        if !should_draw {
            continue;
        }

        // Debug: Draw a box around active clusters with cached fields
        if !cluster.flow_field_cache.is_empty() {
             gizmos.rect(
                 Isometry3d::new(
                     Vec3::new(center_x, 0.6, center_y),
                     Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                 ),
                 Vec2::new(CLUSTER_SIZE as f32 * CELL_SIZE, CLUSTER_SIZE as f32 * CELL_SIZE),
                 Color::srgb(1.0, 0.5, 0.0).with_alpha(0.3),
             );
        }

        for local_field in cluster.flow_field_cache.values() {
            for ly in (0..local_field.height).step_by(step) {
                for lx in (0..local_field.width).step_by(step) {
                    let idx = ly * local_field.width + lx;
                    if idx < local_field.vectors.len() {
                        let vec = local_field.vectors[idx];
                        if vec != FixedVec2::ZERO {
                            let gx = min_x + lx;
                            let gy = min_y + ly;
                            
                            let start = flow_field.grid_to_world(gx, gy).to_vec2();
                            
                            // Check individual arrow distance if needed, but cluster check is usually enough
                            
                            let end = start + vec.to_vec2() * 0.4; // Scale for visibility

                            gizmos.arrow(
                                Vec3::new(start.x, 0.6, start.y),
                                Vec3::new(end.x, 0.6, end.y),
                                Color::srgb(0.5, 0.5, 1.0), // Light blue for flow vectors
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Helper: Check if a cluster should be drawn based on camera view
/// Returns (should_draw, lod_step)
/// - should_draw: true if cluster is within view radius
/// - lod_step: 1 for close, 2 for medium, 4 for far distance
fn should_draw_cluster(cluster_center: Vec2, camera_center: Vec2, view_radius: f32) -> (bool, usize) {
    let dx = cluster_center.x - camera_center.x;
    let dy = cluster_center.y - camera_center.y;
    let distance = (dx * dx + dy * dy).sqrt();
    
    // Skip clusters outside view radius (with cluster size padding)
    let cluster_padding = CLUSTER_SIZE as f32 * CELL_SIZE;
    if distance > (view_radius + cluster_padding) {
        return (false, 1);
    }
    
    // LOD: Reduce arrow density based on camera distance
    let lod_step = if distance < 20.0 {
        1 // Show all arrows when close
    } else if distance < 40.0 {
        2 // Show half the arrows at medium distance
    } else {
        4 // Show quarter of arrows when far
    };
    
    (true, lod_step)
}

// ============================================================================
// Path Visualization
// ============================================================================

/// Draw paths for selected units only
pub fn draw_unit_paths(
    query: Query<(&Transform, &Path), With<crate::game::unit::Selected>>,
    debug_config: Res<DebugConfig>,
    mut gizmos: Gizmos,
) {
    if !debug_config.show_paths {
        return;
    }

    // Only visualize paths for selected units to avoid performance issues
    // With 10K units, drawing all paths would be 10K * max_steps iterations per frame
    for (transform, path) in query.iter() {
        let mut current_pos = transform.translation;
        current_pos.y = 0.6;

        match path {
            Path::Direct(target) => {
                let next_pos = Vec3::new(target.x.to_num(), 0.6, target.y.to_num());
                gizmos.line(current_pos, next_pos, Color::srgb(0.0, 1.0, 0.0));
                gizmos.sphere(next_pos, 0.2, Color::srgb(0.0, 1.0, 0.0));
            },
            Path::LocalAStar { waypoints, current_index } => {
                if *current_index >= waypoints.len() { continue; }
                for i in *current_index..waypoints.len() {
                    let wp = waypoints[i];
                    let next_pos = Vec3::new(wp.x.to_num(), 0.6, wp.y.to_num());
                    gizmos.line(current_pos, next_pos, Color::srgb(0.0, 1.0, 0.0));
                    gizmos.sphere(next_pos, 0.2, Color::srgb(0.0, 1.0, 0.0));
                    current_pos = next_pos;
                }
            },
            Path::Hierarchical { goal, goal_cluster: _ } => {
                // Just draw a line to the goal since we don't store portal list
                let goal_pos_3d = Vec3::new(
                    goal.x.to_num(),
                    0.5,
                    goal.y.to_num()
                );
                gizmos.line(current_pos, goal_pos_3d, Color::srgb(0.0, 1.0, 1.0));
                gizmos.sphere(goal_pos_3d, 0.3, Color::srgb(0.0, 1.0, 1.0));
            }
        }
    }
}

// ============================================================================
// Force Source Visualization
// ============================================================================

/// Draw force sources (black holes, wind spots, etc.)
pub fn draw_force_sources(
    query: Query<(&Transform, &ForceSource)>,
    debug_config: Res<DebugConfig>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    mut gizmos: Gizmos,
    q_camera: Query<(&Camera, &GlobalTransform), With<crate::game::camera::RtsCamera>>,
) {
    // Early exit if debug visualization is disabled
    if !debug_config.show_flow_field {
        return;
    }

    let Some(config) = game_configs.get(&config_handle.0) else { return };
    let Ok((camera, camera_transform)) = q_camera.single() else { return };

    // Get camera view center (raycast to ground)
    let camera_pos = camera_transform.translation();
    let center_pos = if let Ok(ray) = camera.viewport_to_world(camera_transform, Vec2::new(640.0, 360.0)) {
        if ray.direction.y.abs() > 0.001 {
            let t = -ray.origin.y / ray.direction.y;
            if t >= 0.0 {
                ray.origin + ray.direction * t
            } else {
                camera_pos
            }
        } else {
            camera_pos
        }
    } else {
        camera_pos
    };

    let view_radius = config.debug_flow_field_view_radius;
    let camera_center = Vec2::new(center_pos.x, center_pos.z);

    for (transform, source) in query.iter() {
        let source_pos = Vec2::new(transform.translation.x, transform.translation.z);
        
        // Cull force sources outside view radius
        let dx = source_pos.x - camera_center.x;
        let dy = source_pos.y - camera_center.y;
        let distance = (dx * dx + dy * dy).sqrt();
        if distance > view_radius {
            continue;
        }

        let color = match source.force_type {
            super::components::ForceType::Radial(strength) => {
                if strength > FixedNum::ZERO {
                    Color::srgb(0.5, 0.0, 0.5) // Purple for Black Hole
                } else {
                    Color::srgb(0.0, 1.0, 1.0) // Cyan for Wind
                }
            },
            super::components::ForceType::Directional(_) => Color::srgb(1.0, 1.0, 0.0),
        };
        
        let radius = source.radius.to_num::<f32>();
        gizmos.circle(transform.translation, radius, color);
        // Draw a smaller inner circle to indicate center
        gizmos.circle(transform.translation, 0.5, color);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flow_field_gizmo_respects_view_radius() {
        // Test that clusters outside view radius are culled
        let cluster_center = Vec2::new(100.0, 0.0);
        let camera_center = Vec2::new(0.0, 0.0);
        let view_radius = 50.0;
        
        let (should_draw, _) = should_draw_cluster(cluster_center, camera_center, view_radius);
        
        // Cluster at distance 100 should NOT be drawn with radius 50
        // (even with padding, CLUSTER_SIZE * CELL_SIZE is only ~25)
        assert!(!should_draw, "Cluster far outside view radius should not be drawn");
    }
    
    #[test]
    fn test_flow_field_gizmo_draws_nearby_clusters() {
        // Test that clusters within view radius are drawn
        let cluster_center = Vec2::new(10.0, 0.0);
        let camera_center = Vec2::new(0.0, 0.0);
        let view_radius = 50.0;
        
        let (should_draw, _) = should_draw_cluster(cluster_center, camera_center, view_radius);
        
        assert!(should_draw, "Cluster within view radius should be drawn");
    }
    
    #[test]
    fn test_flow_field_lod_step_at_close_distance() {
        // Test LOD: close distance should show all arrows (step = 1)
        let cluster_center = Vec2::new(10.0, 0.0);
        let camera_center = Vec2::new(0.0, 0.0);
        let view_radius = 50.0;
        
        let (should_draw, step) = should_draw_cluster(cluster_center, camera_center, view_radius);
        
        assert!(should_draw);
        assert_eq!(step, 1, "Close clusters should show all arrows (step=1)");
    }
    
    #[test]
    fn test_flow_field_lod_step_at_medium_distance() {
        // Test LOD: medium distance should show every 2nd arrow (step = 2)
        let cluster_center = Vec2::new(25.0, 0.0);
        let camera_center = Vec2::new(0.0, 0.0);
        let view_radius = 50.0;
        
        let (should_draw, step) = should_draw_cluster(cluster_center, camera_center, view_radius);
        
        assert!(should_draw);
        assert_eq!(step, 2, "Medium distance clusters should show every 2nd arrow (step=2)");
    }
    
    #[test]
    fn test_flow_field_lod_step_at_far_distance() {
        // Test LOD: far distance should show every 4th arrow (step = 4)
        let cluster_center = Vec2::new(45.0, 0.0);
        let camera_center = Vec2::new(0.0, 0.0);
        let view_radius = 50.0;
        
        let (should_draw, step) = should_draw_cluster(cluster_center, camera_center, view_radius);
        
        assert!(should_draw);
        assert_eq!(step, 4, "Far distance clusters should show every 4th arrow (step=4)");
    }
}
