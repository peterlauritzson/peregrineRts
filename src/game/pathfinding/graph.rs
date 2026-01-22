use bevy::prelude::*;
use serde::{Serialize, Deserialize};
use std::collections::{BTreeMap, BinaryHeap};
use crate::game::fixed_math::FixedNum;
use super::types::{CLUSTER_SIZE, Portal, ClusterIslandId, IslandId, MAX_ISLANDS};
use super::cluster::Cluster;
use std::cmp::Reverse;

/// Value indicating no route exists in routing table
const NO_ROUTE: usize = usize::MAX;

/// Hierarchical pathfinding graph for large-scale RTS navigation.
///
/// NEW: Region-Based Navigation with Island Awareness + Arena Optimization
///
/// # Architecture
///
/// 1. **Clustering:** Map divided into CLUSTER_SIZE × CLUSTER_SIZE grids (spatial hash)
/// 2. **Regions:** Each cluster decomposed into convex polygons (5-30 typical)
/// 3. **Islands:** Regions grouped by connectivity (handles U-shaped obstacles)
/// 4. **Local Routing:** [region][region] → next_region per cluster
/// 5. **Global Routing:** [(cluster, island)][(cluster, island)] → next_portal
///
/// # Arena Optimization (2026-01-20)
///
/// Following the spatial_hash 100x speedup approach:
/// - **Clusters:** Array instead of BTreeMap - O(1) access
/// - **Island Routing:** Flattened 1D array instead of nested BTreeMap
/// - **Sized dynamically:** Based on actual map dimensions during build
///
/// # Benefits
///
/// - **Memory:** ~20MB vs ~500MB (96% reduction from flow fields)
/// - **Speed:** O(1) array access vs O(log n) BTreeMap lookups
/// - **Last Mile:** Direct movement in same region (convexity guarantee)
/// - **Island Awareness:** Routes to correct side of obstacles
/// - **Scalability:** O(1) lookups for movement decisions
///
/// # See Also
///
/// - [PATHFINDING.md](documents/Design%20docs/PATHFINDING.md) - Complete design doc
/// - [PATHFINDING_MIGRATION.md](documents/Design%20docs/PATHFINDING_MIGRATION.md) - Migration plan
#[derive(Resource, Serialize, Deserialize, Clone)]
pub struct HierarchicalGraph {
    // Map dimensions
    pub cluster_cols: usize,
    pub cluster_rows: usize,
    pub initialized: bool,
    
    // ARENA: Cluster storage - direct 2D array access
    /// Clusters stored in row-major order: [y * cluster_cols + x]
    /// Option<Cluster> allows sparse maps (None for out-of-bounds clusters)
    pub cluster_storage: Vec<Option<Cluster>>,
    
    // ARENA: Island routing table - flattened for O(1) access
    /// Routes from (cluster, island) to (cluster, island) -> portal_id
    /// Index: source_linear_island_id * total_island_capacity + dest_linear_island_id
    /// NO_ROUTE (usize::MAX) indicates no path exists
    pub island_routing_storage: Vec<usize>,
    
    /// Total capacity for island IDs (cluster_cols * cluster_rows * MAX_ISLANDS)
    pub total_island_capacity: usize,
    
    /// ARENA: Portal registry for inter-cluster navigation (O(1) access by portal ID)
    /// Portal IDs are sequential starting from 0, stored in Vec during building
    /// Vec is acceptable here since portals are built once at map load, never grown during runtime
    pub portals: Vec<Portal>,
    pub next_portal_id: usize,
    
    /// ARENA: Portal-to-island mapping (O(1) access by portal ID)
    /// portals[id] -> which island can access this portal in its cluster
    pub portal_island_map: Vec<Option<IslandId>>,
    
    /// ARENA: Portal-to-portal connections (O(1) access by portal ID)
    /// portals[id] -> Vec of (neighbor_portal_id, cost)
    pub portal_connections: Vec<Vec<(usize, FixedNum)>>,
}

impl Default for HierarchicalGraph {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

impl HierarchicalGraph {
    /// Create a new graph with specified cluster dimensions
    pub fn new(cluster_cols: usize, cluster_rows: usize) -> Self {
        let total_clusters = cluster_cols * cluster_rows;
        let total_island_capacity = total_clusters * MAX_ISLANDS;
        let routing_table_size = total_island_capacity * total_island_capacity;
        
        Self {
            cluster_cols,
            cluster_rows,
            initialized: false,
            cluster_storage: vec![None; total_clusters],
            island_routing_storage: vec![NO_ROUTE; routing_table_size],
            total_island_capacity,
            portals: Vec::new(),
            next_portal_id: 0,
            portal_island_map: Vec::new(),
            portal_connections: Vec::new(),
        }
    }
    
    /// Convert cluster coordinates to linear index
    #[inline]
    fn cluster_to_index(&self, cx: usize, cy: usize) -> usize {
        cy * self.cluster_cols + cx
    }
    
    /// Get cluster by coordinates (O(1) array access)
    #[inline]
    pub fn get_cluster(&self, cx: usize, cy: usize) -> Option<&Cluster> {
        if cx >= self.cluster_cols || cy >= self.cluster_rows {
            return None;
        }
        let idx = self.cluster_to_index(cx, cy);
        self.cluster_storage.get(idx)?.as_ref()
    }
    
    /// Get mutable cluster by coordinates
    #[inline]
    pub fn get_cluster_mut(&mut self, cx: usize, cy: usize) -> Option<&mut Cluster> {
        if cx >= self.cluster_cols || cy >= self.cluster_rows {
            return None;
        }
        let idx = self.cluster_to_index(cx, cy);
        self.cluster_storage.get_mut(idx)?.as_mut()
    }
    
    /// Set cluster at coordinates
    #[inline]
    pub fn set_cluster(&mut self, cx: usize, cy: usize, cluster: Cluster) {
        if cx >= self.cluster_cols || cy >= self.cluster_rows {
            warn!("Attempted to set cluster out of bounds: ({}, {})", cx, cy);
            return;
        }
        let idx = self.cluster_to_index(cx, cy);
        if let Some(slot) = self.cluster_storage.get_mut(idx) {
            *slot = Some(cluster);
        }
    }
    
    /// Convert ClusterIslandId to linear island index for routing table
    #[inline]
    fn island_to_linear_id(&self, cluster_island: ClusterIslandId) -> usize {
        let (cx, cy) = cluster_island.cluster;
        let cluster_idx = cy * self.cluster_cols + cx;
        cluster_idx * MAX_ISLANDS + cluster_island.island.0 as usize
    }
    
    /// Get routing table index for source -> dest lookup
    #[inline]
    fn routing_table_index(&self, source: ClusterIslandId, dest: ClusterIslandId) -> usize {
        let source_linear = self.island_to_linear_id(source);
        let dest_linear = self.island_to_linear_id(dest);
        source_linear * self.total_island_capacity + dest_linear
    }
    
    /// Set route in the flattened routing table (O(1))
    #[inline]
    pub fn set_island_route(&mut self, source: ClusterIslandId, dest: ClusterIslandId, portal_id: usize) {
        let idx = self.routing_table_index(source, dest);
        if let Some(slot) = self.island_routing_storage.get_mut(idx) {
            *slot = portal_id;
        }
    }
    
    /// Get route from the flattened routing table (O(1))
    #[inline]
    pub fn get_island_route(&self, source: ClusterIslandId, dest: ClusterIslandId) -> Option<usize> {
        let idx = self.routing_table_index(source, dest);
        let portal_id = *self.island_routing_storage.get(idx)?;
        if portal_id == NO_ROUTE {
            None
        } else {
            Some(portal_id)
        }
    }
    
    /// Iterator over all valid clusters
    pub fn clusters_iter(&self) -> impl Iterator<Item = ((usize, usize), &Cluster)> {
        self.cluster_storage.iter().enumerate().filter_map(|(idx, cluster_opt)| {
            cluster_opt.as_ref().map(|cluster| {
                let cx = idx % self.cluster_cols;
                let cy = idx / self.cluster_cols;
                ((cx, cy), cluster)
            })
        })
    }
    
    /// Mutable iterator over all valid clusters
    pub fn clusters_iter_mut(&mut self) -> impl Iterator<Item = ((usize, usize), &mut Cluster)> {
        let cols = self.cluster_cols;
        self.cluster_storage.iter_mut().enumerate().filter_map(move |(idx, cluster_opt)| {
            cluster_opt.as_mut().map(|cluster| {
                let cx = idx % cols;
                let cy = idx / cols;
                ((cx, cy), cluster)
            })
        })
    }
    
    pub fn reset(&mut self) {
        self.portals.clear();
        self.next_portal_id = 0;
        self.portal_connections.clear();
        
        // Clear all clusters
        for cluster_slot in &mut self.cluster_storage {
            *cluster_slot = None;
        }
        
        // Reset routing table to NO_ROUTE
        for route in &mut self.island_routing_storage {
            *route = NO_ROUTE;
        }
        
        self.initialized = false;
    }

    
    /// Build island-aware routing table using Dijkstra from each (cluster, island) pair
    pub fn build_island_routing_table(&mut self) {
        // Reset routing table to NO_ROUTE
        for route in &mut self.island_routing_storage {
            *route = NO_ROUTE;
        }
        
        info!("[ROUTING TABLE] Building island-aware routing table...");
        
        // Collect all (cluster, island) pairs from valid clusters
        let mut cluster_islands = Vec::new();
        for (cluster_id, cluster) in self.clusters_iter() {
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
        
        // Count actual routes (exclude NO_ROUTE entries)
        let total_entries = self.island_routing_storage.iter().filter(|&&r| r != NO_ROUTE).count();
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
            let (cx, cy) = current.cluster;
            if let Some(cluster) = self.get_cluster(cx, cy) {
                let mut found_any_portal = false;
                // Get portals accessible from this island
                for direction in super::types::Direction::ALL {
                    if let Some(portal_id) = cluster.neighbor_connectivity[current.island.0 as usize][direction.as_index()] {
                        found_any_portal = true;
                        // Find which (cluster, island) this portal leads to
                        if portal_id < self.portals.len() {
                            // Find the connected portal (cross-cluster edge)
                            if portal_id < self.portal_connections.len() {
                                for &(neighbor_portal_id, edge_cost) in &self.portal_connections[portal_id] {
                                    if let Some(neighbor_portal) = self.portals.get(neighbor_portal_id) {
                                        let neighbor_cluster = neighbor_portal.cluster;
                                        
                                        // Determine which island in the neighbor cluster this portal connects to
                                        // Use the portal_island_map we populated earlier
                                        if let Some(Some(neighbor_island)) = self.portal_island_map.get(neighbor_portal_id) {
                                            let neighbor = ClusterIslandId::new(neighbor_cluster, *neighbor_island);
                                            
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
        
        // Store routing table for this source using the flattened arena
        for (dest, portal_id) in next_portal {
            self.set_island_route(source, dest, portal_id);
        }
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
        
        // O(1) array lookup instead of nested BTreeMap
        self.get_island_route(current, goal)
    }
    
    /// Populate neighbor_connectivity: link each island to portals in each direction
    /// 
    /// For each cluster, determines which portals each island can access.
    /// Uses Direction enum for type-safe indexing (North=0, South=1, East=2, West=3)
    fn populate_island_portal_connectivity(&mut self, flow_field: &crate::game::structures::FlowField) {
        use super::region_decomposition::{get_region_id, world_to_cluster_local};
        use super::types::Direction;
        
        info!("[CONNECTIVITY] Linking islands to their boundary portals...");
        
        // Collect all clusters first to avoid borrow checker issues
        let cluster_ids: Vec<_> = self.clusters_iter().map(|(id, _)| id).collect();
        
        // For each cluster
        for &cluster_id in &cluster_ids {
            let (cx, cy) = cluster_id;
            
            // For each portal owned by this cluster
            for (_portal_id, portal) in self.portals.iter().enumerate() {
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
                    if let Some(cluster) = self.get_cluster(cx, cy) {
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
        }
        
        // Now apply updates (need to do this in a second pass to avoid borrow checker issues)
        let mut updates: Vec<((usize, usize), usize, Direction, usize, IslandId)> = Vec::new();
        
        for (cluster_id, cluster) in self.clusters_iter() {
            for (portal_id, portal) in self.portals.iter().enumerate() {
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
            // Ensure capacity
            while self.portal_island_map.len() <= portal_id {
                self.portal_island_map.push(None);
            }
            self.portal_island_map[portal_id] = Some(island_id);
            
            // Store island->portal mapping in neighbor_connectivity
            let (cx, cy) = cluster_id;
            if let Some(cluster) = self.get_cluster_mut(cx, cy) {
                cluster.neighbor_connectivity[island_idx][direction.as_index()] = Some(portal_id);
            }
        }
        
        let cluster_count = self.cluster_storage.iter().filter(|c| c.is_some()).count();
        info!("[CONNECTIVITY] Populated neighbor_connectivity for {} clusters", cluster_count);
    }
    
    /// Populate NavigationRouting resource with routing tables from graph
    /// 
    /// Copies data from HierarchicalGraph into NavigationRouting arenas:
    /// - Island routing: global island-to-island routing table
    /// - Region routing: cluster-local region-to-region routing tables
    fn populate_navigation_routing(&self, routing: &mut super::navigation_routing::NavigationRouting) {
        use super::types::{ClusterArenaIdx, IslandArenaIdx, LocalRegionId};
        
        info!("[NAV ROUTING] Populating routing tables from graph...");
        
        // Copy island routing table (macro-level: island → island → portal)
        // The graph's island_routing_storage is already in the correct format
        for cy in 0..self.cluster_rows {
            for cx in 0..self.cluster_cols {
                let cluster_idx = cy * self.cluster_cols + cx;
                
                if let Some(cluster) = self.get_cluster(cx, cy) {
                    // For each island in this cluster
                    for island_idx in 0..cluster.island_count {
                        if let Some(_island) = &cluster.islands[island_idx] {
                            let source_global_island_idx = IslandArenaIdx((cluster_idx * MAX_ISLANDS + island_idx) as u32);
                            
                            // For each possible destination island
                            for dest_cy in 0..self.cluster_rows {
                                for dest_cx in 0..self.cluster_cols {
                                    let dest_cluster_idx = dest_cy * self.cluster_cols + dest_cx;
                                    
                                    if let Some(dest_cluster) = self.get_cluster(dest_cx, dest_cy) {
                                        for dest_island_idx in 0..dest_cluster.island_count {
                                            if dest_cluster.islands[dest_island_idx].is_some() {
                                                let dest_global_island_idx = IslandArenaIdx((dest_cluster_idx * MAX_ISLANDS + dest_island_idx) as u32);
                                                
                                                // Look up portal from graph's routing table
                                                let source_id = ClusterIslandId::new((cx, cy), IslandId(island_idx as u8));
                                                let dest_id = ClusterIslandId::new((dest_cx, dest_cy), IslandId(dest_island_idx as u8));
                                                
                                                if let Some(portal_id) = self.get_next_portal_for_island(source_id, dest_id) {
                                                    routing.island_routing.set_route(source_global_island_idx, dest_global_island_idx, portal_id);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        info!("[NAV ROUTING] Populated island routing table");
        
        // Copy region routing tables (meso-level: cluster/region → cluster/region → next_region)
        // Each cluster has local_routing[start_region][end_region] = next_region
        for cy in 0..self.cluster_rows {
            for cx in 0..self.cluster_cols {
                let cluster_idx = ClusterArenaIdx((cy * self.cluster_cols + cx) as u32);
                
                if let Some(cluster) = self.get_cluster(cx, cy) {
                    // Copy this cluster's local routing table
                    for start_region in 0..cluster.region_count {
                        for end_region in 0..cluster.region_count {
                            let next_region_u8 = cluster.local_routing[start_region][end_region];
                            
                            // For intra-cluster routing, both start and end cluster are the same
                            routing.region_routing.set_route(
                                cluster_idx,
                                cluster_idx,
                                LocalRegionId(start_region as u8),
                                LocalRegionId(end_region as u8),
                                LocalRegionId(next_region_u8),
                            );
                        }
                    }
                }
            }
        }
        
        info!("[NAV ROUTING] Populated region routing tables");
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
    pub fn build_graph_with_regions_sync(
        &mut self, 
        flow_field: &crate::game::structures::FlowField, 
        nav_lookup: Option<&mut super::navigation_lookup::NavigationLookup>,
        nav_routing: Option<&mut super::navigation_routing::NavigationRouting>
    ) {
        use super::region_decomposition::decompose_cluster_into_regions;
        use super::region_decomposition::build_region_lookup_grid;
        use super::region_connectivity::build_region_connectivity;
        use super::island_detection::identify_islands;
        
        self.reset();
        
        if flow_field.width == 0 || flow_field.height == 0 {
            return;
        }
        
        let width_clusters = (flow_field.width + CLUSTER_SIZE - 1) / CLUSTER_SIZE;
        let height_clusters = (flow_field.height + CLUSTER_SIZE - 1) / CLUSTER_SIZE;
        
        // Initialize graph storage based on actual map dimensions
        // This is done once at map load, so we size it exactly to what we need
        if self.cluster_cols != width_clusters || self.cluster_rows != height_clusters {
            info!("[GRAPH BUILD] Initializing arena for {}x{} clusters", width_clusters, height_clusters);
            *self = Self::new(width_clusters, height_clusters);
        }
        
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
                
                // PERF: Build region lookup grid for O(1) region queries
                build_region_lookup_grid(&mut cluster, cluster_id, flow_field);
                
                self.set_cluster(cx, cy, cluster);
            }
        }
        
        let total_regions: usize = self.clusters_iter().map(|(_, c)| c.region_count).sum();
        info!("[REGION BUILD] Decomposed into {} total regions", total_regions);
        
        // Phase 2: Build region connectivity and local routing within each cluster
        let cluster_ids: Vec<_> = self.clusters_iter().map(|(id, _)| id).collect();
        for &(cx, cy) in &cluster_ids {
            if let Some(cluster) = self.get_cluster_mut(cx, cy) {
                build_region_connectivity(cluster);
                identify_islands(cluster);
            }
        }
        
        let total_islands: usize = self.clusters_iter().map(|(_, c)| c.island_count).sum();
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
                                super::cluster::create_portal_vertical(self, x1, x2, sy, y - 1, cx, cy, cx + 1, cy, flow_field);
                                start_segment = None;
                            }
                        }
                    }
                    if let Some(sy) = start_segment {
                        super::cluster::create_portal_vertical(self, x1, x2, sy, max_y - 1, cx, cy, cx + 1, cy, flow_field);
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
                                super::cluster::create_portal_horizontal(self, sx, x - 1, y1, y2, cx, cy, cx, cy + 1, flow_field);
                                start_segment = None;
                            }
                        }
                    }
                    if let Some(sx) = start_segment {
                        super::cluster::create_portal_horizontal(self, sx, max_x - 1, y1, y2, cx, cy, cx, cy + 1, flow_field);
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
                                flow_field,
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
                                flow_field,
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
        
        // Phase 5: Populate navigation lookup for O(1) queries
        if let Some(lookup) = nav_lookup {
            lookup.populate_from_graph(self, flow_field);
        }
        
        // Phase 6: Populate navigation routing tables for O(1) path queries
        if let Some(routing) = nav_routing {
            self.populate_navigation_routing(routing);
        }
    }
    
    // ============================================================================
    // Public API Methods (Cold Path - for tools/editor/debugging)
    // ============================================================================
    
    /// Build the pathfinding graph using the NEW region-based system
    pub fn build_graph(&mut self, flow_field: &crate::game::structures::FlowField, _use_legacy: bool, nav_lookup: Option<&mut super::navigation_lookup::NavigationLookup>) {
        // Always use new region-based system
        // Legacy parameter kept for backward compatibility but ignored
        self.build_graph_with_regions_sync(flow_field, nav_lookup, None);
    }
    
    /// Get statistics about the graph (for debugging/UI)
    pub fn get_stats(&self) -> GraphStats {
        let portal_count = self.portals.len();
        let cluster_count = self.cluster_storage.iter().filter(|c| c.is_some()).count();
        let total_regions: usize = self.clusters_iter().map(|(_, c)| c.region_count).sum();
        let total_islands: usize = self.clusters_iter().map(|(_, c)| c.island_count).sum();
        
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
        self.cluster_storage.iter().filter(|c| c.is_some()).count()
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
