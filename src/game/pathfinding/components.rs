use bevy::prelude::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use crate::game::math::{FixedVec2, FixedNum};
use super::graph::HierarchicalGraph;

/// Tracks connected components in the pathfinding graph to detect unreachable regions.
///
/// Solves the problem where pathfinding tries to find paths to unreachable targets
/// (e.g., islands cut off by obstacles, or targets inside obstacles). Without this,
/// A* can loop forever trying to reach impossible destinations.
///
/// # Algorithm
///
/// 1. **Build:** After graph construction, use BFS/DFS to find connected components
/// 2. **Check:** Before pathfinding, verify start and goal are in same component
/// 3. **Fallback:** If unreachable, redirect to closest portal in same component
/// 4. **Update:** Rebuild components when obstacles added/removed
///
/// # Design Decisions
///
/// - **Granularity:** Components at cluster level (not cell level) for efficiency
/// - **Fallback strategy:** Find closest reachable point, don't fail silently
/// - **Memory:** O(clusters) - negligible compared to flow field cache
/// - **Update cost:** O(portals) - acceptable for infrequent obstacle changes
#[derive(Resource, Default, Clone)]
pub struct ConnectedComponents {
    /// Maps each cluster to its component ID
    pub cluster_to_component: BTreeMap<(usize, usize), usize>,
    
    /// For each component, stores representative portal IDs
    /// Used to pick fallback targets when pathfinding to unreachable regions
    pub component_portals: BTreeMap<usize, Vec<usize>>,
    
    /// For each component pair (from, to), stores closest portal IDs in 'from' component
    /// that are physically near the 'to' component (even though not connected)
    /// Used for "get as close as possible" behavior
    pub closest_cross_component: BTreeMap<(usize, usize), Vec<usize>>,
    
    pub initialized: bool,
}

impl ConnectedComponents {
    /// Build connected components from the hierarchical graph using BFS.
    /// Groups clusters into connectivity sets where all clusters in a set can reach each other.
    pub fn build_from_graph(&mut self, graph: &HierarchicalGraph) {
        self.cluster_to_component.clear();
        self.component_portals.clear();
        self.closest_cross_component.clear();
        
        if graph.clusters.is_empty() {
            self.initialized = false;
            return;
        }
        
        let mut component_id = 0;
        let all_clusters: Vec<_> = graph.clusters.keys().cloned().collect();
        
        // Build portal_to_cluster lookup for efficient component traversal
        let mut portal_to_cluster: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
        for (cluster_id, cluster) in &graph.clusters {
            for &portal_id in &cluster.portals {
                portal_to_cluster.insert(portal_id, *cluster_id);
            }
        }
        
        // BFS to find connected components
        for &start_cluster in &all_clusters {
            if self.cluster_to_component.contains_key(&start_cluster) {
                continue; // Already visited
            }
            
            // Start new component
            let mut queue = VecDeque::new();
            queue.push_back(start_cluster);
            self.cluster_to_component.insert(start_cluster, component_id);
            
            let mut component_portal_set = BTreeSet::new();
            
            while let Some(current_cluster) = queue.pop_front() {
                if let Some(cluster) = graph.clusters.get(&current_cluster) {
                    // Add all portals from this cluster to the component
                    for &portal_id in &cluster.portals {
                        component_portal_set.insert(portal_id);
                        
                        // Follow edges to find neighboring portals and their clusters
                        if let Some(edges) = graph.edges.get(&portal_id) {
                            for &(neighbor_portal_id, _cost) in edges {
                                if let Some(&neighbor_cluster) = portal_to_cluster.get(&neighbor_portal_id) {
                                    if !self.cluster_to_component.contains_key(&neighbor_cluster) {
                                        self.cluster_to_component.insert(neighbor_cluster, component_id);
                                        queue.push_back(neighbor_cluster);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            
            // Store portals for this component
            self.component_portals.insert(component_id, component_portal_set.into_iter().collect());
            component_id += 1;
        }
        
        // Precompute closest cross-component portals (for fallback behavior)
        self.compute_closest_cross_component(graph);
        
        self.initialized = true;
        
        info!("[CONNECTIVITY] Built {} connected components covering {} clusters", 
              component_id, all_clusters.len());
    }
    
    /// For each component pair, find portals in the 'from' component that are physically
    /// closest to any portal in the 'to' component (even though not path-connected).
    /// This enables "get as close as possible" behavior when targets are unreachable.
    fn compute_closest_cross_component(&mut self, graph: &HierarchicalGraph) {
        let component_ids: Vec<_> = self.component_portals.keys().cloned().collect();
        
        for &from_comp in &component_ids {
            for &to_comp in &component_ids {
                if from_comp == to_comp {
                    continue; // Same component = already reachable
                }
                
                let from_portals = self.component_portals.get(&from_comp).unwrap();
                let to_portals = self.component_portals.get(&to_comp).unwrap();
                
                // Find closest portal in from_comp to any portal in to_comp
                let mut best_portals: Vec<(usize, FixedNum)> = Vec::new();
                
                for &from_portal_id in from_portals {
                    if let Some(from_portal) = graph.nodes.get(from_portal_id) {
                        let from_pos = FixedVec2::new(
                            FixedNum::from_num(from_portal.node.x as i32),
                            FixedNum::from_num(from_portal.node.y as i32)
                        );
                        
                        let mut min_dist = FixedNum::MAX;
                        for &to_portal_id in to_portals {
                            if let Some(to_portal) = graph.nodes.get(to_portal_id) {
                                let to_pos = FixedVec2::new(
                                    FixedNum::from_num(to_portal.node.x as i32),
                                    FixedNum::from_num(to_portal.node.y as i32)
                                );
                                let dist = (from_pos - to_pos).length_squared();
                                if dist < min_dist {
                                    min_dist = dist;
                                }
                            }
                        }
                        
                        best_portals.push((from_portal_id, min_dist));
                    }
                }
                
                // Sort by distance and keep top 3 closest portals
                best_portals.sort_by(|a, b| a.1.cmp(&b.1));
                let closest: Vec<usize> = best_portals.iter().take(3).map(|(id, _)| *id).collect();
                
                if !closest.is_empty() {
                    self.closest_cross_component.insert((from_comp, to_comp), closest);
                }
            }
        }
    }
    
    /// Get the component ID for a given cluster
    pub fn get_component(&self, cluster: (usize, usize)) -> Option<usize> {
        self.cluster_to_component.get(&cluster).copied()
    }
    
    /// Check if two clusters are in the same connected component
    pub fn are_connected(&self, cluster_a: (usize, usize), cluster_b: (usize, usize)) -> bool {
        if let (Some(comp_a), Some(comp_b)) = (self.get_component(cluster_a), self.get_component(cluster_b)) {
            comp_a == comp_b
        } else {
            false
        }
    }
    
    /// Get fallback portals when trying to path from cluster_a to unreachable cluster_b.
    /// Returns portals in cluster_a's component that are closest to cluster_b's component.
    pub fn get_fallback_portals(&self, cluster_a: (usize, usize), cluster_b: (usize, usize)) -> Option<&Vec<usize>> {
        let comp_a = self.get_component(cluster_a)?;
        let comp_b = self.get_component(cluster_b)?;
        
        if comp_a == comp_b {
            return None; // Already connected
        }
        
        self.closest_cross_component.get(&(comp_a, comp_b))
    }
}
