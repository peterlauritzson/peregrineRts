use crate::game::structures::FlowField;
use crate::game::fixed_math::{FixedVec2, FixedNum};
use super::types::{Region, Rect, CLUSTER_SIZE, MAX_REGIONS, RegionId, IslandId};
use smallvec::SmallVec;
use bevy::prelude::*;

/// Decompose a cluster into convex rectangular regions.
/// 
/// Algorithm: Maximal Rectangles
/// 1. Scan cluster row by row
/// 2. Merge walkable tiles into largest possible horizontal strips
/// 3. Merge vertical strips into rectangles
/// 4. Result: Array of rectangles covering all walkable space
///
/// Returns array of regions (typically 1-10 for normal terrain, up to 32 for complex areas)
pub(crate) fn decompose_cluster_into_regions(
    cluster_id: (usize, usize),
    flow_field: &FlowField,
) -> Vec<Region> {
    let (cx, cy) = cluster_id;
    let start_x = cx * CLUSTER_SIZE;
    let start_y = cy * CLUSTER_SIZE;
    let end_x = ((cx + 1) * CLUSTER_SIZE).min(flow_field.width);
    let end_y = ((cy + 1) * CLUSTER_SIZE).min(flow_field.height);
    
    if start_x >= flow_field.width || start_y >= flow_field.height {
        warn!("[DECOMP] Cluster {:?} out of bounds ({}, {}) >= ({}, {})", 
              cluster_id, start_x, start_y, flow_field.width, flow_field.height);
        return Vec::new();
    }
    
    // Find horizontal strips (consecutive walkable tiles in a row)
    let strips = find_horizontal_strips(start_x, end_x, start_y, end_y, flow_field);
    
    if strips.is_empty() {
        // No walkable tiles in this cluster
        return Vec::new();
    }
    
    // Merge strips vertically into rectangles
    let rectangles = merge_strips_into_rectangles(strips, start_x, start_y);
    
    if cluster_id.0 == 0 && cluster_id.1 == 0 {
        info!("[DECOMP] Cluster (0,0): Created {} rectangles from walkable tiles", rectangles.len());
    }
    
    // Convert rectangles to Region structs
    let mut regions = Vec::new();
    for (i, rect) in rectangles.into_iter().enumerate() {
        if i >= MAX_REGIONS {
            warn!("Cluster {:?} has more than {} regions, truncating", cluster_id, MAX_REGIONS);
            break;
        }
        
        let vertices = rect_to_vertices(rect);
        
        regions.push(Region {
            id: RegionId(i as u8),
            bounds: rect,
            vertices,
            island: IslandId(0), // Will be set during island detection
            portals: SmallVec::new(),
        });
    }
    
    regions
}

/// Find all horizontal strips of walkable tiles in the cluster
fn find_horizontal_strips(
    min_x: usize,
    max_x: usize,
    min_y: usize,
    max_y: usize,
    flow_field: &FlowField,
) -> Vec<(usize, usize, usize)> {
    let mut strips = Vec::new();
    
    for y in min_y..max_y {
        let mut strip_start: Option<usize> = None;
        
        for x in min_x..=max_x {
            let is_walkable = if x < max_x && y < flow_field.height {
                let idx = flow_field.get_index(x, y);
                if idx < flow_field.cost_field.len() {
                    // Walkable = any cost < 255 (obstacles are 255)
                    // This allows variable terrain costs: 1=normal, 2-254=slow terrain, 255=impassable
                    flow_field.cost_field[idx] < 255
                } else {
                    false
                }
            } else {
                false // End of row
            };
            
            match (strip_start, is_walkable) {
                (None, true) => {
                    // Start of a new strip
                    strip_start = Some(x);
                }
                (Some(start), false) => {
                    // End of current strip
                    strips.push((y, start, x - 1));
                    strip_start = None;
                }
                _ => {}
            }
        }
    }
    
    strips
}

/// Merge horizontal strips vertically into maximal rectangles
fn merge_strips_into_rectangles(strips: Vec<(usize, usize, usize)>, cluster_start_x: usize, cluster_start_y: usize) -> Vec<Rect> {
    let mut rectangles = Vec::new();
    let mut used = vec![false; strips.len()];
    
    for i in 0..strips.len() {
        if used[i] {
            continue;
        }
        
        let (y, x_start, x_end) = strips[i];
        let rect_y_min = y;
        let mut rect_y_max = y;
        
        // Try to extend this strip upward and downward
        // Look for strips with the same x-range
        for j in (i + 1)..strips.len() {
            if used[j] {
                continue;
            }
            
            let (other_y, other_x_start, other_x_end) = strips[j];
            
            // Check if this strip is adjacent and has same x-range
            if other_x_start == x_start && other_x_end == x_end {
                if other_y == rect_y_max + 1 {
                    rect_y_max = other_y;
                    used[j] = true;
                }
            }
        }
        
        used[i] = true;
        
        // Convert to cluster-local coordinates (relative to cluster origin)
        // Strips use absolute grid coordinates, so subtract cluster origin
        let local_x_start = x_start - cluster_start_x;
        let local_x_end = x_end - cluster_start_x;
        let local_y_min = rect_y_min - cluster_start_y;
        let local_y_max = rect_y_max - cluster_start_y;
        
        // Add 0.5 to represent tile centers
        let min_x = FixedNum::from_num(local_x_start) + FixedNum::from_num(0.5);
        let max_x = FixedNum::from_num(local_x_end) + FixedNum::from_num(0.5);
        let min_y = FixedNum::from_num(local_y_min) + FixedNum::from_num(0.5);
        let max_y = FixedNum::from_num(local_y_max) + FixedNum::from_num(0.5);
        
        rectangles.push(Rect::new(
            FixedVec2::new(min_x, min_y),
            FixedVec2::new(max_x, max_y),
        ));
    }
    
    rectangles
}

/// Convert a rectangle to its 4 corner vertices
fn rect_to_vertices(rect: Rect) -> SmallVec<[FixedVec2; 8]> {
    smallvec::smallvec![
        rect.min,                                    // Bottom-left
        FixedVec2::new(rect.max.x, rect.min.y),     // Bottom-right
        rect.max,                                    // Top-right
        FixedVec2::new(rect.min.x, rect.max.y),     // Top-left
    ]
}

/// Find which region contains a given point in CLUSTER-LOCAL coordinates
/// Point must be in the range [0, CLUSTER_SIZE] relative to the cluster origin
pub(crate) fn get_region_id(regions: &[Option<Region>], region_count: usize, point: FixedVec2) -> Option<RegionId> {
    for i in 0..region_count {
        if let Some(region) = &regions[i] {
            // Fast rejection test using bounding box
            if !region.bounds.contains(point) {
                continue;
            }
            
            // For rectangles (4 vertices), the bounds check is sufficient
            // For arbitrary convex polygons, we'd need point-in-polygon test
            if region.vertices.len() == 4 {
                return Some(region.id);
            }
            
            // Convex polygon test
            if is_point_in_convex_polygon(point, &region.vertices) {
                return Some(region.id);
            }
        }
    }
    None
}

/// Convert world position to cluster-local coordinates for region lookup
/// Returns coordinates in the range [0, CLUSTER_SIZE] relative to cluster origin
pub(crate) fn world_to_cluster_local(
    world_pos: FixedVec2,
    cluster_id: (usize, usize),
    flow_field: &crate::game::structures::FlowField,
) -> Option<FixedVec2> {
    // Convert world to grid coordinates
    let (gx, gy) = flow_field.world_to_grid(world_pos)?;
    
    // Get cluster origin in grid coordinates
    let cluster_origin_x = cluster_id.0 * CLUSTER_SIZE;
    let cluster_origin_y = cluster_id.1 * CLUSTER_SIZE;
    
    // Convert to cluster-local coordinates
    let local_x = (gx as isize - cluster_origin_x as isize) as f32 + 0.5;
    let local_y = (gy as isize - cluster_origin_y as isize) as f32 + 0.5;
    
    Some(FixedVec2::from_f32(local_x, local_y))
}

/// Test if a point is inside a convex polygon
/// Uses the property that point must be on the same side of all edges
fn is_point_in_convex_polygon(point: FixedVec2, vertices: &[FixedVec2]) -> bool {
    if vertices.len() < 3 {
        return false;
    }
    
    let mut sign: Option<bool> = None;
    
    for i in 0..vertices.len() {
        let v1 = vertices[i];
        let v2 = vertices[(i + 1) % vertices.len()];
        
        // Edge vector
        let edge = v2 - v1;
        // Vector from edge start to point
        let to_point = point - v1;
        
        // Cross product (2D): edge.x * to_point.y - edge.y * to_point.x
        let cross = edge.x * to_point.y - edge.y * to_point.x;
        
        let is_positive = cross >= FixedNum::ZERO;
        
        match sign {
            None => sign = Some(is_positive),
            Some(s) => {
                if s != is_positive {
                    return false; // Point is on different sides of different edges
                }
            }
        }
    }
    
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::pathfinding::types::Rect as PathRect;

    #[test]
    fn test_point_in_rectangle() {
        let min = FixedVec2::new(FixedNum::from_num(0), FixedNum::from_num(0));
        let max = FixedVec2::new(FixedNum::from_num(10), FixedNum::from_num(10));
        let rect = PathRect::new(min, max);
        
        assert!(rect.contains(FixedVec2::new(FixedNum::from_num(5), FixedNum::from_num(5))));
        assert!(!rect.contains(FixedVec2::new(FixedNum::from_num(15), FixedNum::from_num(5))));
    }
}
