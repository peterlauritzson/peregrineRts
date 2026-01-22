use crate::game::structures::FlowField;
use crate::game::fixed_math::{FixedVec2, FixedNum};
use super::types::{Region, Rect, CLUSTER_SIZE, MAX_REGIONS, RegionId, IslandId, ClusterId};
use super::graph::HierarchicalGraph;
use smallvec::SmallVec;
use bevy::prelude::*;

/// Decompose a cluster into convex rectangular regions.
/// 
/// Algorithm: Maximal Rectangles with Obstacle Dilation
/// 1. Dilate obstacles by 1-2 tiles to reduce fragmentation
/// 2. Scan cluster row by row
/// 3. Merge walkable tiles into largest possible horizontal strips
/// 4. Merge vertical strips into rectangles
/// 5. Result: Array of rectangles covering all walkable space
///
/// **Obstacle Dilation:** Treats obstacles as 1-2 tiles larger for pathfinding.
/// This dramatically reduces region count for circular obstacles (60-80% reduction).
/// Actual collision detection still uses real obstacle bounds.
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
    
    // OPTIMIZATION: Apply obstacle dilation to reduce fragmentation
    // This fills small gaps and rounds off circular obstacles
    const DILATION_RADIUS: usize = 1; // 1-2 tiles recommended
    
    // Find horizontal strips (consecutive walkable tiles in a row)
    let strips = find_horizontal_strips_with_dilation(
        start_x, end_x, start_y, end_y, flow_field, DILATION_RADIUS
    );
    
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
        
        // Check if region is convex (all rectangles from our algorithm are axis-aligned, so convex)
        // Mark as dangerous if: non-rectangular shape, or very small/thin (could be artifact)
        let is_dangerous = !is_convex_region(&vertices, &rect);
        
        regions.push(Region {
            id: RegionId(i as u8),
            bounds: rect,
            // NOLINT: MEMORY_OK: setup only - graph building precomputation
            vertices: vertices.clone(),
            island: IslandId(0), // Will be set during island detection
            portals: SmallVec::new(),
            is_dangerous,
        });
    }
    
    regions
}

/// Find all horizontal strips of walkable tiles in the cluster WITH OBSTACLE DILATION
/// 
/// Dilation expands obstacles by `dilation_radius` tiles to reduce fragmentation.
/// Treats a tile as obstacle if any tile within dilation_radius is an obstacle.
fn find_horizontal_strips_with_dilation(
    min_x: usize,
    max_x: usize,
    min_y: usize,
    max_y: usize,
    flow_field: &FlowField,
    dilation_radius: usize,
) -> Vec<(usize, usize, usize)> {
    let mut strips = Vec::new();
    
    for y in min_y..max_y {
        let mut strip_start: Option<usize> = None;
        
        for x in min_x..=max_x {
            // Check if this tile is walkable AFTER dilation
            let is_walkable_dilated = if x < max_x && y < flow_field.height {
                is_tile_walkable_dilated(x, y, flow_field, dilation_radius)
            } else {
                false // End of row
            };
            
            match (strip_start, is_walkable_dilated) {
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

// ============================================================================
// PERF: Fast Point Containment Checks
// ============================================================================

/// Convert world position to cluster-local grid coordinates
/// Check if a world position is inside a cluster (O(1) rectangle check).
/// Returns false if cluster doesn't exist or point is outside cluster bounds.
/// 
/// # Arguments
/// * `pos` - World position to check
/// * `cluster_id` - Cluster to check against
/// * `graph` - Hierarchical graph (for validation)
/// * `flow_field` - Flow field (for world<->grid conversion)
pub(crate) fn point_in_cluster(
    pos: FixedVec2,
    cluster_id: ClusterId,
    graph: &HierarchicalGraph,
    flow_field: &FlowField,
) -> bool {
    let (cx, cy) = cluster_id.as_tuple();
    
    // Bounds check: cluster must exist
    if cx >= graph.cluster_cols || cy >= graph.cluster_rows {
        return false;
    }
    
    // Calculate cluster world bounds
    let cluster_grid_min_x = cx * CLUSTER_SIZE;
    let cluster_grid_min_y = cy * CLUSTER_SIZE;
    let cluster_grid_max_x = ((cx + 1) * CLUSTER_SIZE).min(flow_field.width);
    let cluster_grid_max_y = ((cy + 1) * CLUSTER_SIZE).min(flow_field.height);
    
    // Convert to world coordinates
    let world_min = flow_field.grid_to_world(cluster_grid_min_x, cluster_grid_min_y);
    let world_max = flow_field.grid_to_world(cluster_grid_max_x, cluster_grid_max_y);
    
    // Rectangle containment check (4 comparisons - very fast)
    pos.x >= world_min.x && pos.x <= world_max.x &&
    pos.y >= world_min.y && pos.y <= world_max.y
}

/// Check if a world position is inside a specific region (O(1) rectangle check).
/// Returns false if cluster/region doesn't exist or point is outside region bounds.
/// 
/// # Arguments
/// * `pos` - World position to check
/// * `cluster_id` - Cluster containing the region
/// * `region_id` - Region to check against
/// * `graph` - Hierarchical graph
pub(crate) fn point_in_region(
    pos: FixedVec2,
    cluster_id: ClusterId,
    region_id: RegionId,
    graph: &HierarchicalGraph,
) -> bool {
    let (cx, cy) = cluster_id.as_tuple();
    
    // Get cluster (validates cluster exists)
    let cluster = match graph.get_cluster(cx, cy) {
        Some(c) => c,
        None => return false,
    };
    
    // Get region (validates region exists in this cluster)
    let region = match cluster.regions.get(region_id.0 as usize) {
        Some(Some(r)) => r,
        _ => return false, // Region doesn't exist or out of bounds
    };
    
    // Rectangle containment check (4 comparisons - very fast)
    region.bounds.contains(pos)
}

/// Check if a tile is walkable after applying obstacle dilation
/// Returns false if the tile OR any neighbor within dilation_radius is an obstacle
fn is_tile_walkable_dilated(
    x: usize,
    y: usize,
    flow_field: &FlowField,
    dilation_radius: usize,
) -> bool {
    // Check the tile itself
    let idx = flow_field.get_index(x, y);
    if idx >= flow_field.cost_field.len() || flow_field.cost_field[idx] == 255 {
        return false; // Tile is obstacle
    }
    
    // Check neighbors within dilation radius
    let radius = dilation_radius as isize;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx == 0 && dy == 0 {
                continue; // Already checked center
            }
            
            let nx = (x as isize + dx) as usize;
            let ny = (y as isize + dy) as usize;
            
            // Check bounds
            if nx >= flow_field.width || ny >= flow_field.height {
                continue;
            }
            
            let neighbor_idx = flow_field.get_index(nx, ny);
            if neighbor_idx < flow_field.cost_field.len() && flow_field.cost_field[neighbor_idx] == 255 {
                // Neighbor is obstacle - mark this tile as non-walkable
                return false;
            }
        }
    }
    
    true // Tile and all neighbors are walkable
}

/// Find all horizontal strips of walkable tiles in the cluster (NO DILATION - legacy)
/// Kept for reference/debugging, but not currently used in production.
#[allow(dead_code)]
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
    // DEPRECATED: This function is kept for compatibility but should not be used in hot paths
    // Use get_region_id_fast() with the cluster's region_lookup_grid instead
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

/// PERF: Fast O(1) region lookup using world coordinates directly (no grid conversion)
/// Returns None if point is outside cluster or in unwalkable area
#[inline]
pub(crate) fn get_region_id_by_world_pos(
    cluster: &super::cluster::Cluster,
    world_pos: FixedVec2,
) -> Option<RegionId> {
    // Quantize world position to match HashMap keys (0.5 world unit resolution)
    let x_quantized = (world_pos.x.to_num::<f32>() * 2.0) as i32;
    let y_quantized = (world_pos.y.to_num::<f32>() * 2.0) as i32;
    
    cluster.region_world_lookup.get(&(x_quantized, y_quantized)).copied()
}

/// PERF: Fast O(1) island lookup using world coordinates directly (no searching!)
/// Returns None if point is outside cluster or in unwalkable area
/// This eliminates the O(N) linear search through regions to find nearest island
#[inline]
pub(crate) fn get_island_id_by_world_pos(
    cluster: &super::cluster::Cluster,
    world_pos: FixedVec2,
) -> Option<super::types::IslandId> {
    // Quantize world position to match HashMap keys (0.5 world unit resolution)
    let x_quantized = (world_pos.x.to_num::<f32>() * 2.0) as i32;
    let y_quantized = (world_pos.y.to_num::<f32>() * 2.0) as i32;
    
    cluster.island_world_lookup.get(&(x_quantized, y_quantized)).copied()
}

/// Build the region lookup grid for a cluster after regions have been created
/// This enables O(1) region lookups instead of O(N) linear search
pub(crate) fn build_region_lookup_grid(
    cluster: &mut super::cluster::Cluster,
    cluster_id: (usize, usize),
    flow_field: &FlowField,
) {
    let (cx, cy) = cluster_id;
    let start_x = cx * CLUSTER_SIZE;
    let start_y = cy * CLUSTER_SIZE;
    
    // Iterate through every grid cell in the cluster
    for local_y in 0..CLUSTER_SIZE {
        for local_x in 0..CLUSTER_SIZE {
            let world_x = start_x + local_x;
            let world_y = start_y + local_y;
            
            // Skip out of bounds
            if world_x >= flow_field.width || world_y >= flow_field.height {
                cluster.region_lookup_grid[local_y][local_x] = None;
                continue;
            }
            
            // Convert grid to world position
            let world_pos = flow_field.grid_to_world(world_x, world_y);
            
            // Find which region contains this position (using slow method during setup)
            let region_id = get_region_id(&cluster.regions, cluster.region_count, world_pos);
            
            // Store in lookup grid
            cluster.region_lookup_grid[local_y][local_x] = region_id.map(|r| r.0);
            
            // ALSO store in world-coordinate HashMaps (quantized to 0.5 world units)
            // This allows O(1) lookup without world_to_grid conversion in hot path
            if let Some(reg_id) = region_id {
                let x_quantized = (world_pos.x.to_num::<f32>() * 2.0) as i32;
                let y_quantized = (world_pos.y.to_num::<f32>() * 2.0) as i32;
                cluster.region_world_lookup.insert((x_quantized, y_quantized), reg_id);
                
                // ALSO populate island lookup for O(1) island queries (no searching!)
                if let Some(region) = &cluster.regions[reg_id.0 as usize] {
                    cluster.island_world_lookup.insert((x_quantized, y_quantized), region.island);
                }
            }
        }
    }
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

/// Check if a region is convex and well-formed
/// For our maximal rectangles algorithm, regions should always be axis-aligned rectangles (convex)
/// Mark as dangerous if: non-rectangular, very thin (aspect ratio > 10), or very small
fn is_convex_region(vertices: &SmallVec<[FixedVec2; 8]>, bounds: &Rect) -> bool {
    // Must have exactly 4 vertices for rectangles
    if vertices.len() != 4 {
        return false;
    }
    
    // Check aspect ratio - very thin regions might be artifacts
    let width = bounds.width();
    let height = bounds.height();
    
    // Avoid divide by zero - if either dimension is zero, region is degenerate
    let min_dimension = width.min(height);
    if min_dimension <= FixedNum::ZERO {
        return false; // Degenerate region
    }
    
    let aspect_ratio = width.max(height) / min_dimension;
    
    if aspect_ratio > FixedNum::from_num(10.0) {
        // Very thin strip - likely edge artifact
        return false;
    }
    
    // Check minimum size - tiny regions are problematic
    let area = width * height;
    if area < FixedNum::from_num(1.0) {
        return false;
    }
    
    // Axis-aligned rectangles are always convex
    true
}
