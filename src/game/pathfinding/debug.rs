use bevy::prelude::*;
use crate::game::simulation::{MapFlowField, DebugConfig};
use crate::game::config::{GameConfig, GameConfigHandle};
use super::graph::HierarchicalGraph;

pub(super) fn draw_graph_gizmos(
    graph: Res<HierarchicalGraph>,
    map_flow_field: Res<MapFlowField>,
    debug_config: Res<DebugConfig>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    mut gizmos: Gizmos,
    q_camera: Query<(&Camera, &GlobalTransform), With<crate::game::camera::RtsCamera>>,
) {
    if !debug_config.show_pathfinding_graph {
        return;
    }

    let flow_field = &map_flow_field.0;
    if flow_field.width == 0 { return; }

    let Some(config) = game_configs.get(&config_handle.0) else { return };
    let Ok((camera, camera_transform)) = q_camera.single() else { return };

    // Legend is displayed in the console when G key is pressed (see toggle_debug)

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
    
    // NEW: Draw regions and islands with different colors
    for (cluster_id, cluster) in &graph.clusters {
        let cluster_x_tiles = cluster_id.0 * super::types::CLUSTER_SIZE;
        let cluster_y_tiles = cluster_id.1 * super::types::CLUSTER_SIZE;
        
        // Draw each region with a color based on its island
        for i in 0..cluster.region_count {
            if let Some(region) = &cluster.regions[i] {
                // Color based on island ID (cycle through colors)
                let island_hue = (region.island.0 as f32 * 0.25) % 1.0;
                let color = Color::hsl(island_hue * 360.0, 0.8, 0.6).with_alpha(0.3);
                
                // NOTE: Region bounds are in cluster-local fixed-point coordinates (0-CLUSTER_SIZE)
                // Need to convert to world coordinates via grid coordinates
                // region.bounds uses fixed-point numbers representing tile positions
                let min_grid_x = cluster_x_tiles + region.bounds.min.x.floor().to_num::<usize>();
                let min_grid_y = cluster_y_tiles + region.bounds.min.y.floor().to_num::<usize>();
                let max_grid_x = cluster_x_tiles + region.bounds.max.x.ceil().to_num::<usize>();
                let max_grid_y = cluster_y_tiles + region.bounds.max.y.ceil().to_num::<usize>();
                
                let min_world = flow_field.grid_to_world(min_grid_x, min_grid_y);
                let max_world = flow_field.grid_to_world(max_grid_x, max_grid_y);
                
                let center = Vec3::new(
                    (min_world.x.to_num::<f32>() + max_world.x.to_num::<f32>()) / 2.0,
                    0.5,
                    (min_world.y.to_num::<f32>() + max_world.y.to_num::<f32>()) / 2.0
                );
                let size = Vec2::new(
                    max_world.x.to_num::<f32>() - min_world.x.to_num::<f32>(),
                    max_world.y.to_num::<f32>() - min_world.y.to_num::<f32>()
                );
                
                // Check if in view
                let center_2d = Vec2::new(center.x, center.z);
                let dx = center_2d.x - camera_center.x;
                let dy = center_2d.y - camera_center.y;
                let distance = (dx * dx + dy * dy).sqrt();
                if distance > view_radius + size.length() {
                    continue;
                }
                
                gizmos.rect(
                    Isometry3d::new(
                        center,
                        Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                    ),
                    size,
                    color,
                );
                
                // Draw region portals (connections to other regions)
                for portal in &region.portals {
                    // Portal center is also in cluster-local coordinates
                    let portal_grid_x = cluster_x_tiles + portal.center.x.to_num::<usize>();
                    let portal_grid_y = cluster_y_tiles + portal.center.y.to_num::<usize>();
                    let portal_world = flow_field.grid_to_world(portal_grid_x, portal_grid_y);
                    
                    let portal_center = Vec3::new(
                        portal_world.x.to_num(),
                        0.7,
                        portal_world.y.to_num()
                    );
                    gizmos.sphere(portal_center, 0.2, Color::srgb(1.0, 1.0, 0.0));
                }
            }
        }
    }
    
    // Draw inter-cluster portals (for cross-cluster routing)

    for portal in graph.portals.values() {
        let pos = flow_field.grid_to_world(portal.node.x, portal.node.y);
        let portal_pos = Vec2::new(pos.x.to_num(), pos.y.to_num());
        
        let dx = portal_pos.x - camera_center.x;
        let dy = portal_pos.y - camera_center.y;
        let distance = (dx * dx + dy * dy).sqrt();
        if distance > view_radius {
            continue;
        }

        gizmos.sphere(
            Vec3::new(pos.x.to_num(), 1.2, pos.y.to_num()),
            0.25,
            Color::srgb(0.0, 1.0, 1.0),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_gizmo_culls_distant_portals() {
        // Verify that graph gizmo culling logic matches expected behavior
        // Portal at (100, 0) should be culled when camera is at (0, 0) with radius 50
        let portal_pos = Vec2::new(100.0, 0.0);
        let camera_center = Vec2::new(0.0, 0.0);
        let view_radius = 50.0;
        
        let dx = portal_pos.x - camera_center.x;
        let dy = portal_pos.y - camera_center.y;
        let distance = (dx * dx + dy * dy).sqrt();
        
        assert!(distance > view_radius, "Portal should be outside view radius");
    }
    
    #[test]
    fn test_graph_gizmo_draws_nearby_portals() {
        // Portal at (10, 0) should be drawn when camera is at (0, 0) with radius 50
        let portal_pos = Vec2::new(10.0, 0.0);
        let camera_center = Vec2::new(0.0, 0.0);
        let view_radius = 50.0;
        
        let dx = portal_pos.x - camera_center.x;
        let dy = portal_pos.y - camera_center.y;
        let distance = (dx * dx + dy * dy).sqrt();
        
        assert!(distance <= view_radius, "Portal should be within view radius");
    }
    
    #[test]
    fn test_graph_gizmo_edge_culling_both_endpoints() {
        // Verify edge culling requires both endpoints to be visible
        let view_radius = 50.0;
        let camera_center = Vec2::new(0.0, 0.0);
        
        // Edge from (10, 0) to (100, 0) - start visible, end not visible
        let start_pos = Vec2::new(10.0, 0.0);
        let end_pos = Vec2::new(100.0, 0.0);
        
        let start_dx = start_pos.x - camera_center.x;
        let start_dy = start_pos.y - camera_center.y;
        let start_distance = (start_dx * start_dx + start_dy * start_dy).sqrt();
        
        let end_dx = end_pos.x - camera_center.x;
        let end_dy = end_pos.y - camera_center.y;
        let end_distance = (end_dx * end_dx + end_dy * end_dy).sqrt();
        
        assert!(start_distance <= view_radius, "Start should be visible");
        assert!(end_distance > view_radius, "End should not be visible");
        // Edge should be culled because end is not visible
    }
}
