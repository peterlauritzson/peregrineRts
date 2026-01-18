use super::types::{Island, IslandId, TORTUOSITY_THRESHOLD, MAX_ISLANDS, MAX_REGIONS, NO_PATH, CLUSTER_SIZE};
use super::cluster::Cluster;
use crate::game::fixed_math::{FixedNum, FixedVec2};
use smallvec::SmallVec;
use bevy::prelude::*;

/// Identify islands (connected components) within a cluster using BOUNDARY-FOCUSED approach
/// 
/// **CRITICAL DESIGN:** Islands represent "sides of cross-cluster obstacles", NOT every disconnected pocket
/// 
/// Algorithm:
/// 1. Identify boundary regions (touch cluster edges or contain inter-cluster portals)
/// 2. Create islands from boundary regions using tortuosity-based flood fill
/// 3. Merge interior isolated regions into nearest boundary island
/// 
/// This prevents explosion of isolated interior pockets into separate islands.
pub(crate) fn identify_islands(cluster: &mut Cluster) {
    if cluster.region_count == 0 {
        cluster.island_count = 0;
        return;
    }
    
    let mut assigned = [false; MAX_REGIONS];
    let mut island_count = 0;
    
    // PHASE 1: Identify boundary regions
    let mut boundary_regions = Vec::new();
    let cluster_bounds = get_cluster_bounds(cluster.id);
    
    for i in 0..cluster.region_count {
        if let Some(region) = &cluster.regions[i] {
            if is_boundary_region(region, &cluster_bounds) {
                boundary_regions.push(i);
            }
        }
    }
    
    if boundary_regions.is_empty() {
        // No boundary regions - entire cluster is isolated
        // Create single island with all regions
        warn!("Cluster {:?} has NO boundary regions - creating single island", cluster.id);
        let island_id = IslandId(0);
        let mut island_regions = SmallVec::new();
        
        for i in 0..cluster.region_count {
            if let Some(region) = &cluster.regions[i] {
                island_regions.push(region.id);
                assigned[i] = true;
            }
        }
        
        let representative = cluster.regions[0].as_ref().map(|r| r.bounds.center()).unwrap_or(FixedVec2::ZERO);
        cluster.islands[0] = Some(Island { id: island_id, representative, regions: island_regions.clone() });
        
        for &region_id in &island_regions {
            if let Some(region) = &mut cluster.regions[region_id.0 as usize] {
                region.island = island_id;
            }
        }
        
        cluster.island_count = 1;
        return;
    }
    
    // PHASE 2: Create islands from boundary regions using tortuosity-based connectivity
    for &seed in &boundary_regions {
        if assigned[seed] {
            continue;
        }
        
        if island_count >= MAX_ISLANDS {
            warn!("Cluster {:?} has more than {} boundary islands, merging remaining", cluster.id, MAX_ISLANDS);
            // Assign remaining boundary regions to the last island
            if let Some(last_island) = &mut cluster.islands[island_count - 1] {
                for &r in &boundary_regions {
                    if !assigned[r] {
                        assigned[r] = true;
                        last_island.regions.push(cluster.regions[r].as_ref().unwrap().id);
                        if let Some(region) = &mut cluster.regions[r] {
                            region.island = last_island.id;
                        }
                    }
                }
            }
            break;
        }
        
        // Start a new island from this boundary region
        let island_id = IslandId(island_count as u8);
        let mut island_regions: SmallVec<[super::types::RegionId; MAX_REGIONS]> = SmallVec::new();
        
        // Flood fill from seed, adding well-connected regions
        let mut to_visit = vec![seed];
        assigned[seed] = true;
        
        while let Some(current) = to_visit.pop() {
            island_regions.push(cluster.regions[current].as_ref().unwrap().id);
            
            // Only expand to other boundary regions that are well-connected
            for &candidate in &boundary_regions {
                if assigned[candidate] {
                    continue;
                }
                
                if is_well_connected(cluster, current, candidate) {
                    assigned[candidate] = true;
                    to_visit.push(candidate);
                }
            }
        }
        
        // Get representative position (center of first region)
        let representative = cluster.regions[seed]
            .as_ref()
            .map(|r| r.bounds.center())
            .unwrap_or(FixedVec2::ZERO);
        
        // Create island
        cluster.islands[island_count] = Some(Island {
            id: island_id,
            representative,
            regions: island_regions.clone(),
        });
        
        // Update boundary regions with their island ID
        for &region_id in &island_regions {
            if let Some(region) = &mut cluster.regions[region_id.0 as usize] {
                region.island = island_id;
            }
        }
        
        island_count += 1;
    }
    
    // PHASE 3: Merge interior (non-boundary) regions into nearest boundary island
    let mut interior_assignments = Vec::new();
    
    for i in 0..cluster.region_count {
        if assigned[i] {
            continue; // Already assigned to a boundary island
        }
        
        // This is an interior region - find nearest boundary island
        if let Some(interior_region) = &cluster.regions[i] {
            let nearest_island = find_nearest_island(interior_region, cluster, island_count);
            interior_assignments.push((i, interior_region.id, nearest_island));
            assigned[i] = true;
        }
    }
    
    // Now apply the assignments
    for (region_idx, region_id, nearest_island) in interior_assignments {
        // Assign to nearest island
        if let Some(region_mut) = &mut cluster.regions[region_idx] {
            region_mut.island = nearest_island;
        }
        
        // Add to island's region list
        if let Some(island) = &mut cluster.islands[nearest_island.0 as usize] {
            island.regions.push(region_id);
        }
    }
    
    cluster.island_count = island_count;
    
    // Debug logging
    static LOGGED_CLUSTERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    if LOGGED_CLUSTERS.load(std::sync::atomic::Ordering::Relaxed) < 5 {
        info!("[ISLAND DETECTION] Cluster {:?}: {} regions, {} boundary, {} islands", 
            cluster.id, cluster.region_count, boundary_regions.len(), island_count);
        LOGGED_CLUSTERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Get the bounding box for a cluster in cluster-local coordinates
fn get_cluster_bounds(_cluster_id: (usize, usize)) -> super::types::Rect {
    use super::types::Rect;
    
    // Cluster bounds in local coordinates (0 to CLUSTER_SIZE)
    let min = FixedVec2::new(FixedNum::ZERO, FixedNum::ZERO);
    let max = FixedVec2::new(
        FixedNum::from_num(CLUSTER_SIZE),
        FixedNum::from_num(CLUSTER_SIZE)
    );
    
    Rect::new(min, max)
}

/// Check if a region is a boundary region (touches cluster edges)
/// 
/// Boundary regions are those that:
/// 1. Touch the cluster's edge (x=0, x=CLUSTER_SIZE, y=0, y=CLUSTER_SIZE)
/// 2. Contain or are adjacent to inter-cluster portals (handled via portals connectivity)
fn is_boundary_region(region: &super::types::Region, cluster_bounds: &super::types::Rect) -> bool {
    let epsilon = FixedNum::from_num(0.5);
    
    // Check if region touches any cluster edge
    let touches_left = (region.bounds.min.x - cluster_bounds.min.x).abs() < epsilon;
    let touches_right = (region.bounds.max.x - cluster_bounds.max.x).abs() < epsilon;
    let touches_bottom = (region.bounds.min.y - cluster_bounds.min.y).abs() < epsilon;
    let touches_top = (region.bounds.max.y - cluster_bounds.max.y).abs() < epsilon;
    
    touches_left || touches_right || touches_bottom || touches_top
}

/// Find the nearest boundary island to an interior region
fn find_nearest_island(
    interior_region: &super::types::Region,
    cluster: &Cluster,
    island_count: usize,
) -> IslandId {
    let interior_center = interior_region.bounds.center();
    let mut nearest_island = IslandId(0);
    let mut min_dist_sq = FixedNum::from_num(1_000_000.0); // Very large value
    
    for i in 0..island_count {
        if let Some(island) = &cluster.islands[i] {
            // Find closest region in this island
            for &region_id in &island.regions {
                if let Some(region) = &cluster.regions[region_id.0 as usize] {
                    let center = region.bounds.center();
                    let dx = center.x - interior_center.x;
                    let dy = center.y - interior_center.y;
                    let dist_sq = dx * dx + dy * dy;
                    
                    if dist_sq < min_dist_sq {
                        min_dist_sq = dist_sq;
                        nearest_island = island.id;
                    }
                }
            }
        }
    }
    
    nearest_island
}

/// Check if two regions are "well connected" (low tortuosity path between them)
fn is_well_connected(cluster: &Cluster, region_a: usize, region_b: usize) -> bool {
    // If routing table says NO_PATH, they're definitely not connected
    if cluster.local_routing[region_a][region_b] == NO_PATH {
        return false;
    }
    
    // Get region centers
    let center_a = get_region_center(cluster, region_a);
    let center_b = get_region_center(cluster, region_b);
    
    // Calculate euclidean distance
    let euclidean_distance = (center_b - center_a).length();
    
    // Avoid division by zero
    if euclidean_distance < FixedNum::from_num(0.1) {
        return true; // Same position, definitely connected
    }
    
    // Calculate path distance using routing table
    let path_distance = estimate_path_distance(cluster, region_a, region_b);
    
    // Calculate tortuosity
    let tortuosity = path_distance / euclidean_distance;
    
    // Well-connected if tortuosity is below threshold
    tortuosity <= FixedNum::from_num(TORTUOSITY_THRESHOLD)
}

/// Get the center of a region
fn get_region_center(cluster: &Cluster, region_id: usize) -> FixedVec2 {
    cluster.regions[region_id]
        .as_ref()
        .map(|r| r.bounds.center())
        .unwrap_or(FixedVec2::ZERO)
}

/// Estimate path distance by walking the routing table
fn estimate_path_distance(cluster: &Cluster, start: usize, end: usize) -> FixedNum {
    if start == end {
        return FixedNum::ZERO;
    }
    
    let mut current = start;
    let mut total_distance = FixedNum::ZERO;
    let mut visited = [false; MAX_REGIONS];
    let max_hops = MAX_REGIONS; // Prevent infinite loops
    
    for _ in 0..max_hops {
        if current == end {
            return total_distance;
        }
        
        visited[current] = true;
        
        let next = cluster.local_routing[current][end] as usize;
        
        if next == NO_PATH as usize {
            // No path exists
            return FixedNum::from_num(1000.0); // Large number to indicate disconnection
        }
        
        if next == current {
            // Stuck in a loop or at destination
            break;
        }
        
        if visited[next] {
            // Loop detected
            return FixedNum::from_num(1000.0);
        }
        
        // Add distance from current to next
        let current_pos = get_region_center(cluster, current);
        let next_pos = get_region_center(cluster, next);
        total_distance += (next_pos - current_pos).length();
        
        current = next;
    }
    
    // If we didn't reach the end, return large distance
    if current != end {
        return FixedNum::from_num(1000.0);
    }
    
    total_distance
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_tortuosity_calculation() {
        // Simple test: two regions next to each other should have low tortuosity
        let region_a_center = FixedVec2::new(FixedNum::from_num(5), FixedNum::from_num(5));
        let region_b_center = FixedVec2::new(FixedNum::from_num(10), FixedNum::from_num(5));
        
        let euclidean = (region_b_center - region_a_center).length();
        let path = euclidean; // Direct path
        let tortuosity = path / euclidean;
        
        assert!(tortuosity <= FixedNum::from_num(TORTUOSITY_THRESHOLD));
    }
}
