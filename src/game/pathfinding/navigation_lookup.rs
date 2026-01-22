/// High-performance arena-based navigation lookup system
/// 
/// Provides O(1) lookups from world coordinates to cluster/region/island data
/// using a global uniform grid backed by pre-allocated arenas.

use bevy::prelude::*;
use super::types::{CLUSTER_SIZE, Region, Island, MAX_REGIONS, ClusterArenaIdx, RegionArenaIdx, IslandArenaIdx};
use super::cluster::Cluster;
use super::graph::HierarchicalGraph;

// ============================================================================
// Navigation Lookup Structures
// ============================================================================

/// Cell in the global navigation lookup grid
/// Each cell stores type-safe indices into the respective arenas (not IDs!)
#[derive(Clone, Copy, Debug)]
pub struct NavigationCell {
    /// Index into cluster arena (calculated from position)
    pub cluster_idx: ClusterArenaIdx,
    /// Index into region arena (global: cluster_idx × MAX_REGIONS + local_region_id)
    pub region_idx: RegionArenaIdx,
    /// Index into island arena (global: cluster_idx × MAX_ISLANDS + local_island_id)
    pub island_idx: IslandArenaIdx,
}

impl Default for NavigationCell {
    fn default() -> Self {
        Self {
            cluster_idx: ClusterArenaIdx(0),
            region_idx: RegionArenaIdx(0),
            island_idx: IslandArenaIdx(0),
        }
    }
}

/// Pre-allocated arenas for all navigation data
/// Uses Box<[T]> for stable addressing and cache-friendly sequential layout
pub struct NavigationArenas {
    /// Cluster arena: [num_clusters] pre-allocated at map creation
    /// Size: (map_width / CLUSTER_SIZE) × (map_height / CLUSTER_SIZE)
    pub clusters: Box<[Cluster]>,
    
    /// Region arena: [MAX_REGIONS × num_clusters] pre-allocated for dynamic updates
    /// Allows regions to change when structures are built without reallocation
    pub regions: Box<[Option<Region>]>,
    
    /// Island arena: [MAX_ISLANDS × num_clusters] pre-allocated
    /// Islands can change dynamically when map topology changes
    pub islands: Box<[Option<Island>]>,
    
    /// Metadata for understanding arena layout
    pub num_clusters: usize,
    pub clusters_x: usize,
    pub clusters_y: usize,
}

/// Complete navigation lookup system
/// Combines global grid with pre-allocated arenas for O(1) lookups
#[derive(Resource)]
pub struct NavigationLookup {
    /// Global uniform grid: [map_height][map_width]
    /// Each cell stores indices into the arenas
    /// Memory: map_width × map_height × 12 bytes
    grid: Box<[Box<[NavigationCell]>]>,
    
    /// Pre-allocated arenas for actual navigation data
    pub arenas: NavigationArenas,
    
    /// Grid dimensions (for bounds checking)
    width: usize,
    height: usize,
}

impl NavigationArenas {
    /// Create pre-allocated arenas based on map size
    pub fn new(map_width: usize, map_height: usize) -> Self {
        let clusters_x = (map_width + CLUSTER_SIZE - 1) / CLUSTER_SIZE;
        let clusters_y = (map_height + CLUSTER_SIZE - 1) / CLUSTER_SIZE;
        let num_clusters = clusters_x * clusters_y;
        
        // Pre-allocate clusters (exact size known)
        let clusters = (0..num_clusters)
            .map(|i| {
                let cx = i % clusters_x;
                let cy = i / clusters_x;
                Cluster::new((cx, cy))
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        
        // Pre-allocate regions (MAX_REGIONS per cluster for dynamic updates)
        let max_regions = MAX_REGIONS * num_clusters;
        let regions = vec![None; max_regions].into_boxed_slice();
        
        // Pre-allocate islands (MAX_ISLANDS per cluster for dynamic updates)
        let max_islands = super::types::MAX_ISLANDS * num_clusters;
        let islands = vec![None; max_islands].into_boxed_slice();
        
        Self {
            clusters,
            regions,
            islands,
            num_clusters,
            clusters_x,
            clusters_y,
        }
    }
    
    /// Get cluster by index
    #[inline]
    pub fn get_cluster(&self, idx: ClusterArenaIdx) -> Option<&Cluster> {
        self.clusters.get(idx.0 as usize)
    }
    
    /// Get region by index
    #[inline]
    pub fn get_region(&self, idx: RegionArenaIdx) -> Option<&Region> {
        self.regions.get(idx.0 as usize).and_then(|r| r.as_ref())
    }
    
    /// Get island by index
    #[inline]
    pub fn get_island(&self, idx: IslandArenaIdx) -> Option<&Island> {
        self.islands.get(idx.0 as usize).and_then(|i| i.as_ref())
    }
}

impl NavigationLookup {
    /// Create navigation lookup from map dimensions
    /// This will eventually be populated from existing graph data
    pub fn new(map_width: usize, map_height: usize) -> Self {
        // Create global grid
        let grid = (0..map_height)
            .map(|_| vec![NavigationCell::default(); map_width].into_boxed_slice())
            .collect::<Vec<_>>()
            .into_boxed_slice();
        
        let arenas = NavigationArenas::new(map_width, map_height);
        
        Self {
            grid,
            arenas,
            width: map_width,
            height: map_height,
        }
    }
    
    /// O(1) lookup: world position → navigation data indices
    #[inline]
    pub fn lookup(&self, grid_x: usize, grid_y: usize) -> Option<NavigationCell> {
        if grid_y < self.height && grid_x < self.width {
            Some(self.grid[grid_y][grid_x])
        } else {
            None
        }
    }
    
    // TODO(IMPLEMENT): Populate grid from existing HierarchicalGraph data
    pub fn populate_from_graph(&mut self, graph: &HierarchicalGraph, flow_field: &crate::game::structures::FlowField) {
        info!("[NAV LOOKUP] Populating navigation lookup grid from graph...");
        
        // Step 1: Copy cluster/region/island data into arenas
        let mut region_arena_idx = 0;
        let mut island_arena_idx = 0;
        
        for cy in 0..self.arenas.clusters_y {
            for cx in 0..self.arenas.clusters_x {
                let cluster_idx = cy * self.arenas.clusters_x + cx;
                
                if let Some(source_cluster) = graph.get_cluster(cx, cy) {
                    // Copy cluster data
                    let dest_cluster = &mut self.arenas.clusters[cluster_idx];
                    *dest_cluster = source_cluster.clone();
                    
                    // Copy regions from this cluster into region arena
                    for i in 0..source_cluster.region_count {
                        if let Some(region) = &source_cluster.regions[i] {
                            self.arenas.regions[region_arena_idx] = Some(region.clone());
                            region_arena_idx += 1;
                        }
                    }
                    
                    // Copy islands from this cluster into island arena
                    for i in 0..source_cluster.island_count {
                        if let Some(island) = &source_cluster.islands[i] {
                            self.arenas.islands[island_arena_idx] = Some(island.clone());
                            island_arena_idx += 1;
                        }
                    }
                }
            }
        }
        
        info!("[NAV LOOKUP] Populated {} regions and {} islands into arenas", 
              region_arena_idx, island_arena_idx);
        
        // Step 2: Fill the navigation grid
        // For each grid cell, determine which cluster/region/island it belongs to
        for grid_y in 0..self.height {
            for grid_x in 0..self.width {
                let world_pos = flow_field.grid_to_world(grid_x, grid_y);
                
                // Calculate cluster
                let cluster_x = grid_x / super::types::CLUSTER_SIZE;
                let cluster_y = grid_y / super::types::CLUSTER_SIZE;
                let cluster_idx = ClusterArenaIdx((cluster_y * self.arenas.clusters_x + cluster_x) as u32);
                
                // Look up region and island from cluster data
                if let Some(cluster) = graph.get_cluster(cluster_x, cluster_y) {
                    // Use existing lookup to find region (O(1) via HashMap or grid)
                    let region_id_opt = super::region_decomposition::get_region_id_by_world_pos(cluster, world_pos);
                    
                    if let Some(region_id) = region_id_opt {
                        // Get island from region
                        if let Some(region) = &cluster.regions[region_id.0 as usize] {
                            let island_id = region.island;
                            
                            // Calculate arena indices using type-safe wrappers
                            let region_arena_idx = RegionArenaIdx((cluster_idx.0 as usize * super::types::MAX_REGIONS) as u32 + region_id.0 as u32);
                            let island_arena_idx = IslandArenaIdx((cluster_idx.0 as usize * super::types::MAX_ISLANDS) as u32 + island_id.0 as u32);
                            
                            self.grid[grid_y][grid_x] = NavigationCell {
                                cluster_idx,
                                region_idx: region_arena_idx,
                                island_idx: island_arena_idx,
                            };
                        }
                    }
                }
            }
        }
        
        info!("[NAV LOOKUP] Navigation lookup grid population complete!");
    }
}

impl Default for NavigationLookup {
    fn default() -> Self {
        // Create empty 1x1 lookup (will be replaced when graph is built)
        Self::new(1, 1)
    }
}
