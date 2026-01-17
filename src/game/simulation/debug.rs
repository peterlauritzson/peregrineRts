/// Debug visualization systems for the simulation.
///
/// This module handles all debug rendering including:
/// - Path visualization for selected units
/// - Force source visualization

use bevy::prelude::*;
use crate::game::fixed_math::{FixedVec2, FixedNum};
use crate::game::config::{GameConfig, GameConfigHandle};
use crate::game::pathfinding::{Path, HierarchicalGraph};
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
    selected_query: Query<&Path, With<crate::game::unit::Selected>>,
) {
    let Some(config) = game_configs.get(&config_handle.0) else { return };

    if keyboard.just_pressed(config.key_debug_graph) {
        debug_config.show_pathfinding_graph = !debug_config.show_pathfinding_graph;
        if debug_config.show_pathfinding_graph {
            info!("Pathfinding graph debug ENABLED");
            info!("  Graph initialized: {}", graph.initialized);
            { 
                let stats = graph.get_stats();
                info!("  Total regions: {} in {} clusters ({} islands)", 
                      stats.region_count, stats.cluster_count, stats.island_count);
            }
            info!("  Total clusters: {}", graph.clusters.len());
            info!("\n=== LEGEND ===");
            info!("  Colored rectangles = Regions (convex navigable areas)");
            info!("  Same color = Same island (connected regions)");
            info!("  Yellow spheres = Region portals (within cluster)");
            info!("  Cyan spheres = Cluster portals (between clusters)");
            info!("  Each region uses a hue based on its island ID");
        } else {
            info!("Pathfinding graph debug disabled");
        }
    }
    if keyboard.just_pressed(config.key_debug_path) {
        debug_config.show_paths = !debug_config.show_paths;
        if debug_config.show_paths {
            let selected_count = selected_query.iter().count();
            let with_paths = selected_query.iter().count();
            info!("Path visualization ENABLED");
            info!("  Selected units: {} ({} with paths)", selected_count, with_paths);
            info!("  TIP: Select units (drag) and give move order (right-click) to see paths");
        } else {
            info!("Path visualization disabled");
        }
    }
}

// ============================================================================
// Path Visualization
// ============================================================================

/// Draw paths for selected units only
pub fn draw_unit_paths(
    query: Query<(&Transform, &Path), With<crate::game::unit::Selected>>,
    debug_config: Res<DebugConfig>,
    map_flow_field: Res<MapFlowField>,
    graph: Res<crate::game::pathfinding::HierarchicalGraph>,
    mut gizmos: Gizmos,
) {
    if !debug_config.show_paths {
        return;
    }
    
    if !graph.initialized {
        warn!("[PATH DEBUG] Cannot draw paths - graph not initialized");
        return;
    }

    let selected_count = query.iter().count();
    if selected_count == 0 {
        // Only warn once to avoid spam
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
            Path::Hierarchical { goal, goal_cluster, goal_island } => {
                // Draw path as: unit -> portal -> portal -> ... -> goal
                use crate::game::pathfinding::{get_region_id, ClusterIslandId, CLUSTER_SIZE, world_to_cluster_local};
                
                let flow_field = &map_flow_field.0;
                let unit_pos = FixedVec2::from_f32(transform.translation.x, transform.translation.z);
                
                if let Some((gx, gy)) = flow_field.world_to_grid(unit_pos) {
                    let current_cluster = (gx / CLUSTER_SIZE, gy / CLUSTER_SIZE);
                    
                    if current_cluster == *goal_cluster {
                        // Same cluster - check if same region
                        if let Some(current_cluster_data) = graph.clusters.get(&current_cluster) {
                            // Convert to cluster-local coordinates
                            let curr_reg = world_to_cluster_local(unit_pos, current_cluster, flow_field)
                                .and_then(|local_pos| get_region_id(&current_cluster_data.regions, current_cluster_data.region_count, local_pos));
                            let goal_reg = world_to_cluster_local(*goal, current_cluster, flow_field)
                                .and_then(|local_goal| get_region_id(&current_cluster_data.regions, current_cluster_data.region_count, local_goal));
                            
                            if let (Some(curr_reg), Some(goal_reg)) = (curr_reg, goal_reg) {
                                    if curr_reg == goal_reg {
                                        // Same region - direct movement
                                        let goal_pos_3d = Vec3::new(goal.x.to_num(), 0.5, goal.y.to_num());
                                        gizmos.line(current_pos, goal_pos_3d, Color::srgb(0.0, 1.0, 0.0));
                                        gizmos.sphere(goal_pos_3d, 0.3, Color::srgb(0.0, 1.0, 0.0));
                                    } else {
                                        // Different region, same cluster - use local routing
                                        let portal_id: u8 = current_cluster_data.local_routing[curr_reg.0 as usize][goal_reg.0 as usize];
                                        if portal_id != crate::game::pathfinding::NO_PATH {
                                            if let Some(region) = &current_cluster_data.regions[curr_reg.0 as usize] {
                                                if let Some(portal) = region.portals.get(portal_id as usize) {
                                                    let portal_pos_3d = Vec3::new(portal.center.x.to_num(), 0.5, portal.center.y.to_num());
                                                    gizmos.line(current_pos, portal_pos_3d, Color::srgb(0.0, 0.5, 1.0));
                                                    gizmos.sphere(portal_pos_3d, 0.2, Color::srgb(0.0, 0.7, 1.0));
                                                    current_pos = portal_pos_3d;
                                                }
                                            }
                                        }
                                        let goal_pos_3d = Vec3::new(goal.x.to_num(), 0.5, goal.y.to_num());
                                        gizmos.line(current_pos, goal_pos_3d, Color::srgb(0.0, 1.0, 0.0));
                                        gizmos.sphere(goal_pos_3d, 0.3, Color::srgb(0.0, 1.0, 0.0));
                                    }
                                    continue;
                            }
                        }
                    }
                    
                    // Different cluster - use island routing to find portal sequence
                    if let Some(current_cluster_data) = graph.clusters.get(&current_cluster) {
                        // Convert to cluster-local coordinates
                        if let Some(curr_reg) = world_to_cluster_local(unit_pos, current_cluster, flow_field)
                            .and_then(|local_pos| get_region_id(&current_cluster_data.regions, current_cluster_data.region_count, local_pos)) {
                            let current_island = current_cluster_data.regions[curr_reg.0 as usize]
                                .as_ref()
                                .map(|r| r.island)
                                .unwrap_or(crate::game::pathfinding::IslandId(0));
                            
                            let from_island_id = ClusterIslandId::new(current_cluster, current_island);
                            let to_island_id = ClusterIslandId::new(*goal_cluster, *goal_island);
                            
                            // Collect all portals in the path
                            let mut portal_sequence = Vec::new();
                            let mut visited = std::collections::HashSet::new();
                            let mut current_island_id = from_island_id;
                            
                            while current_island_id != to_island_id && !visited.contains(&current_island_id) && portal_sequence.len() < 20 {
                                visited.insert(current_island_id);
                                
                                if let Some(next_portal_id) = graph.get_next_portal_for_island(current_island_id, to_island_id) {
                                    portal_sequence.push(next_portal_id);
                                    
                                    // Determine the island ID on the other side of this portal

                                    if let Some(portal) = graph.portals.get(&next_portal_id) {
                                        let next_cluster = portal.cluster;
                                        // For simplification, just assume we reach the correct island
                                        // In reality we'd need to query which island contains the portal endpoint
                                            if next_cluster == to_island_id.cluster {
                                            current_island_id = to_island_id;
                                        } else {
                                            // Move to next cluster with island 0 as approximation
                                            current_island_id = ClusterIslandId::new(next_cluster, crate::game::pathfinding::IslandId(0));
                                        }
                                    } else {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                            
                            // Draw the path through all portals
                            for portal_id in portal_sequence {

                                if let Some(portal) = graph.portals.get(&portal_id) {
                                    let portal_pos = flow_field.grid_to_world(portal.node.x, portal.node.y);
                                    let portal_pos_3d = Vec3::new(portal_pos.x.to_num(), 0.6, portal_pos.y.to_num());
                                    
                                    gizmos.line(current_pos, portal_pos_3d, Color::srgb(1.0, 0.5, 0.0));
                                    gizmos.sphere(portal_pos_3d, 0.3, Color::srgb(1.0, 0.7, 0.0));
                                    current_pos = portal_pos_3d;
                                }
                            }
                            
                            // Final line to goal
                            let goal_pos_3d = Vec3::new(goal.x.to_num(), 0.5, goal.y.to_num());
                            gizmos.line(current_pos, goal_pos_3d, Color::srgb(0.0, 1.0, 0.0));
                            gizmos.sphere(goal_pos_3d, 0.3, Color::srgb(0.0, 1.0, 0.0));
                        }
                    }
                }
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
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    mut gizmos: Gizmos,
    q_camera: Query<(&Camera, &GlobalTransform), With<crate::game::camera::RtsCamera>>,
) {
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

    let view_radius = config.debug_view_radius;
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
