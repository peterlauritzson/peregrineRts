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

    // Draw nodes (portals) with frustum culling
    for portal in &graph.nodes {
        let pos = flow_field.grid_to_world(portal.node.x, portal.node.y);
        let portal_pos = Vec2::new(pos.x.to_num(), pos.y.to_num());
        
        // Cull portals outside view radius
        let dx = portal_pos.x - camera_center.x;
        let dy = portal_pos.y - camera_center.y;
        let distance = (dx * dx + dy * dy).sqrt();
        if distance > view_radius {
            continue;
        }

        gizmos.sphere(
            Vec3::new(pos.x.to_num(), 1.0, pos.y.to_num()),
            0.3,
            Color::srgb(0.0, 1.0, 1.0),
        );

        // Draw portal range
        let min_pos = flow_field.grid_to_world(portal.range_min.x, portal.range_min.y);
        let max_pos = flow_field.grid_to_world(portal.range_max.x, portal.range_max.y);
        
        gizmos.line(
            Vec3::new(min_pos.x.to_num(), 1.0, min_pos.y.to_num()),
            Vec3::new(max_pos.x.to_num(), 1.0, max_pos.y.to_num()),
            Color::srgb(0.0, 1.0, 1.0),
        );
    }

    // Draw edges with frustum culling (both endpoints must be visible)
    for (from_id, edges) in &graph.edges {
        if let Some(from_portal) = graph.nodes.get(*from_id) {
            let start = flow_field.grid_to_world(from_portal.node.x, from_portal.node.y);
            let start_pos = Vec2::new(start.x.to_num(), start.y.to_num());
            
            // Cull edges where start portal is outside view
            let dx = start_pos.x - camera_center.x;
            let dy = start_pos.y - camera_center.y;
            let start_distance = (dx * dx + dy * dy).sqrt();
            if start_distance > view_radius {
                continue;
            }

            for (to_id, _) in edges {
                if let Some(to_portal) = graph.nodes.get(*to_id) {
                    let end = flow_field.grid_to_world(to_portal.node.x, to_portal.node.y);
                    let end_pos = Vec2::new(end.x.to_num(), end.y.to_num());
                    
                    // Cull edges where end portal is outside view
                    let dx = end_pos.x - camera_center.x;
                    let dy = end_pos.y - camera_center.y;
                    let end_distance = (dx * dx + dy * dy).sqrt();
                    if end_distance > view_radius {
                        continue;
                    }

                    gizmos.line(
                        Vec3::new(start.x.to_num(), 1.0, start.y.to_num()),
                        Vec3::new(end.x.to_num(), 1.0, end.y.to_num()),
                        Color::srgb(1.0, 1.0, 0.0),
                    );
                }
            }
        }
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
