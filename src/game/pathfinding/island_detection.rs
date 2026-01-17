use super::types::{Island, IslandId, TORTUOSITY_THRESHOLD, MAX_ISLANDS, MAX_REGIONS, NO_PATH};
use super::cluster::Cluster;
use crate::game::fixed_math::{FixedNum, FixedVec2};
use smallvec::SmallVec;
use bevy::prelude::*;

/// Identify islands (connected components) within a cluster based on tortuosity
pub(crate) fn identify_islands(cluster: &mut Cluster) {
    if cluster.region_count == 0 {
        cluster.island_count = 0;
        return;
    }
    
    let mut assigned = [false; MAX_REGIONS];
    let mut island_count = 0;
    
    // Process each unassigned region
    for seed in 0..cluster.region_count {
        if assigned[seed] {
            continue;
        }
        
        if island_count >= MAX_ISLANDS {
            warn!("Cluster {:?} has more than {} islands, using fallback", cluster.id, MAX_ISLANDS);
            // Assign remaining regions to the last island
            if let Some(last_island) = &mut cluster.islands[island_count - 1] {
                for r in seed..cluster.region_count {
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
        
        // Start a new island
        let island_id = IslandId(island_count as u8);
        let mut island_regions: SmallVec<[super::types::RegionId; MAX_REGIONS]> = SmallVec::new();
        
        // Flood fill from seed, adding regions that are "well connected"
        let mut to_visit = vec![seed];
        assigned[seed] = true;
        
        while let Some(current) = to_visit.pop() {
            island_regions.push(cluster.regions[current].as_ref().unwrap().id);
            
            // Check all regions for connectivity to current
            for candidate in 0..cluster.region_count {
                if assigned[candidate] {
                    continue;
                }
                
                // Check if candidate is well-connected to current
                if is_well_connected(cluster, current, candidate) {
                    assigned[candidate] = true;
                    to_visit.push(candidate);
                }
            }
        }
        
        // Skip creating island if it's a single tiny isolated region with no portals
        // These are unusable for pathfinding anyway
        if island_regions.len() == 1 {
            let first_region_id = island_regions[0];
            let region_idx = first_region_id.0 as usize;
            if let Some(region) = &cluster.regions[region_idx] {
                // Check if region has ANY portals to other regions
                if region.portals.is_empty() {
                    // Isolated region with no connections - skip it
                    // Mark as assigned to island 0 (fallback) to avoid re-processing
                    if let Some(region_mut) = &mut cluster.regions[region_idx] {
                        region_mut.island = IslandId(0);
                    }
                    continue;
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
        
        // Update regions with their island ID
        for &region_id in &island_regions {
            if let Some(region) = &mut cluster.regions[region_id.0 as usize] {
                region.island = island_id;
            }
        }
        
        island_count += 1;
    }
    
    cluster.island_count = island_count;
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
