/// High-performance routing tables for pathfinding
/// 
/// Provides O(1) lookups for:
/// - Island-to-island routing (macro/global level)
/// - Region-to-region routing (meso/local level)

use bevy::prelude::*;
use super::types::{MAX_REGIONS, MAX_ISLANDS, ClusterArenaIdx, IslandArenaIdx, LocalRegionId};

/// Value indicating no route exists
const NO_ROUTE: usize = usize::MAX;
const NO_REGION: u8 = 255;

/// Island-to-island routing arena
/// 
/// Maps [source_island_idx][dest_island_idx] -> portal_id
/// Size: (NUM_CLUSTERS * MAX_ISLANDS)² entries
pub struct IslandRoutingArena {
    /// Flattened 2D array: [source * total_capacity + dest]
    /// Value: portal_id or NO_ROUTE if unreachable
    routing: Box<[usize]>,
    
    /// Total number of island slots (NUM_CLUSTERS * MAX_ISLANDS)
    total_island_capacity: usize,
}

impl IslandRoutingArena {
    /// Create island routing arena for a map
    pub fn new(num_clusters: usize) -> Self {
        let total_island_capacity = num_clusters * MAX_ISLANDS;
        let total_entries = total_island_capacity * total_island_capacity;
        
        Self {
            routing: vec![NO_ROUTE; total_entries].into_boxed_slice(),
            total_island_capacity,
        }
    }
    
    /// Find next portal to travel from source island to destination island
    /// 
    /// Takes global island arena indices (island_idx from NavigationCell)
    /// Type-safe API ensures correct index computation at compile time
    /// 
    /// # Returns
    /// Portal ID to use for next step, or None if unreachable
    #[inline]
    pub fn find_next_portal(&self, start_island: IslandArenaIdx, goal_island: IslandArenaIdx) -> Option<usize> {
        let idx = start_island.0 as usize * self.total_island_capacity + goal_island.0 as usize;
        let portal_id = *self.routing.get(idx)?;
        
        if portal_id == NO_ROUTE {
            None
        } else {
            Some(portal_id)
        }
    }
    
    /// Set route in the island routing table (used during graph building)
    #[inline]
    pub fn set_route(&mut self, start_island: IslandArenaIdx, goal_island: IslandArenaIdx, portal_id: usize) {
        let idx = start_island.0 as usize * self.total_island_capacity + goal_island.0 as usize;
        if let Some(slot) = self.routing.get_mut(idx) {
            *slot = portal_id;
        }
    }
}

/// Region-to-region routing arena
/// 
/// Maps [start_cluster_idx][end_cluster_idx][start_region_idx][end_region_idx] -> next_region_id
/// This is a 4D structure flattened into a 1D array
/// Size: NUM_CLUSTERS * NUM_CLUSTERS * MAX_REGIONS * MAX_REGIONS entries
pub struct RegionRoutingArena {
    /// Flattened 4D array
    /// Index calculation: start_cluster * (num_clusters * MAX_REGIONS * MAX_REGIONS) 
    ///                  + end_cluster * (MAX_REGIONS * MAX_REGIONS)
    ///                  + start_region * MAX_REGIONS
    ///                  + end_region
    routing: Box<[u8]>,
    
    /// Number of clusters in the map
    num_clusters: usize,
}

impl RegionRoutingArena {
    /// Create region routing arena for a map
    pub fn new(num_clusters: usize) -> Self {
        let total_entries = num_clusters * num_clusters * MAX_REGIONS * MAX_REGIONS;
        
        Self {
            routing: vec![NO_REGION; total_entries].into_boxed_slice(),
            num_clusters,
        }
    }
    
    /// Get next region to move toward when navigating from start to end
    /// 
    /// Takes cluster indices and LOCAL region IDs (not global region_idx)
    /// Type-safe API ensures correct index computation at compile time
    /// 
    /// # Returns
    /// Next local region ID to move toward, or None if unreachable (different islands)
    #[inline]
    pub fn get_next_region(
        &self,
        start_cluster: ClusterArenaIdx,
        end_cluster: ClusterArenaIdx,
        start_region: LocalRegionId,
        end_region: LocalRegionId,
    ) -> Option<LocalRegionId> {
        let idx = start_cluster.0 as usize * (self.num_clusters * MAX_REGIONS * MAX_REGIONS)
            + end_cluster.0 as usize * (MAX_REGIONS * MAX_REGIONS)
            + start_region.0 as usize * MAX_REGIONS
            + end_region.0 as usize;
        
        let next_region = *self.routing.get(idx)?;
        
        if next_region == NO_REGION {
            None
        } else {
            Some(LocalRegionId(next_region))
        }
    }
    
    /// Set route in the region routing table (used during graph building)
    #[inline]
    pub fn set_route(
        &mut self,
        start_cluster: ClusterArenaIdx,
        end_cluster: ClusterArenaIdx,
        start_region: LocalRegionId,
        end_region: LocalRegionId,
        next_region: LocalRegionId,
    ) {
        let idx = start_cluster.0 as usize * (self.num_clusters * MAX_REGIONS * MAX_REGIONS)
            + end_cluster.0 as usize * (MAX_REGIONS * MAX_REGIONS)
            + start_region.0 as usize * MAX_REGIONS
            + end_region.0 as usize;
        
        if let Some(slot) = self.routing.get_mut(idx) {
            *slot = next_region.0;
        }
    }
}

/// Complete navigation routing system
/// Combines island and region routing for hierarchical pathfinding
#[derive(Resource)]
pub struct NavigationRouting {
    /// Island-to-island routing (macro level)
    pub island_routing: IslandRoutingArena,
    
    /// Region-to-region routing (meso level)
    pub region_routing: RegionRoutingArena,
}

impl NavigationRouting {
    /// Create navigation routing for a map
    pub fn new(num_clusters: usize) -> Self {
        Self {
            island_routing: IslandRoutingArena::new(num_clusters),
            region_routing: RegionRoutingArena::new(num_clusters),
        }
    }

    /// Check if routing tables are sized correctly for the given number of clusters
    pub fn is_sized_correctly(&self, num_clusters: usize) -> bool {
        let required_capacity = num_clusters * MAX_ISLANDS;
        self.island_routing.total_island_capacity == required_capacity
    }

    /// Resize routing tables to accommodate the specified number of clusters.
    /// Warning: This clears all existing routing data!
    pub fn resize(&mut self, num_clusters: usize) {
        *self = Self::new(num_clusters);
    }
}

impl Default for NavigationRouting {
    fn default() -> Self {
        // Create empty 1-cluster routing (will be replaced when graph is built)
        Self::new(1)
    }
}
