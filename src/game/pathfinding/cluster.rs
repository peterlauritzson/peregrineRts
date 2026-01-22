use serde::{Serialize, Deserialize};
use crate::game::fixed_math::FixedNum;
use super::types::{Portal, Node, Region, Island, RegionId, MAX_REGIONS, MAX_ISLANDS, NO_PATH};

/// Represents a spatial cluster in the hierarchical pathfinding graph.
///
/// # NEW: Region-Based Navigation (Convex Decomposition)
///
/// Each cluster is decomposed into convex regions for proper intra-cluster pathfinding.
/// This replaces the old flow field approach with a more memory-efficient and robust system.
///
/// ## Memory Budget
///
/// - **Regions:** ~32 max × 64 bytes = ~2 KB
/// - **Local routing table:** 32×32 × 1 byte = 1 KB  
/// - **Islands:** ~4 × 32 bytes = 128 bytes
/// - **Total per cluster:** ~3 KB (down from ~75 KB with flow fields)
///
/// For a 2048×2048 map:
/// - Clusters: 82 × 82 = 6,724
/// - **Total: 6,724 × 3 KB ≈ 20 MB** (vs. previous ~504 MB)
///
/// ## Benefits
///
/// - **Memory:** 96% reduction (20MB vs 504MB)
/// - **Last Mile:** Direct movement in same region (convexity guarantee)
/// - **Island Awareness:** Routes to correct side of obstacles automatically
/// - **Dynamic Updates:** Faster cluster re-baking (region decomposition vs flow field generation)
///
/// See: [PATHFINDING.md](documents/Design%20docs/PATHFINDING.md) - Complete design documentation
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Cluster {
    pub id: (usize, usize),
    
    // Region-based navigation
    /// Convex regions within this cluster (5-30 typical, 32 max)
    pub regions: [Option<Region>; MAX_REGIONS],
    /// Number of valid regions (0..regions.len())
    pub region_count: usize,
    
    /// Islands (connected components) of regions (1-4 typical)
    pub islands: [Option<Island>; MAX_ISLANDS],
    /// Number of valid islands
    pub island_count: usize,
    
    /// Local routing table: [from_region][to_region] = next_region_id
    /// NO_PATH (255) indicates regions are on different islands
    pub local_routing: [[u8; MAX_REGIONS]; MAX_REGIONS],
    
    /// Neighbor connectivity: [island_id][direction] -> Option<portal_id>
    /// Direction: 0=North, 1=South, 2=East, 3=West, 4=NE, 5=NW, 6=SE, 7=SW (use Direction enum for type safety)
    pub neighbor_connectivity: [[Option<usize>; 8]; MAX_ISLANDS],
    
    /// PERF: Region lookup grid for O(1) point-in-region queries
    /// Maps cluster-local grid positions to region IDs (None if unwalkable)
    /// Size: CLUSTER_SIZE × CLUSTER_SIZE (25×25 = 625 bytes per cluster)
    /// This replaces O(N) linear search through regions with O(1) array access
    pub region_lookup_grid: Box<[[Option<u8>; super::types::CLUSTER_SIZE]; super::types::CLUSTER_SIZE]>,
    
    /// PERF: Fast region lookup by world coordinates (HashMap fallback)
    /// Maps quantized world coordinates to region IDs
    /// Used when entity is in cluster but grid lookup would require expensive world_to_grid conversion
    /// Key: (x_quantized, y_quantized) in world space, quantized to 0.5 world units
    pub region_world_lookup: std::collections::HashMap<(i32, i32), RegionId>,
    
    /// PERF: Fast island lookup by world coordinates (O(1) no searching!)
    /// Maps quantized world coordinates to island IDs
    /// Key: (x_quantized, y_quantized) in world space, quantized to 0.5 world units
    /// This eliminates O(N) search through regions to find nearest island
    pub island_world_lookup: std::collections::HashMap<(i32, i32), super::types::IslandId>,
}

impl Cluster {
    pub fn new(id: (usize, usize)) -> Self {
        Self {
            id,
            regions: [const { None }; MAX_REGIONS],
            region_count: 0,
            islands: [const { None }; MAX_ISLANDS],
            island_count: 0,
            local_routing: [[NO_PATH; MAX_REGIONS]; MAX_REGIONS],
            neighbor_connectivity: [[None; 8]; MAX_ISLANDS],
            region_lookup_grid: Box::new([[None; super::types::CLUSTER_SIZE]; super::types::CLUSTER_SIZE]),
            region_world_lookup: std::collections::HashMap::new(),
            island_world_lookup: std::collections::HashMap::new(),
        }
    }
    
}

pub(super) fn create_portal_vertical(
    graph: &mut super::graph::HierarchicalGraph,
    x1: usize, x2: usize,
    y_start: usize, y_end: usize,
    c1x: usize, c1y: usize,
    c2x: usize, c2y: usize,
    flow_field: &crate::game::structures::FlowField,
) {
    let mid_y = (y_start + y_end) / 2;
    
    let id1 = graph.next_portal_id;
    graph.next_portal_id += 1;
    graph.portals.push(Portal { 
        id: id1, 
        node: Node { x: x1, y: mid_y }, 
        range_min: Node { x: x1, y: y_start },
        range_max: Node { x: x1, y: y_end },
        cluster: (c1x, c1y),
        world_pos: flow_field.grid_to_world(x1, mid_y),
    });

    let id2 = graph.next_portal_id;
    graph.next_portal_id += 1;
    graph.portals.push(Portal { 
        id: id2, 
        node: Node { x: x2, y: mid_y }, 
        range_min: Node { x: x2, y: y_start },
        range_max: Node { x: x2, y: y_end },
        cluster: (c2x, c2y),
        world_pos: flow_field.grid_to_world(x2, mid_y),
    });
    
    let cost = FixedNum::from_num(1.0);
    // Ensure capacity for portal connections
    while graph.portal_connections.len() <= id2.max(id1) {
        graph.portal_connections.push(Vec::new());
    }
    graph.portal_connections[id1].push((id2, cost));
    graph.portal_connections[id2].push((id1, cost));
}

pub(super) fn create_portal_horizontal(
    graph: &mut super::graph::HierarchicalGraph,
    x_start: usize, x_end: usize,
    y1: usize, y2: usize,
    c1x: usize, c1y: usize,
    c2x: usize, c2y: usize,
    flow_field: &crate::game::structures::FlowField,
) {
    let mid_x = (x_start + x_end) / 2;
    
    let id1 = graph.next_portal_id;
    graph.next_portal_id += 1;
    graph.portals.push(Portal { 
        id: id1, 
        node: Node { x: mid_x, y: y1 }, 
        range_min: Node { x: x_start, y: y1 },
        range_max: Node { x: x_end, y: y1 },
        cluster: (c1x, c1y),
        world_pos: flow_field.grid_to_world(mid_x, y1),
    });

    let id2 = graph.next_portal_id;
    graph.next_portal_id += 1;
    graph.portals.push(Portal { 
        id: id2, 
        node: Node { x: mid_x, y: y2 }, 
        range_min: Node { x: x_start, y: y2 },
        range_max: Node { x: x_end, y: y2 },
        cluster: (c2x, c2y),
        world_pos: flow_field.grid_to_world(mid_x, y2),
    });
    
    let cost = FixedNum::from_num(1.0);
    // Ensure capacity for portal connections
    while graph.portal_connections.len() <= id2.max(id1) {
        graph.portal_connections.push(Vec::new());
    }
    graph.portal_connections[id1].push((id2, cost));
    graph.portal_connections[id2].push((id1, cost));
}

/// Create a diagonal portal connecting two clusters at different positions
pub(super) fn create_portal_diagonal(
    graph: &mut super::graph::HierarchicalGraph,
    x1: usize, y1: usize,
    c1x: usize, c1y: usize,
    x2: usize, y2: usize,
    c2x: usize, c2y: usize,
    flow_field: &crate::game::structures::FlowField,
) {
    let id1 = graph.next_portal_id;
    graph.next_portal_id += 1;
    graph.portals.insert(id1, Portal { 
        id: id1, 
        node: Node { x: x1, y: y1 }, 
        range_min: Node { x: x1, y: y1 },
        range_max: Node { x: x1, y: y1 },
        cluster: (c1x, c1y),
        world_pos: flow_field.grid_to_world(x1, y1),
    });

    let id2 = graph.next_portal_id;
    graph.next_portal_id += 1;
    graph.portals.push(Portal { 
        id: id2, 
        node: Node { x: x2, y: y2 }, 
        range_min: Node { x: x2, y: y2 },
        range_max: Node { x: x2, y: y2 },
        cluster: (c2x, c2y),
        world_pos: flow_field.grid_to_world(x2, y2),
    });
    
    // Diagonal distance: sqrt(2) ≈ 1.414
    let cost = FixedNum::from_num(1.414);
    // Ensure capacity for portal connections
    while graph.portal_connections.len() <= id2.max(id1) {
        graph.portal_connections.push(Vec::new());
    }
    graph.portal_connections[id1].push((id2, cost));
    graph.portal_connections[id2].push((id1, cost));
}
