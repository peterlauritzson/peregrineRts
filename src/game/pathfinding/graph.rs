use bevy::prelude::*;
use serde::{Serialize, Deserialize};
use std::collections::{BTreeMap, BinaryHeap};
use crate::game::fixed_math::FixedNum;
use super::types::{CLUSTER_SIZE, Portal, ClusterIslandId, IslandId};
use super::cluster::Cluster;
use std::cmp::Reverse;

/// Hierarchical pathfinding graph for large-scale RTS navigation.
///
/// NEW: Region-Based Navigation with Island Awareness
///
/// # Architecture
///
/// 1. **Clustering:** Map divided into CLUSTER_SIZE × CLUSTER_SIZE grids (spatial hash)
/// 2. **Regions:** Each cluster decomposed into convex polygons (5-30 typical)
/// 3. **Islands:** Regions grouped by connectivity (handles U-shaped obstacles)
/// 4. **Local Routing:** [region][region] → next_region per cluster
/// 5. **Global Routing:** [(cluster, island)][(cluster, island)] → next_portal
///
/// # Benefits
///
/// - **Memory:** ~20MB vs ~500MB (96% reduction from flow fields)
/// - **Last Mile:** Direct movement in same region (convexity guarantee)
/// - **Island Awareness:** Routes to correct side of obstacles
/// - **Scalability:** O(1) lookups for movement decisions
///
/// # See Also
///
/// - [PATHFINDING.md](documents/Design%20docs/PATHFINDING.md) - Complete design doc
/// - [PATHFINDING_MIGRATION.md](documents/Design%20docs/PATHFINDING_MIGRATION.md) - Migration plan
#[derive(Resource, Default, Serialize, Deserialize, Clone)]
pub struct HierarchicalGraph {
    // Core data
    pub clusters: BTreeMap<(usize, usize), Cluster>,
    pub initialized: bool,
    
    // Island-aware routing
    /// Island-to-island routing table: [source][dest] = next_portal_id
    /// Key: (cluster, island) pairs
    /// Value: Portal ID to take from source island toward dest island
    pub island_routing_table: BTreeMap<ClusterIslandId, BTreeMap<ClusterIslandId, usize>>,
    
    /// Portal registry for inter-cluster navigation
    /// Maps portal_id to Portal data (position, clusters it connects)
    pub portals: BTreeMap<usize, Portal>,
    pub next_portal_id: usize,
    
    /// Portal-to-island mapping: which island can access this portal in its cluster?
    /// Maps portal_id -> island_id in that portal's cluster
    /// This is populated during populate_island_portal_connectivity
    pub portal_island_map: BTreeMap<usize, IslandId>,
    
    /// Portal-to-portal connections for cross-cluster navigation
    /// Maps portal_id to list of (connected_portal_id, cost)
    pub portal_connections: BTreeMap<usize, Vec<(usize, FixedNum)>>,
}

impl HierarchicalGraph {
    pub fn reset(&mut self) {
        self.portals.clear();
        self.next_portal_id = 0;
        self.portal_connections.clear();
        self.clusters.clear();
        self.island_routing_table.clear();
        self.initialized = false;
    }

    
    /// Build island-aware routing table using Dijkstra from each (cluster, island) pair
    pub fn build_island_routing_table(&mut self) {
        self.island_routing_table.clear();
        
        if self.clusters.is_empty() {
            return;
        }
        
        info!("[ROUTING TABLE] Building island-aware routing table...");
        
        // Collect all (cluster, island) pairs
        let mut cluster_islands = Vec::new();
        for (&cluster_id, cluster) in &self.clusters {
            for island_idx in 0..cluster.island_count {
                let island_id = IslandId(island_idx as u8);
                cluster_islands.push(ClusterIslandId::new(cluster_id, island_id));
            }
        }
        
        info!("[ROUTING TABLE] Processing {} (cluster, island) pairs", cluster_islands.len());
        
        // For each source (cluster, island), run Dijkstra
        for &source in &cluster_islands {
            self.build_routing_for_island(source);
        }
        
        let total_entries: usize = self.island_routing_table.values().map(|m| m.len()).sum();
        info!(
            "[ROUTING TABLE] Complete: {} island pairs, ~{} KB memory",
            total_entries,
            (total_entries * std::mem::size_of::<usize>() * 3) / 1024
        );
    }
    
    /// Build routing from one (cluster, island) to all others

    fn build_routing_for_island(&mut self, source: ClusterIslandId) {
        let mut distances: BTreeMap<ClusterIslandId, FixedNum> = BTreeMap::new();
        let mut next_portal: BTreeMap<ClusterIslandId, usize> = BTreeMap::new();
        let mut heap: BinaryHeap<Reverse<(FixedNum, ClusterIslandId, Option<usize>)>> = BinaryHeap::new();
        
        distances.insert(source, FixedNum::ZERO);
        heap.push(Reverse((FixedNum::ZERO, source, None)));
        
        while let Some(Reverse((cost, current, first_portal))) = heap.pop() {
            // Skip if we've found a better path
            if let Some(&best_cost) = distances.get(&current) {
                if cost > best_cost {
                    continue;
                }
            }
            
            // Record the first portal used to reach this (cluster, island)
            if let Some(portal_id) = first_portal {
                if !next_portal.contains_key(&current) {
                    next_portal.insert(current, portal_id);
                }
            }
            
            // Explore neighbors via portals
            if let Some(cluster) = self.clusters.get(&current.cluster) {
                let mut found_any_portal = false;
                // Get portals accessible from this island
                for direction in super::types::Direction::ALL {
                    if let Some(portal_id) = cluster.neighbor_connectivity[current.island.0 as usize][direction.as_index()] {
                        found_any_portal = true;
                        // Find which (cluster, island) this portal leads to
                        if let Some(_portal) = self.portals.get(&portal_id) {
                            // Find the connected portal (cross-cluster edge)
                            if let Some(neighbors) = self.portal_connections.get(&portal_id) {
                                for &(neighbor_portal_id, edge_cost) in neighbors {
                                    if let Some(neighbor_portal) = self.portals.get(&neighbor_portal_id) {
                                        let neighbor_cluster = neighbor_portal.cluster;
                                        
                                        // Determine which island in the neighbor cluster this portal connects to
                                        // Use the portal_island_map we populated earlier
                                        if let Some(&neighbor_island) = self.portal_island_map.get(&neighbor_portal_id) {
                                            let neighbor = ClusterIslandId::new(neighbor_cluster, neighbor_island);
                                            
                                            let new_cost = cost + edge_cost;
                                            let should_update = distances.get(&neighbor)
                                                .map_or(true, |&old_cost| new_cost < old_cost);
                                            
                                            if should_update {
                                                distances.insert(neighbor, new_cost);
                                                
                                                // Determine which portal to record
                                                let portal_to_record = if current == source {
                                                    // First hop from source
                                                    portal_id
                                                } else {
                                                    // Inherit first portal from current
                                                    first_portal.unwrap_or(portal_id)
                                                };
                                                
                                                heap.push(Reverse((new_cost, neighbor, Some(portal_to_record))));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                
                // Debug: Warn if an island has no portals (isolated island)
                if !found_any_portal && current == source {
                    warn!("[ROUTING] Source island {:?} has NO accessible portals - isolated island!", current);
                }
            }
        }
        
        // Debug: Log if we found very few destinations (might indicate connectivity issues)
        let destination_count = next_portal.len();
        if destination_count < 5 && destination_count > 0 {
            warn!("[ROUTING] Source {:?} only reached {} destinations (possible connectivity issue)", 
                source, destination_count);
        }
        
        // Store routing table for this source
        self.island_routing_table.insert(source, next_portal);
    }
    
    /// Lookup next portal to take from current (cluster, island) toward goal (cluster, island)
    pub fn get_next_portal_for_island(
        &self,
        current: ClusterIslandId,
        goal: ClusterIslandId,
    ) -> Option<usize> {
        if current == goal {
            return None;
        }
        
        self.island_routing_table
            .get(&current)?
            .get(&goal)
            .copied()
    }
    
    /// Populate neighbor_connectivity: link each island to portals in each direction
    /// 
    /// For each cluster, determines which portals each island can access.
    /// Uses Direction enum for type-safe indexing (North=0, South=1, East=2, West=3)
    fn populate_island_portal_connectivity(&mut self, flow_field: &crate::game::structures::FlowField) {
        use super::region_decomposition::{get_region_id, world_to_cluster_local};
        use super::types::Direction;
        
        info!("[CONNECTIVITY] Linking islands to their boundary portals...");
        
        // For each cluster
        for (&cluster_id, cluster) in &self.clusters {
            // For each portal owned by this cluster
            for (&_portal_id, portal) in &self.portals {
                if portal.cluster != cluster_id {
                    continue; // Portal belongs to a different cluster
                }
                
                // Determine which direction this portal is in (relative to cluster)
                let cluster_x_tiles = cluster_id.0 * CLUSTER_SIZE;
                let cluster_y_tiles = cluster_id.1 * CLUSTER_SIZE;
                let cluster_max_x = cluster_x_tiles + CLUSTER_SIZE - 1;
                let cluster_max_y = cluster_y_tiles + CLUSTER_SIZE - 1;
                
                // Detect portal direction (including diagonals) - check corners first
                let _direction = if portal.node.x == cluster_x_tiles && portal.node.y == cluster_y_tiles {
                    Direction::SouthWest  // Bottom-left corner
                } else if portal.node.x == cluster_max_x && portal.node.y == cluster_y_tiles {
                    Direction::SouthEast  // Bottom-right corner
                } else if portal.node.x == cluster_x_tiles && portal.node.y == cluster_max_y {
                    Direction::NorthWest  // Top-left corner
                } else if portal.node.x == cluster_max_x && portal.node.y == cluster_max_y {
                    Direction::NorthEast  // Top-right corner
                } else if portal.node.y == cluster_y_tiles {
                    Direction::South  // Bottom edge
                } else if portal.node.y == cluster_max_y {
                    Direction::North  // Top edge
                } else if portal.node.x == cluster_x_tiles {
                    Direction::West  // Left edge
                } else if portal.node.x == cluster_max_x {
                    Direction::East  // Right edge
                } else {
                    continue; // Portal not on cluster edge or corner (shouldn't happen)
                };
                
                // Find which island can access this portal
                // Convert portal position to world coordinates, then to cluster-local
                let portal_world = flow_field.grid_to_world(portal.node.x, portal.node.y);
                
                if let Some(portal_local) = world_to_cluster_local(portal_world, cluster_id, flow_field) {
                    if let Some(region_id) = get_region_id(&cluster.regions, cluster.region_count, portal_local) {
                        // Find which island this region belongs to
                        if let Some(region) = &cluster.regions[region_id.0 as usize] {
                            let _island_id = region.island;
                            
                            // Link this island to this portal in this direction
                            // Note: We need to modify the cluster, so we'll collect updates first
                            // For now, we'll just track (cluster_id, island_idx, direction, portal_id)
                        }
                    }
                }
            }
        }
        
        // Now apply updates (need to do this in a second pass to avoid borrow checker issues)
        let mut updates: Vec<((usize, usize), usize, Direction, usize, IslandId)> = Vec::new();
        
        for (&cluster_id, cluster) in &self.clusters {
            for (&portal_id, portal) in &self.portals {
                if portal.cluster != cluster_id {
                    continue;
                }
                
                let cluster_x_tiles = cluster_id.0 * CLUSTER_SIZE;
                let cluster_y_tiles = cluster_id.1 * CLUSTER_SIZE;
                let cluster_max_x = cluster_x_tiles + CLUSTER_SIZE - 1;
                let cluster_max_y = cluster_y_tiles + CLUSTER_SIZE - 1;
                
                // Detect portal direction - check corners first, then edges
                let direction = if portal.node.x == cluster_max_x && portal.node.y == cluster_max_y {
                    Direction::NorthEast  // Top-right corner
                } else if portal.node.x == cluster_x_tiles && portal.node.y == cluster_max_y {
                    Direction::NorthWest  // Top-left corner
                } else if portal.node.x == cluster_max_x && portal.node.y == cluster_y_tiles {
                    Direction::SouthEast  // Bottom-right corner
                } else if portal.node.x == cluster_x_tiles && portal.node.y == cluster_y_tiles {
                    Direction::SouthWest  // Bottom-left corner
                } else if portal.node.y == cluster_max_y {
                    Direction::North  // Top edge
                } else if portal.node.y == cluster_y_tiles {
                    Direction::South  // Bottom edge
                } else if portal.node.x == cluster_max_x {
                    Direction::East  // Right edge
                } else if portal.node.x == cluster_x_tiles {
                    Direction::West  // Left edge
                } else {
                    continue; // Portal not on this cluster's boundary
                };
                
                let portal_world = flow_field.grid_to_world(portal.node.x, portal.node.y);
                
                if let Some(portal_local) = world_to_cluster_local(portal_world, cluster_id, flow_field) {
                    // Try to find the region this portal is in
                    let region_id = get_region_id(&cluster.regions, cluster.region_count, portal_local);
                    
                    let island_id = if let Some(region_id) = region_id {
                        // Portal is directly in a region - use that island
                        cluster.regions[region_id.0 as usize].as_ref().map(|r| r.island)
                    } else {
                        // Portal is NOT in any region (edge tile near obstacle)
                        // Find the nearest region and use its island
                        // This handles cases where portals are on cluster boundaries near obstacles
                        let mut nearest_island = None;
                        let mut min_distance_sq = 1_000_000.0; // Large enough for pathfinding, won't overflow
                        
                        for region_opt in &cluster.regions[0..cluster.region_count] {
                            if let Some(region) = region_opt {
                                // Calculate distance from portal to region center
                                let center = region.bounds.center();
                                let dx = center.x - portal_local.x;
                                let dy = center.y - portal_local.y;
                                let dist_sq = dx * dx + dy * dy;
                                
                                if dist_sq < FixedNum::from_num(min_distance_sq) {
                                    min_distance_sq = dist_sq.to_num::<f32>();
                                    nearest_island = Some(region.island);
                                }
                            }
                        }
                        
                        if nearest_island.is_none() {
                            warn!("[CONNECTIVITY] Portal {} at ({},{}) in cluster {:?} has no nearby regions!",
                                portal_id, portal.node.x, portal.node.y, cluster_id);
                        }
                        
                        nearest_island
                    };
                    
                    if let Some(island_id) = island_id {
                        let island_idx = island_id.0 as usize;
                        updates.push((cluster_id, island_idx, direction, portal_id, island_id));
                    }
                }
            }
        }
        
        // Apply updates
        for (cluster_id, island_idx, direction, portal_id, island_id) in updates {
            // Store portal->island mapping
            self.portal_island_map.insert(portal_id, island_id);
            
            // Store island->portal mapping in neighbor_connectivity
            if let Some(cluster) = self.clusters.get_mut(&cluster_id) {
                cluster.neighbor_connectivity[island_idx][direction.as_index()] = Some(portal_id);
            }
        }
        
        info!("[CONNECTIVITY] Populated neighbor_connectivity for {} clusters", self.clusters.len());
    }
    
    /// NEW: Build graph using region-based navigation (replaces old portal system)
    /// 
    /// This creates:
    /// 1. Clusters (spatial grid)
    /// 2. Convex regions within each cluster
    /// 3. Islands (connected components based on tortuosity)
    /// 4. Island-aware routing table
    ///
    /// Memory: ~20MB vs ~500MB for old system
    pub fn build_graph_with_regions_sync(&mut self, flow_field: &crate::game::structures::FlowField) {
        use super::region_decomposition::decompose_cluster_into_regions;
        use super::region_connectivity::build_region_connectivity;
        use super::island_detection::identify_islands;
        
        self.reset();
        
        if flow_field.width == 0 || flow_field.height == 0 {
            return;
        }
        
        let width_clusters = (flow_field.width + CLUSTER_SIZE - 1) / CLUSTER_SIZE;
        let height_clusters = (flow_field.height + CLUSTER_SIZE - 1) / CLUSTER_SIZE;
        
        info!("[REGION BUILD] Initializing {} clusters...", width_clusters * height_clusters);
        
        // Phase 1: Create clusters and decompose into regions
        for cy in 0..height_clusters {
            for cx in 0..width_clusters {
                let cluster_id = (cx, cy);
                let mut cluster = Cluster::new(cluster_id);
                
                // Decompose into convex regions
                let regions = decompose_cluster_into_regions(cluster_id, flow_field);
                cluster.region_count = regions.len().min(super::types::MAX_REGIONS);
                
                for (i, region) in regions.into_iter().enumerate().take(super::types::MAX_REGIONS) {
                    cluster.regions[i] = Some(region);
                }
                
                self.clusters.insert(cluster_id, cluster);
            }
        }
        
        let total_regions: usize = self.clusters.values().map(|c| c.region_count).sum();
        info!("[REGION BUILD] Decomposed into {} total regions", total_regions);
        
        // Phase 2: Build region connectivity and local routing within each cluster
        let cluster_ids: Vec<_> = self.clusters.keys().cloned().collect();
        for cluster_id in &cluster_ids {
            if let Some(cluster) = self.clusters.get_mut(cluster_id) {
                build_region_connectivity(cluster);
                identify_islands(cluster);
            }
        }
        
        let total_islands: usize = self.clusters.values().map(|c| c.island_count).sum();
        info!("[REGION BUILD] Identified {} total islands", total_islands);
        
        // Phase 3: Build portals between clusters (for inter-cluster routing)
        // NOTE: We still need portals to connect clusters, but not for flow fields

        {
            // Vertical portals
            for cy in 0..height_clusters {
                for cx in 0..width_clusters.saturating_sub(1) {
                    let min_y = cy * CLUSTER_SIZE;
                    let max_y = ((cy + 1) * CLUSTER_SIZE).min(flow_field.height);
                    let x1 = (cx + 1) * CLUSTER_SIZE - 1;
                    let x2 = (cx + 1) * CLUSTER_SIZE;
                    
                    if x2 >= flow_field.width { continue; }
                    
                    let mut start_segment = None;
                    for y in min_y..max_y {
                        let idx1 = flow_field.get_index(x1, y);
                        let idx2 = flow_field.get_index(x2, y);
                        let walkable = flow_field.cost_field[idx1] != 255 && flow_field.cost_field[idx2] != 255;

                        if walkable {
                            if start_segment.is_none() {
                                start_segment = Some(y);
                            }
                        } else {
                            if let Some(sy) = start_segment {
                                super::cluster::create_portal_vertical(self, x1, x2, sy, y - 1, cx, cy, cx + 1, cy);
                                start_segment = None;
                            }
                        }
                    }
                    if let Some(sy) = start_segment {
                        super::cluster::create_portal_vertical(self, x1, x2, sy, max_y - 1, cx, cy, cx + 1, cy);
                    }
                }
            }

            // Horizontal portals
            for cx in 0..width_clusters {
                for cy in 0..height_clusters.saturating_sub(1) {
                    let min_x = cx * CLUSTER_SIZE;
                    let max_x = ((cx + 1) * CLUSTER_SIZE).min(flow_field.width);
                    let y1 = (cy + 1) * CLUSTER_SIZE - 1;
                    let y2 = (cy + 1) * CLUSTER_SIZE;
                    
                    if y2 >= flow_field.height { continue; }
                    
                    let mut start_segment = None;
                    for x in min_x..max_x {
                        let idx1 = flow_field.get_index(x, y1);
                        let idx2 = flow_field.get_index(x, y2);
                        let walkable = flow_field.cost_field[idx1] != 255 && flow_field.cost_field[idx2] != 255;

                        if walkable {
                            if start_segment.is_none() {
                                start_segment = Some(x);
                            }
                        } else {
                            if let Some(sx) = start_segment {
                                super::cluster::create_portal_horizontal(self, sx, x - 1, y1, y2, cx, cy, cx, cy + 1);
                                start_segment = None;
                            }
                        }
                    }
                    if let Some(sx) = start_segment {
                        super::cluster::create_portal_horizontal(self, sx, max_x - 1, y1, y2, cx, cy, cx, cy + 1);
                    }
                }
            }
            
            // Diagonal portals at cluster corners
            // Each of the 4 clusters at a corner gets its own diagonal portal (like edge portals)
            // But there are only 2 diagonal paths: NE-SW and NW-SE
            
            for cy in 0..height_clusters.saturating_sub(1) {
                for cx in 0..width_clusters.saturating_sub(1) {
                    // Four clusters meet at this corner area
                    // Each cluster needs a portal just inside its own boundary
                    
                    // Check if the corner area is walkable (center of the 2x2 corner area)
                    let check_x = (cx + 1) * CLUSTER_SIZE;
                    let check_y = (cy + 1) * CLUSTER_SIZE;
                    if check_x >= flow_field.width || check_y >= flow_field.height {
                        continue;
                    }
                    
                    // Check multiple tiles around corner for walkability
                    let mut walkable = false;
                    for dx in 0..2 {
                        for dy in 0..2 {
                            let x = check_x.saturating_sub(1) + dx;
                            let y = check_y.saturating_sub(1) + dy;
                            if x < flow_field.width && y < flow_field.height {
                                let idx = flow_field.get_index(x, y);
                                if flow_field.cost_field[idx] != 255 {
                                    walkable = true;
                                }
                            }
                        }
                    }
                    if !walkable {
                        continue;
                    }
                    
                    // Path 1: NE-SW diagonal
                    // Cluster (cx, cy) NE corner connects to Cluster (cx+1, cy+1) SW corner
                    let ne_x = (cx + 1) * CLUSTER_SIZE - 1;  // Max x of cluster (cx, cy)
                    let ne_y = (cy + 1) * CLUSTER_SIZE - 1;  // Max y of cluster (cx, cy)
                    let sw_x = (cx + 1) * CLUSTER_SIZE;      // Min x of cluster (cx+1, cy+1)
                    let sw_y = (cy + 1) * CLUSTER_SIZE;      // Min y of cluster (cx+1, cy+1)
                    
                    // Only create if both positions are in bounds and walkable
                    if ne_x < flow_field.width && ne_y < flow_field.height &&
                       sw_x < flow_field.width && sw_y < flow_field.height {
                        let idx_ne = flow_field.get_index(ne_x, ne_y);
                        let idx_sw = flow_field.get_index(sw_x, sw_y);
                        if flow_field.cost_field[idx_ne] != 255 && flow_field.cost_field[idx_sw] != 255 {
                            super::cluster::create_portal_diagonal(
                                self, ne_x, ne_y, cx, cy,
                                sw_x, sw_y, cx + 1, cy + 1,
                            );
                        }
                    }
                    
                    // Path 2: NW-SE diagonal  
                    // Cluster (cx+1, cy) NW corner connects to Cluster (cx, cy+1) SE corner
                    let nw_x = (cx + 1) * CLUSTER_SIZE;      // Min x of cluster (cx+1, cy)
                    let nw_y = (cy + 1) * CLUSTER_SIZE - 1;  // Max y of cluster (cx+1, cy)
                    let se_x = (cx + 1) * CLUSTER_SIZE - 1;  // Max x of cluster (cx, cy+1)
                    let se_y = (cy + 1) * CLUSTER_SIZE;      // Min y of cluster (cx, cy+1)
                    
                    if nw_x < flow_field.width && nw_y < flow_field.height &&
                       se_x < flow_field.width && se_y < flow_field.height {
                        let idx_nw = flow_field.get_index(nw_x, nw_y);
                        let idx_se = flow_field.get_index(se_x, se_y);
                        if flow_field.cost_field[idx_nw] != 255 && flow_field.cost_field[idx_se] != 255 {
                            super::cluster::create_portal_diagonal(
                                self, nw_x, nw_y, cx + 1, cy,
                                se_x, se_y, cx, cy + 1,
                            );
                        }
                    }
                }
            }
            
            info!("[REGION BUILD] Created {} portals between clusters", self.portals.len());
        }
        
        // Phase 3.5: Link islands to their accessible portals (neighbor_connectivity)
        self.populate_island_portal_connectivity(flow_field);
        
        // Phase 4: Build island-aware routing table
        self.build_island_routing_table();
        
        self.initialized = true;
        info!("[REGION BUILD] Graph build complete!");
    }
    
    // ============================================================================
    // Public API Methods (Cold Path - for tools/editor/debugging)
    // ============================================================================
    
    /// Build the pathfinding graph using the NEW region-based system
    pub fn build_graph(&mut self, flow_field: &crate::game::structures::FlowField, _use_legacy: bool) {
        // Always use new region-based system
        // Legacy parameter kept for backward compatibility but ignored
        self.build_graph_with_regions_sync(flow_field);
    }
    
    /// Get statistics about the graph (for debugging/UI)
    pub fn get_stats(&self) -> GraphStats {

        let portal_count = self.portals.len();
        let cluster_count = self.clusters.len();
        let total_regions: usize = self.clusters.values().map(|c| c.region_count).sum();
        let total_islands: usize = self.clusters.values().map(|c| c.island_count).sum();
        
        GraphStats {
            cluster_count,
            portal_count,
            region_count: total_regions,
            island_count: total_islands,
            initialized: self.initialized,
        }
    }
    
    /// Get the number of clusters
    pub fn cluster_count(&self) -> usize {
        self.clusters.len()
    }
}

/// Statistics about the pathfinding graph
#[derive(Debug, Clone, Copy)]
pub struct GraphStats {
    pub cluster_count: usize,
    pub portal_count: usize,
    pub region_count: usize,
    pub island_count: usize,
    pub initialized: bool,
}
