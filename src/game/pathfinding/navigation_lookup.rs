/// High-performance arena-based navigation lookup system
/// 
/// Provides O(1) lookups from world coordinates to cluster/region/island data
/// using a global uniform grid backed by pre-allocated arenas.

use bevy::prelude::*;
use super::types::{CLUSTER_SIZE, Region, Island, MAX_REGIONS, ClusterArenaIdx, RegionArenaIdx, IslandArenaIdx};
use super::cluster::Cluster;
use super::graph::HierarchicalGraph;
use crate::game::fixed_math::{FixedVec2, FixedNum};

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
        
        // Step 0: Resize grid and arenas to match actual map dimensions
        let map_width = flow_field.width;
        let map_height = flow_field.height;
        
        if self.width != map_width || self.height != map_height {
            info!("[NAV LOOKUP] Resizing grid from {}×{} to {}×{}", 
                  self.width, self.height, map_width, map_height);
            
            // Recreate grid with correct dimensions
            self.grid = (0..map_height)
                .map(|_| vec![NavigationCell::default(); map_width].into_boxed_slice())
                .collect::<Vec<_>>()
                .into_boxed_slice();
            
            // Recreate arenas with correct dimensions
            self.arenas = NavigationArenas::new(map_width, map_height);
            
            // Update stored dimensions
            self.width = map_width;
            self.height = map_height;
        }
        
        // Step 1: Copy cluster/region/island data into arenas using BLOCKED indexing
        // Regions: cluster_idx × MAX_REGIONS + local_region_id
        // Islands: cluster_idx × MAX_ISLANDS + local_island_id
        for cy in 0..self.arenas.clusters_y {
            for cx in 0..self.arenas.clusters_x {
                let cluster_idx = super::types::ClusterArenaIdx::from_coords(cx, cy, self.arenas.clusters_x);
                
                if let Some(source_cluster) = graph.get_cluster(cx, cy) {
                    // Copy cluster data
                    let dest_cluster = &mut self.arenas.clusters[cluster_idx.0 as usize];
                    *dest_cluster = source_cluster.clone();
                    
                    // Copy regions using blocked indexing
                    for i in 0..source_cluster.region_count {
                        if let Some(region) = &source_cluster.regions[i] {
                            let region_arena_idx = super::types::RegionArenaIdx::from_cluster_and_local(
                                cluster_idx,
                                region.id
                            );
                            self.arenas.regions[region_arena_idx.0 as usize] = Some(region.clone());
                        }
                    }
                    
                    // Copy islands using blocked indexing
                    for i in 0..source_cluster.island_count {
                        if let Some(island) = &source_cluster.islands[i] {
                            let island_arena_idx = super::types::IslandArenaIdx::from_cluster_and_local(
                                cluster_idx,
                                island.id
                            );
                            self.arenas.islands[island_arena_idx.0 as usize] = Some(island.clone());
                        }
                    }
                }
            }
        }
        
        info!("[NAV LOOKUP] Populated arenas with blocked indexing");
        
        // Debug: Check if region_world_lookup HashMaps are populated
        let mut total_hash_entries = 0;
        for cluster in self.arenas.clusters.iter() {
            total_hash_entries += cluster.region_world_lookup.len();
        }
        info!("[NAV LOOKUP] Total region_world_lookup entries across all clusters: {}", total_hash_entries);
        
        // Step 2: Fill the navigation grid
        // For each grid cell, determine which cluster/region/island it belongs to
        for grid_y in 0..self.height {
            for grid_x in 0..self.width {
                // Calculate cluster
                let cluster_x = grid_x / super::types::CLUSTER_SIZE;
                let cluster_y = grid_y / super::types::CLUSTER_SIZE;
                let cluster_idx = super::types::ClusterArenaIdx::from_coords(
                    cluster_x, 
                    cluster_y, 
                    self.arenas.clusters_x
                );
                
                // Initialize with cluster_idx - every cell belongs to a cluster
                // Region/island indices default to 0 if no region is found (unwalkable cells)
                let mut nav_cell = NavigationCell {
                    cluster_idx,
                    region_idx: RegionArenaIdx(0),
                    island_idx: IslandArenaIdx(0),
                };
                
                // Look up region and island from cluster data (use arenas, not graph!)
                // The arenas were already populated in Step 1 with cloned cluster data
                let cluster = &self.arenas.clusters[cluster_idx.0 as usize];
                {
                    // Use fast local grid lookup instead of world position hashmap
                    // This is faster and avoids floating point quantization issues
                    let local_x = grid_x % super::types::CLUSTER_SIZE;
                    let local_y = grid_y % super::types::CLUSTER_SIZE;
                    
                    // Manually lookup region using local coordinates because the prebuilt 
                    // region_lookup_grid in the cluster might be empty due to coordinate 
                    // space issues during build (it expects global coords but regions are local).
                    // Regions are defined in [0..CLUSTER_SIZE] local space.
                    let local_point = FixedVec2::new(
                        FixedNum::from_num(local_x) + FixedNum::from_num(0.5),
                        FixedNum::from_num(local_y) + FixedNum::from_num(0.5)
                    );
                    
                    let region_id_opt = super::region_decomposition::get_region_id(
                        &cluster.regions, 
                        cluster.region_count, 
                        local_point
                    );
                    
                    if let Some(region_id) = region_id_opt {
                        // Get island from region
                        if let Some(region) = &cluster.regions[region_id.0 as usize] {
                            let island_id = region.island;
                            
                            // Calculate arena indices using type-safe utility methods
                            let region_arena_idx = super::types::RegionArenaIdx::from_cluster_and_local(
                                cluster_idx,
                                region_id
                            );
                            let island_arena_idx = super::types::IslandArenaIdx::from_cluster_and_local(
                                cluster_idx,
                                island_id
                            );
                            
                            nav_cell.region_idx = region_arena_idx;
                            nav_cell.island_idx = island_arena_idx;
                        }
                    }
                }
                
                self.grid[grid_y][grid_x] = nav_cell;
            }
        }
        
        // Log statistics about populated cells
        let mut cells_with_regions = 0;
        let mut cells_without_regions = 0;
        for row in self.grid.iter() {
            for cell in row.iter() {
                if cell.region_idx.0 != 0 {
                    cells_with_regions += 1;
                } else {
                    cells_without_regions += 1;
                }
            }
        }
        
        info!("[NAV LOOKUP] Navigation lookup grid population complete!");
        info!("[NAV LOOKUP] Cells with regions: {}, cells without regions (unwalkable): {}", 
            cells_with_regions, cells_without_regions);
    }
}

impl Default for NavigationLookup {
    fn default() -> Self {
        // TODO(PERF): This allocates a 1×1 grid that gets immediately discarded and replaced
        // when populate_from_graph() resizes to the actual map dimensions. This causes:
        // 1. Double allocation (waste)
        // 2. Heap fragmentation (the 1×1 allocation becomes garbage)
        // 3. For large maps (~12MB for 1000×1000), we want clean contiguous allocation
        // 
        // Better approach: Use lazy initialization (empty/None state) until map is loaded,
        // then allocate once with correct dimensions. Would avoid throw-away allocations
        // and improve memory locality over long play sessions.
        Self::new(1, 1)
    }
}
