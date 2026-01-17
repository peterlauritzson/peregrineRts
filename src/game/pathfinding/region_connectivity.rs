use super::types::{Region, RegionPortal, RegionId, LineSegment, NO_PATH, MAX_REGIONS};
use super::cluster::Cluster;
use crate::game::fixed_math::{FixedVec2, FixedNum};
use std::collections::{BinaryHeap, HashMap};
use std::cmp::Reverse;

/// Build connectivity graph between regions and compute local routing table
pub(crate) fn build_region_connectivity(cluster: &mut Cluster) {
    // Step 1: Find shared edges (portals) between regions
    find_region_portals(cluster);
    
    // Step 2: Build routing table using BFS/Dijkstra
    build_local_routing_table(cluster);
}

/// Find portals (shared edges) between adjacent regions
fn find_region_portals(cluster: &mut Cluster) {
    // Collect portal information first, then add to regions
    let mut portals_to_add: Vec<(usize, RegionPortal)> = Vec::new();
    
    // For each pair of regions, check if they share an edge
    for i in 0..cluster.region_count {
        for j in (i + 1)..cluster.region_count {
            if let (Some(region_a), Some(region_b)) = (&cluster.regions[i], &cluster.regions[j]) {
                if let Some(shared_edge) = find_shared_edge(region_a, region_b) {
                    // Portal from A to B
                    portals_to_add.push((i, RegionPortal {
                        edge: shared_edge,
                        center: shared_edge.center(),
                        next_region: region_b.id,
                    }));
                    
                    // Portal from B to A (bidirectional)
                    portals_to_add.push((j, RegionPortal {
                        edge: shared_edge,
                        center: shared_edge.center(),
                        next_region: region_a.id,
                    }));
                }
            }
        }
    }
    
    // Now add all the portals
    for (region_idx, portal) in portals_to_add {
        if let Some(region) = &mut cluster.regions[region_idx] {
            region.portals.push(portal);
        }
    }
}

/// Find the shared edge between two regions (if any)
fn find_shared_edge(a: &Region, b: &Region) -> Option<LineSegment> {
    // For rectangles (4 vertices), check each edge of A against each edge of B
    for i in 0..a.vertices.len() {
        let a1 = a.vertices[i];
        let a2 = a.vertices[(i + 1) % a.vertices.len()];
        
        for j in 0..b.vertices.len() {
            let b1 = b.vertices[j];
            let b2 = b.vertices[(j + 1) % b.vertices.len()];
            
            if let Some(overlap) = compute_segment_overlap(a1, a2, b1, b2) {
                return Some(overlap);
            }
        }
    }
    
    None
}

/// Compute the overlap between two line segments (if they are collinear and overlapping)
fn compute_segment_overlap(
    a1: FixedVec2,
    a2: FixedVec2,
    b1: FixedVec2,
    b2: FixedVec2,
) -> Option<LineSegment> {
    // Check if segments are axis-aligned (horizontal or vertical)
    let a_is_horizontal = (a1.y - a2.y).abs() < FixedNum::from_num(0.1);
    let a_is_vertical = (a1.x - a2.x).abs() < FixedNum::from_num(0.1);
    let b_is_horizontal = (b1.y - b2.y).abs() < FixedNum::from_num(0.1);
    let b_is_vertical = (b1.x - b2.x).abs() < FixedNum::from_num(0.1);
    
    // Both must be horizontal or both must be vertical
    if a_is_horizontal && b_is_horizontal {
        // Check if they're on the same y-coordinate
        if (a1.y - b1.y).abs() < FixedNum::from_num(0.1) {
            // Find overlap in x-range
            let a_min_x = a1.x.min(a2.x);
            let a_max_x = a1.x.max(a2.x);
            let b_min_x = b1.x.min(b2.x);
            let b_max_x = b1.x.max(b2.x);
            
            let overlap_min = a_min_x.max(b_min_x);
            let overlap_max = a_max_x.min(b_max_x);
            
            if overlap_max > overlap_min {
                return Some(LineSegment {
                    start: FixedVec2::new(overlap_min, a1.y),
                    end: FixedVec2::new(overlap_max, a1.y),
                });
            }
        }
    } else if a_is_vertical && b_is_vertical {
        // Check if they're on the same x-coordinate
        if (a1.x - b1.x).abs() < FixedNum::from_num(0.1) {
            // Find overlap in y-range
            let a_min_y = a1.y.min(a2.y);
            let a_max_y = a1.y.max(a2.y);
            let b_min_y = b1.y.min(b2.y);
            let b_max_y = b1.y.max(b2.y);
            
            let overlap_min = a_min_y.max(b_min_y);
            let overlap_max = a_max_y.min(b_max_y);
            
            if overlap_max > overlap_min {
                return Some(LineSegment {
                    start: FixedVec2::new(a1.x, overlap_min),
                    end: FixedVec2::new(a1.x, overlap_max),
                });
            }
        }
    }
    
    None
}

/// Build local routing table using Dijkstra from each region
fn build_local_routing_table(cluster: &mut Cluster) {
    // Initialize all entries to NO_PATH
    cluster.local_routing = [[NO_PATH; MAX_REGIONS]; MAX_REGIONS];
    
    // Set diagonal (region to itself) to itself
    for i in 0..cluster.region_count {
        cluster.local_routing[i][i] = i as u8;
    }
    
    // For each source region, run Dijkstra to find shortest paths to all others
    for start in 0..cluster.region_count {
        let Some(start_region) = &cluster.regions[start] else { continue; };
        
        // Dijkstra's algorithm
        let mut distances: HashMap<RegionId, FixedNum> = HashMap::new();
        let mut next_hop: HashMap<RegionId, RegionId> = HashMap::new();
        let mut heap = BinaryHeap::new();
        
        distances.insert(start_region.id, FixedNum::ZERO);
        heap.push(Reverse((FixedNum::ZERO, start_region.id)));
        
        while let Some(Reverse((cost, current_id))) = heap.pop() {
            // If we've already found a better path, skip
            if let Some(&best_cost) = distances.get(&current_id) {
                if cost > best_cost {
                    continue;
                }
            }
            
            // Get current region
            let Some(current_region) = &cluster.regions[current_id.0 as usize] else { continue; };
            
            // Explore neighbors
            for portal in &current_region.portals {
                let neighbor_id = portal.next_region;
                let edge_cost = portal.edge.length();
                let new_cost = cost + edge_cost;
                
                let should_update = distances.get(&neighbor_id)
                    .map_or(true, |&old_cost| new_cost < old_cost);
                
                if should_update {
                    distances.insert(neighbor_id, new_cost);
                    heap.push(Reverse((new_cost, neighbor_id)));
                    
                    // Track the next hop from start
                    if current_id == start_region.id {
                        // Direct neighbor of start
                        next_hop.insert(neighbor_id, neighbor_id);
                    } else {
                        // Inherit the next hop from current
                        if let Some(&hop) = next_hop.get(&current_id) {
                            next_hop.insert(neighbor_id, hop);
                        }
                    }
                }
            }
        }
        
        // Populate routing table for this source
        for (dest_id, first_hop) in next_hop {
            cluster.local_routing[start][dest_id.0 as usize] = first_hop.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_horizontal_segment_overlap() {
        let a1 = FixedVec2::new(FixedNum::from_num(0), FixedNum::from_num(5));
        let a2 = FixedVec2::new(FixedNum::from_num(10), FixedNum::from_num(5));
        let b1 = FixedVec2::new(FixedNum::from_num(5), FixedNum::from_num(5));
        let b2 = FixedVec2::new(FixedNum::from_num(15), FixedNum::from_num(5));
        
        let overlap = compute_segment_overlap(a1, a2, b1, b2);
        assert!(overlap.is_some());
        
        let segment = overlap.unwrap();
        assert_eq!(segment.start.x, FixedNum::from_num(5));
        assert_eq!(segment.end.x, FixedNum::from_num(10));
    }
    
    #[test]
    fn test_no_overlap() {
        let a1 = FixedVec2::new(FixedNum::from_num(0), FixedNum::from_num(5));
        let a2 = FixedVec2::new(FixedNum::from_num(10), FixedNum::from_num(5));
        let b1 = FixedVec2::new(FixedNum::from_num(15), FixedNum::from_num(5));
        let b2 = FixedVec2::new(FixedNum::from_num(25), FixedNum::from_num(5));
        
        let overlap = compute_segment_overlap(a1, a2, b1, b2);
        assert!(overlap.is_none());
    }
}
