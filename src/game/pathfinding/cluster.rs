use bevy::prelude::*;
use serde::{Serialize, Deserialize};
use std::collections::BTreeMap;
use crate::game::fixed_math::FixedNum;
use super::types::{Portal, Node};

/// Represents a spatial cluster in the hierarchical pathfinding graph.
///
/// # Flow Field Cache Memory Budget
///
/// Each cluster precomputes and caches flow fields for ALL its portals during graph build.
/// This is a **bounded, eager-caching** strategy:
///
/// - **Memory per cluster:** ~50-100 KB (depends on portal count)
/// - **Total memory (2048x2048 map):** ~335-670 MB for ~6,700 clusters
/// - **Cache invalidation:** Clusters clear cache when obstacles added
/// - **No LRU needed:** Cache is bounded by portal count (typically 4-8 per cluster)
///
/// ## Design Rationale
///
/// **Why precompute everything?**
/// - Flow field generation is expensive (A* + integration field construction)
/// - Portals are fixed after graph build (don't change during gameplay)
/// - Cache hit rate would be ~100% for typical RTS gameplay
/// - Avoids runtime cache misses and LRU overhead
///
/// **When does memory grow?**
/// - Only during graph build (one-time cost)
/// - Never grows during gameplay (portal count is fixed)
/// - Cleared and rebuilt when obstacles added (see Issue #10)
///
/// **Alternative considered:** LRU cache with capacity limit
/// - **Rejected because:** Would add complexity without benefit
/// - Flow fields are needed repeatedly (units path through same clusters)
/// - Cache eviction would cause expensive regeneration mid-game
/// - Memory budget is acceptable for target hardware (< 1 GB total)
///
/// ## Memory Breakdown
///
/// For a 2048x2048 map with 25x25 cluster size:
/// - Clusters: 82 × 82 = 6,724
/// - Portals per cluster: ~4-8 (average ~6)
/// - Flow field size: 625 vectors (16 bytes) + 625 u32s (4 bytes) = 12.5 KB
/// - Memory per cluster: 6 portals × 12.5 KB = 75 KB
/// - **Total cache memory: 6,724 × 75 KB ≈ 504 MB**
///
/// This is acceptable given:
/// - Modern systems have 8+ GB RAM
/// - Game targets large-scale RTS (10M units)
/// - Memory budget prioritizes performance over minimal footprint
///
/// See also: [PATHFINDING.md](documents/Design%20docs/PATHFINDING.md) - Hierarchical pathfinding design
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Cluster {
    pub id: (usize, usize),
    pub portals: Vec<usize>,
    pub flow_field_cache: BTreeMap<usize, super::types::LocalFlowField>,
}

impl Cluster {
    pub fn clear_cache(&mut self) {
        self.flow_field_cache.clear();
    }
    
    pub fn get_flow_field(
        &self,
        portal_id: usize,
    ) -> Option<&super::types::LocalFlowField> {
        self.flow_field_cache.get(&portal_id)
    }

    /// Get flow field for a portal, generating it on-demand if missing.
    /// This should only happen if obstacles were added and regeneration failed.
    /// Logs a warning when generation is needed.
    pub fn get_or_generate_flow_field(
        &mut self,
        portal_id: usize,
        portal: &Portal,
        map_flow_field: &crate::game::structures::FlowField,
    ) -> &super::types::LocalFlowField {
        if !self.flow_field_cache.contains_key(&portal_id) {
            warn!(
                "[FLOW FIELD] Missing flow field for portal {} in cluster {:?} - generating on-demand. \
                This indicates flow field regeneration after obstacle placement may have failed.",
                portal_id, self.id
            );
            // TODO: Consider pausing game with overlay: "Generating missing flow field..."
            let field = super::cluster_flow::generate_local_flow_field(self.id, portal, map_flow_field);
            self.flow_field_cache.insert(portal_id, field);
        }
        &self.flow_field_cache[&portal_id]
    }
}

pub(super) fn create_portal_vertical(
    graph: &mut super::graph::HierarchicalGraph,
    x1: usize, x2: usize,
    y_start: usize, y_end: usize,
    c1x: usize, c1y: usize,
    c2x: usize, c2y: usize,
) {
    let mid_y = (y_start + y_end) / 2;
    
    let id1 = graph.nodes.len();
    graph.nodes.push(Portal { 
        id: id1, 
        node: Node { x: x1, y: mid_y }, 
        range_min: Node { x: x1, y: y_start },
        range_max: Node { x: x1, y: y_end },
        cluster: (c1x, c1y) 
    });
    graph.clusters.entry((c1x, c1y)).or_default().portals.push(id1);

    let id2 = graph.nodes.len();
    graph.nodes.push(Portal { 
        id: id2, 
        node: Node { x: x2, y: mid_y }, 
        range_min: Node { x: x2, y: y_start },
        range_max: Node { x: x2, y: y_end },
        cluster: (c2x, c2y) 
    });
    graph.clusters.entry((c2x, c2y)).or_default().portals.push(id2);

    let cost = FixedNum::from_num(1.0);
    graph.edges.entry(id1).or_default().push((id2, cost));
    graph.edges.entry(id2).or_default().push((id1, cost));
}

pub(super) fn create_portal_horizontal(
    graph: &mut super::graph::HierarchicalGraph,
    x_start: usize, x_end: usize,
    y1: usize, y2: usize,
    c1x: usize, c1y: usize,
    c2x: usize, c2y: usize,
) {
    let mid_x = (x_start + x_end) / 2;
    
    let id1 = graph.nodes.len();
    graph.nodes.push(Portal { 
        id: id1, 
        node: Node { x: mid_x, y: y1 }, 
        range_min: Node { x: x_start, y: y1 },
        range_max: Node { x: x_end, y: y1 },
        cluster: (c1x, c1y) 
    });
    graph.clusters.entry((c1x, c1y)).or_default().portals.push(id1);

    let id2 = graph.nodes.len();
    graph.nodes.push(Portal { 
        id: id2, 
        node: Node { x: mid_x, y: y2 }, 
        range_min: Node { x: x_start, y: y2 },
        range_max: Node { x: x_end, y: y2 },
        cluster: (c2x, c2y) 
    });
    graph.clusters.entry((c2x, c2y)).or_default().portals.push(id2);

    let cost = FixedNum::from_num(1.0);
    graph.edges.entry(id1).or_default().push((id2, cost));
    graph.edges.entry(id2).or_default().push((id1, cost));
}
