# Pathfinding Migration Guide: Portal-Based HPA* → Convex Region Decomposition

**Status:** Planning Document  
**Created:** January 16, 2026  
**Target:** Implement the design from [PATHFINDING.md](PATHFINDING.md)

---

## Overview

This document provides a step-by-step migration plan to transform the current portal-based hierarchical pathfinding system into a convex region decomposition system with proper "last mile" navigation.

### Current System (What We Have)

**Architecture:**
- **Clusters:** 25×25 tile grids
- **Portals:** Transition points between adjacent clusters (single position + range)
- **Inter-cluster:** Flow fields from any position to portals
- **Routing:** Precomputed cluster→cluster routing table
- **Path Storage:** `Path::Hierarchical { goal, goal_cluster }`

**Problems:**
1. **"Last Mile" Issue:** When unit and target are in the same cluster, there's no proper pathfinding - just flow fields to portals or direct movement
2. **No convexity guarantees:** Units can get stuck or move inefficiently within clusters
3. **No island awareness at cluster level:** Can enter clusters on the wrong side of obstacles
4. **Flow fields are memory-heavy:** 12.5KB per portal, ~75KB per cluster, ~504MB total for 2048×2048 map

### Target System (What We Want)

**Architecture:**
- **Clusters:** Same 25×25 tile grids (for spatial hashing)
- **Regions:** Convex polygons/rectangles within each cluster (5-30 per cluster)
- **Islands:** Connected components of regions (handles U-shaped obstacles)
- **Local Routing:** `[region][region] → next_region` per cluster
- **Global Routing:** `[(cluster, island)][(cluster, island)] → portal_id`

**Benefits:**
1. **Solves "Last Mile":** Direct movement in same region (convexity), clamped projection between regions
2. **Island awareness:** Routes to correct side of obstacles automatically
3. **Lower memory:** ~3KB per cluster vs ~75KB, ~5MB total vs ~504MB
4. **Simpler logic:** No flow field generation/caching, just lookup tables

---

## Implementation Strategy: Choose Your Approach

The migration can be done with different levels of smoothing sophistication. Choose based on your priorities:

### Strategy A: Pure Regions (Recommended Start)

**What:** Implement Phases 1-6 exactly as written
- Convex region decomposition
- Clamped projection for portal crossing
- Region-based movement

**Pros:**
- Simplest to implement and debug
- Lowest memory footprint (~5MB)
- Best for dynamic obstacles

**Cons:**
- Movement may look slightly "straight-line" without smoothing
- Requires tuning clamped projection for good corner cutting

**When to choose:** You want fastest implementation, lowest memory, or have complex/dynamic maps

### Strategy B: Regions + Portal Flow Fields (Hybrid)

**What:** Implement Phases 1-6, then add optional Phase 5.5
- Region system as fallback
- Portal-to-portal flow fields for large open clusters
- Anticipatory blending at cluster boundaries

**Pros:**
- Smooth, organic movement in open areas
- Robust handling of complex areas (via regions)
- Best visual quality

**Cons:**
- ~25MB memory (still much less than current 504MB)
- More complex implementation

**When to choose:** Visual quality is critical, map has mix of open and complex areas

### Strategy C: Enhanced Regions (Recommended Final)

**What:** Implement Phases 1-6 with these additions:
- Clamped projection with "look-through" (projects target onto portal, not just next region)
- Anticipatory blending at cluster boundaries
- Wall repulsion during region decomposition

**Pros:**
- Nearly as smooth as flow fields
- Still only ~5MB memory
- Simpler than full hybrid

**Cons:**
- Requires careful tuning of projection and blending

**When to choose:** Balance between simplicity and quality

### Decision Matrix

| Priority | Memory Budget | Map Type | Recommended Strategy |
|----------|---------------|----------|---------------------|
| Fast Implementation | Any | Any | Strategy A |
| Best Visual Quality | 25-50MB | Mixed open/complex | Strategy B |
| Dynamic Obstacles | <10MB | Indoor/complex | Strategy A or C |
| Large Open Battles | 25-50MB | Mostly outdoor | Strategy B |
| Balanced | <10MB | Any | Strategy C |

### Migration Flexibility

**You can always upgrade:**
- Start with Strategy A (pure regions)
- Test with real gameplay
- Add Strategy C enhancements if movement looks too robotic
- Add Strategy B flow fields only for specific large clusters if needed

**The migration is designed to support all three:**
- Phases 1-4: Core infrastructure (same for all strategies)
- Phase 5: Basic movement (Strategy A)
- Phase 5.5: Optional flow fields (Strategy B)
- Smoothing techniques documented in design doc (Strategy C)

---

## Migration Phases

We'll migrate in **7 phases**, keeping the system functional at each step.

---

## Phase 1: Add Region Data Structures (Preparation)

**Goal:** Add new data structures without breaking existing code

**Estimated Time:** 2-4 hours  
**Risk:** Low - purely additive changes

### 1.1 Update `types.rs`

**Add:**
```rust
/// Maximum number of regions per cluster.
/// Typical clusters: 1-10 regions (open terrain vs. complex rooms)
/// Complex clusters: up to 32 regions (mazes, tight corridors)
pub const MAX_REGIONS: usize = 32;

/// Maximum number of islands (connected components) per cluster.
/// Most clusters: 1 island (fully connected)
/// Split clusters: 2-3 islands (river, wall, U-shaped building)
pub const MAX_ISLANDS: usize = 4;

/// Tortuosity threshold for splitting islands.
/// If path_distance / euclidean_distance > this value, regions are separate islands.
pub const TORTUOSITY_THRESHOLD: f32 = 3.0;

/// A convex polygon/rectangle representing a navigable region within a cluster.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Region {
    /// Bounding rectangle (for point-in-region fast rejection)
    pub bounds: Rect,
    
    /// Full polygon vertices (if more complex than rectangle)
    /// For rectangles, this is just the 4 corners
    pub vertices: SmallVec<[FixedVec2; 8]>,
    
    /// Which island (connected component) this region belongs to
    pub island_id: IslandId,
    
    /// Portals to adjacent regions (shared edges)
    pub portals: SmallVec<[RegionPortal; 8]>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct IslandId(pub u8);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegionPortal {
    /// The shared edge between this region and the next
    pub edge: LineSegment,
    
    /// Which region this portal leads to
    pub next_region: RegionId,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct RegionId(pub u8);

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct LineSegment {
    pub start: FixedVec2,
    pub end: FixedVec2,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Rect {
    pub min: FixedVec2,
    pub max: FixedVec2,
}

impl Rect {
    pub fn contains(&self, point: FixedVec2) -> bool {
        point.x >= self.min.x && point.x <= self.max.x &&
        point.y >= self.min.y && point.y <= self.max.y
    }
}

/// Lookup table: NO_PATH indicates regions are not connected (different islands)
pub const NO_PATH: u8 = 255;
```

**Why:** Foundation for region-based navigation, doesn't interfere with existing portal system

### 1.2 Update `cluster.rs`

**Add to `Cluster` struct:**
```rust
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Cluster {
    pub id: (usize, usize),
    
    // === OLD SYSTEM (will be removed in Phase 7) ===
    pub portals: Vec<usize>,  // Portal IDs to neighboring clusters
    pub flow_field_cache: BTreeMap<usize, super::types::LocalFlowField>,
    
    // === NEW SYSTEM ===
    /// Convex regions within this cluster (fixed-size for pre-allocation)
    pub regions: [Option<Region>; MAX_REGIONS],
    pub region_count: usize,
    
    /// Local routing table: [from_region][to_region] = next_region
    /// Returns NO_PATH if regions are in different islands
    /// This REPLACES flow fields for intra-cluster navigation!
    pub local_routing: [[u8; MAX_REGIONS]; MAX_REGIONS],
    
    /// Inter-cluster portals stored by direction
    /// Used to navigate TO other clusters (not between regions)
    pub inter_cluster_portals: [Option<LineSegment>; 4],  // N, E, S, W
    
    /// Maps island ID to which inter-cluster portal it connects to
    /// neighbor_connectivity[island_id][direction] = Some(portal) | None
    /// direction: 0=North, 1=East, 2=South, 3=West
    pub neighbor_connectivity: [[Option<usize>; 4]; MAX_ISLANDS],
    
    /// Island metadata (connected component info)
    pub islands: [Option<Island>; MAX_ISLANDS],
    pub island_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Island {
    pub id: IslandId,
    /// Which regions belong to this island
    pub regions: SmallVec<[RegionId; MAX_REGIONS]>,
}
```

**Why:** Adds region data alongside existing portal data, allows gradual migration

### 1.3 Testing

**Create test:**
```rust
#[test]
fn test_cluster_struct_sizes() {
    // Verify memory footprint is reasonable
    let cluster_size = std::mem::size_of::<Cluster>();
    println!("Cluster size: {} bytes", cluster_size);
    assert!(cluster_size < 10_000, "Cluster should be under 10KB");
}
```

**Commit checkpoint:** "Add region data structures (Phase 1 complete)"

---

## Phase 2: Implement Convex Decomposition Algorithm

**Goal:** Generate convex regions from cluster tiles

**Estimated Time:** 8-16 hours  
**Risk:** Medium - complex algorithm, needs thorough testing

### 2.1 Create `region_decomposition.rs`

**New file:** `src/game/pathfinding/region_decomposition.rs`

```rust
use crate::game::structures::FlowField;
use crate::game::fixed_math::FixedVec2;
use super::types::{Region, Rect, CLUSTER_SIZE, MAX_REGIONS};

/// Decompose a cluster into convex rectangular regions.
/// 
/// Algorithm: Maximal Rectangles
/// 1. Scan cluster row by row
/// 2. Merge walkable tiles into largest possible horizontal strips
/// 3. Two strips are "connected" if they touch vertically
/// 4. Result: Array of rectangles covering all walkable space
///
/// Alternative: For non-rectangular regions, use triangulation + convex merge
pub fn decompose_cluster_into_regions(
    cluster_id: (usize, usize),
    flow_field: &FlowField,
) -> Vec<Region> {
    let (cx, cy) = cluster_id;
    let min_x = cx * CLUSTER_SIZE;
    let max_x = ((cx + 1) * CLUSTER_SIZE).min(flow_field.width);
    let min_y = cy * CLUSTER_SIZE;
    let max_y = ((cy + 1) * CLUSTER_SIZE).min(flow_field.height);
    
    let mut regions = Vec::new();
    
    // Step 1: Find horizontal strips (maximal rectangles)
    let mut strips = find_horizontal_strips(min_x, max_x, min_y, max_y, flow_field);
    
    // Step 2: Merge vertically adjacent strips if they form valid rectangles
    let rectangles = merge_strips_into_rectangles(strips);
    
    // Step 3: Convert to Region structs
    for (i, rect) in rectangles.iter().enumerate() {
        if i >= MAX_REGIONS {
            warn!("Cluster {:?} has {} regions, truncating to {}", 
                  cluster_id, rectangles.len(), MAX_REGIONS);
            break;
        }
        
        regions.push(Region {
            bounds: *rect,
            vertices: rect_to_vertices(*rect),
            island_id: IslandId(0), // Will be computed in Phase 3
            portals: SmallVec::new(), // Will be computed in Phase 2.3
        });
    }
    
    regions
}

fn find_horizontal_strips(
    min_x: usize, max_x: usize,
    min_y: usize, max_y: usize,
    flow_field: &FlowField,
) -> Vec<(usize, usize, usize)> {
    // Returns: (y, start_x, end_x) for each horizontal strip
    let mut strips = Vec::new();
    
    for y in min_y..max_y {
        let mut strip_start = None;
        
        for x in min_x..max_x {
            let idx = flow_field.get_index(x, y);
            let walkable = flow_field.cost_field[idx] != 255;
            
            if walkable {
                if strip_start.is_none() {
                    strip_start = Some(x);
                }
            } else {
                if let Some(start_x) = strip_start {
                    strips.push((y, start_x, x - 1));
                    strip_start = None;
                }
            }
        }
        
        if let Some(start_x) = strip_start {
            strips.push((y, start_x, max_x - 1));
        }
    }
    
    strips
}

fn merge_strips_into_rectangles(strips: Vec<(usize, usize, usize)>) -> Vec<Rect> {
    // Greedy merging: combine strips with same x-range vertically
    let mut rectangles = Vec::new();
    let mut used = vec![false; strips.len()];
    
    for i in 0..strips.len() {
        if used[i] { continue; }
        
        let (y_start, x_start, x_end) = strips[i];
        let mut y_end = y_start;
        used[i] = true;
        
        // Try to extend downward
        for j in i+1..strips.len() {
            if used[j] { continue; }
            
            let (y_j, x_j_start, x_j_end) = strips[j];
            
            // Check if this strip is directly below and has same x-range
            if y_j == y_end + 1 && x_j_start == x_start && x_j_end == x_end {
                y_end = y_j;
                used[j] = true;
            }
        }
        
        rectangles.push(Rect {
            min: FixedVec2::from_grid(x_start, y_start),
            max: FixedVec2::from_grid(x_end + 1, y_end + 1),
        });
    }
    
    rectangles
}

fn rect_to_vertices(rect: Rect) -> SmallVec<[FixedVec2; 8]> {
    smallvec![
        rect.min,
        FixedVec2::new(rect.max.x, rect.min.y),
        rect.max,
        FixedVec2::new(rect.min.x, rect.max.y),
    ]
}

/// Find which region contains a given point
pub fn get_region_id(cluster: &Cluster, point: FixedVec2) -> Option<RegionId> {
    for i in 0..cluster.region_count {
        if let Some(region) = &cluster.regions[i] {
            // Fast rejection test
            if !region.bounds.contains(point) {
                continue;
            }
            
            // For rectangles, bounds check is sufficient
            // For complex polygons, do point-in-polygon test
            if is_point_in_convex_polygon(point, &region.vertices) {
                return Some(RegionId(i as u8));
            }
        }
    }
    None
}

fn is_point_in_convex_polygon(point: FixedVec2, vertices: &[FixedVec2]) -> bool {
    // For convex polygons: check that point is on the same side of all edges
    if vertices.len() < 3 { return false; }
    
    let mut sign = None;
    
    for i in 0..vertices.len() {
        let v1 = vertices[i];
        let v2 = vertices[(i + 1) % vertices.len()];
        
        let cross = (v2.x - v1.x) * (point.y - v1.y) - (v2.y - v1.y) * (point.x - v1.x);
        
        let current_sign = cross >= FixedNum::ZERO;
        
        if let Some(expected_sign) = sign {
            if current_sign != expected_sign {
                return false; // Point is on different side of an edge
            }
        } else {
            sign = Some(current_sign);
        }
    }
    
    true
}
```

### 2.2 Create `region_connectivity.rs`

**New file:** `src/game/pathfinding/region_connectivity.rs`

```rust
use super::types::{Region, RegionPortal, RegionId, LineSegment, NO_PATH};
use super::cluster::Cluster;

/// Build connectivity graph between regions and compute local routing table
pub fn build_region_connectivity(cluster: &mut Cluster) {
    // Step 1: Find shared edges (portals) between regions
    find_region_portals(cluster);
    
    // Step 2: Run Dijkstra/BFS from each region to build routing table
    build_local_routing_table(cluster);
}

fn find_region_portals(cluster: &mut Cluster) {
    for i in 0..cluster.region_count {
        for j in i+1..cluster.region_count {
            if let (Some(region_a), Some(region_b)) = (&cluster.regions[i], &cluster.regions[j]) {
                if let Some(shared_edge) = find_shared_edge(region_a, region_b) {
                    // Add bidirectional portal
                    if let Some(region_a_mut) = &mut cluster.regions[i] {
                        region_a_mut.portals.push(RegionPortal {
                            edge: shared_edge,
                            next_region: RegionId(j as u8),
                        });
                    }
                    if let Some(region_b_mut) = &mut cluster.regions[j] {
                        region_b_mut.portals.push(RegionPortal {
                            edge: shared_edge,
                            next_region: RegionId(i as u8),
                        });
                    }
                }
            }
        }
    }
}

fn find_shared_edge(a: &Region, b: &Region) -> Option<LineSegment> {
    // Check each edge of polygon A against each edge of polygon B
    // If they overlap significantly, return the overlap as shared edge
    
    for i in 0..a.vertices.len() {
        let a1 = a.vertices[i];
        let a2 = a.vertices[(i + 1) % a.vertices.len()];
        
        for j in 0..b.vertices.len() {
            let b1 = b.vertices[j];
            let b2 = b.vertices[(j + 1) % b.vertices.len()];
            
            // Check if edges are collinear and overlapping
            if let Some(overlap) = compute_segment_overlap(a1, a2, b1, b2) {
                return Some(overlap);
            }
        }
    }
    
    None
}

fn compute_segment_overlap(
    a1: FixedVec2, a2: FixedVec2,
    b1: FixedVec2, b2: FixedVec2,
) -> Option<LineSegment> {
    // TODO: Implement proper segment intersection/overlap detection
    // For now, simplified version for axis-aligned rectangles
    
    // Check if segments are on same line (either horizontal or vertical)
    let a_vertical = (a1.x - a2.x).abs() < FixedNum::EPSILON;
    let b_vertical = (b1.x - b2.x).abs() < FixedNum::EPSILON;
    
    if a_vertical && b_vertical && (a1.x - b1.x).abs() < FixedNum::EPSILON {
        // Both vertical and on same x-coordinate
        let min_y = a1.y.min(a2.y).max(b1.y.min(b2.y));
        let max_y = a1.y.max(a2.y).min(b1.y.max(b2.y));
        
        if max_y > min_y {
            return Some(LineSegment {
                start: FixedVec2::new(a1.x, min_y),
                end: FixedVec2::new(a1.x, max_y),
            });
        }
    }
    
    let a_horizontal = (a1.y - a2.y).abs() < FixedNum::EPSILON;
    let b_horizontal = (b1.y - b2.y).abs() < FixedNum::EPSILON;
    
    if a_horizontal && b_horizontal && (a1.y - b1.y).abs() < FixedNum::EPSILON {
        // Both horizontal and on same y-coordinate
        let min_x = a1.x.min(a2.x).max(b1.x.min(b2.x));
        let max_x = a1.x.max(a2.x).min(b1.x.max(b2.x));
        
        if max_x > min_x {
            return Some(LineSegment {
                start: FixedVec2::new(min_x, a1.y),
                end: FixedVec2::new(max_x, a1.y),
            });
        }
    }
    
    None
}

fn build_local_routing_table(cluster: &mut Cluster) {
    // Initialize to NO_PATH
    for i in 0..MAX_REGIONS {
        for j in 0..MAX_REGIONS {
            cluster.local_routing[i][j] = if i == j { i as u8 } else { NO_PATH };
        }
    }
    
    // BFS from each region to all others
    for start in 0..cluster.region_count {
        let mut visited = [false; MAX_REGIONS];
        let mut queue = std::collections::VecDeque::new();
        let mut first_step = [NO_PATH; MAX_REGIONS];
        
        queue.push_back(start);
        visited[start] = true;
        first_step[start] = start as u8;
        
        while let Some(current) = queue.pop_front() {
            if let Some(region) = &cluster.regions[current] {
                for portal in &region.portals {
                    let next = portal.next_region.0 as usize;
                    
                    if !visited[next] {
                        visited[next] = true;
                        
                        // Record first step from start
                        if current == start {
                            first_step[next] = next as u8;
                        } else {
                            first_step[next] = first_step[current];
                        }
                        
                        queue.push_back(next);
                    }
                }
            }
        }
        
        // Copy to routing table
        for end in 0..cluster.region_count {
            cluster.local_routing[start][end] = first_step[end];
        }
    }
}
```

### 2.3 Integration & Testing

**Update `graph_build.rs`:**
```rust
// Add after cluster initialization
pub fn decompose_and_connect_cluster(
    graph: &mut HierarchicalGraph,
    flow_field: &FlowField,
    cluster_id: (usize, usize),
) {
    use super::region_decomposition::decompose_cluster_into_regions;
    use super::region_connectivity::build_region_connectivity;
    
    // Generate regions
    let regions = decompose_cluster_into_regions(cluster_id, flow_field);
    
    if let Some(cluster) = graph.clusters.get_mut(&cluster_id) {
        // Store regions in fixed-size array
        for (i, region) in regions.into_iter().enumerate() {
            if i >= MAX_REGIONS {
                break;
            }
            cluster.regions[i] = Some(region);
        }
        cluster.region_count = regions.len().min(MAX_REGIONS);
        
        // Build connectivity
        build_region_connectivity(cluster);
    }
}
```

**Test:**
```rust
#[test]
fn test_simple_cluster_decomposition() {
    // Create 25×25 cluster with simple layout:
    // ████████████████
    // ████████████████
    // ░░░░░░░░░░░░░░░░  <- Open space (should be 1 region)
    // ░░░░░░░░░░░░░░░░
    
    let mut flow_field = FlowField::new(25, 25);
    for y in 2..25 {
        for x in 0..25 {
            flow_field.cost_field[flow_field.get_index(x, y)] = 1; // Walkable
        }
    }
    
    let regions = decompose_cluster_into_regions((0, 0), &flow_field);
    
    assert_eq!(regions.len(), 1, "Simple open area should be 1 region");
}

#[test]
fn test_complex_cluster_decomposition() {
    // Create cluster with U-shaped obstacle:
    // ░░░░███░░░░  <- Should create 2 regions (left and right arms)
    // ░░░░███░░░░
    // ░░░░░░░░░░░  <- Plus bottom region = 3 total
    
    // TODO: Implement test
}
```

**Commit checkpoint:** "Implement convex decomposition (Phase 2 complete)"

---

## Phase 3: Island Detection & Tortuosity-Based Splitting

**Goal:** Group regions into islands, split based on path complexity

**Estimated Time:** 4-8 hours  
**Risk:** Medium - algorithm complexity

### 3.1 Create `island_detection.rs`

**New file:** `src/game/pathfinding/island_detection.rs`

```rust
use super::types::{Island, IslandId, TORTUOSITY_THRESHOLD, MAX_ISLANDS, MAX_REGIONS};
use super::cluster::Cluster;
use crate::game::fixed_math::FixedNum;

pub fn identify_islands(cluster: &mut Cluster) {
    let mut assigned = [false; MAX_REGIONS];
    let mut island_count = 0;
    
    for start_region in 0..cluster.region_count {
        if assigned[start_region] { continue; }
        
        if island_count >= MAX_ISLANDS {
            warn!("Cluster {:?} exceeded MAX_ISLANDS ({}), merging remaining regions into last island",
                  cluster.id, MAX_ISLANDS);
            // Assign all remaining to last island
            for i in start_region..cluster.region_count {
                if let Some(region) = &mut cluster.regions[i] {
                    region.island_id = IslandId((island_count - 1) as u8);
                }
            }
            break;
        }
        
        let current_island_id = IslandId(island_count as u8);
        let mut island_regions = SmallVec::new();
        let mut queue = vec![start_region];
        assigned[start_region] = true;
        
        while let Some(current) = queue.pop() {
            island_regions.push(RegionId(current as u8));
            
            // Mark this region as part of current island
            if let Some(region) = &mut cluster.regions[current] {
                region.island_id = current_island_id;
            }
            
            // Check neighbors
            for next in 0..cluster.region_count {
                if assigned[next] { continue; }
                
                // Check if well-connected (low tortuosity)
                if is_well_connected(cluster, current, next) {
                    assigned[next] = true;
                    queue.push(next);
                }
            }
        }
        
        cluster.islands[island_count] = Some(Island {
            id: current_island_id,
            regions: island_regions,
        });
        island_count += 1;
    }
    
    cluster.island_count = island_count;
}

fn is_well_connected(cluster: &Cluster, region_a: usize, region_b: usize) -> bool {
    // Calculate path distance using routing table
    let path_exists = cluster.local_routing[region_a][region_b] != NO_PATH;
    if !path_exists {
        return false; // Physically disconnected
    }
    
    // Calculate euclidean distance between region centers
    let center_a = get_region_center(cluster, region_a);
    let center_b = get_region_center(cluster, region_b);
    let euclidean_dist = (center_a - center_b).length();
    
    if euclidean_dist < FixedNum::EPSILON {
        return true; // Same position, definitely connected
    }
    
    // Estimate path distance (walk the routing table)
    let path_dist = estimate_path_distance(cluster, region_a, region_b);
    
    // Tortuosity check
    let tortuosity = path_dist / euclidean_dist;
    
    tortuosity <= FixedNum::from_num(TORTUOSITY_THRESHOLD)
}

fn get_region_center(cluster: &Cluster, region_id: usize) -> FixedVec2 {
    if let Some(region) = &cluster.regions[region_id] {
        let sum = region.vertices.iter().fold(FixedVec2::ZERO, |acc, v| acc + *v);
        sum / FixedNum::from_num(region.vertices.len() as i32)
    } else {
        FixedVec2::ZERO
    }
}

fn estimate_path_distance(cluster: &Cluster, start: usize, end: usize) -> FixedNum {
    // Walk the routing table, accumulate distances
    let mut current = start;
    let mut total_distance = FixedNum::ZERO;
    let mut visited = [false; MAX_REGIONS];
    
    while current != end {
        if visited[current] {
            // Loop detected, return large distance
            return FixedNum::from_num(10000);
        }
        visited[current] = true;
        
        let next = cluster.local_routing[current][end] as usize;
        if next == NO_PATH as usize {
            // No path exists
            return FixedNum::MAX;
        }
        
        // Add distance from current to next
        let center_current = get_region_center(cluster, current);
        let center_next = get_region_center(cluster, next);
        total_distance += (center_current - center_next).length();
        
        current = next;
    }
    
    total_distance
}
```

### 3.2 Integration

**Update Phase 2's `decompose_and_connect_cluster`:**
```rust
pub fn decompose_and_connect_cluster(
    graph: &mut HierarchicalGraph,
    flow_field: &FlowField,
    cluster_id: (usize, usize),
) {
    use super::region_decomposition::decompose_cluster_into_regions;
    use super::region_connectivity::build_region_connectivity;
    use super::island_detection::identify_islands;  // NEW
    
    let regions = decompose_cluster_into_regions(cluster_id, flow_field);
    
    if let Some(cluster) = graph.clusters.get_mut(&cluster_id) {
        for (i, region) in regions.into_iter().enumerate() {
            if i >= MAX_REGIONS { break; }
            cluster.regions[i] = Some(region);
        }
        cluster.region_count = regions.len().min(MAX_REGIONS);
        
        build_region_connectivity(cluster);
        identify_islands(cluster);  // NEW
    }
}
```

**Commit checkpoint:** "Implement island detection (Phase 3 complete)"

---

## Phase 4: Update Global Routing Table (Island-Aware)

**Goal:** Change routing table to use `(cluster, island)` keys instead of just `cluster`

**Estimated Time:** 6-10 hours  
**Risk:** High - changes core pathfinding logic

### 4.1 Update `graph.rs`

**Change routing table type:**
```rust
pub struct HierarchicalGraph {
    // ... existing fields ...
    
    /// OLD (Phase 4 removal):
    // pub cluster_routing_table: BTreeMap<(usize, usize), BTreeMap<(usize, usize), usize>>,
    
    /// NEW: Island-aware routing table
    /// routing_table[(start_cluster, start_island)][(goal_cluster, goal_island)] = first_portal_id
    pub routing_table: BTreeMap<
        ClusterIslandId,
        BTreeMap<ClusterIslandId, usize>
    >,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClusterIslandId {
    pub cluster: (usize, usize),
    pub island: IslandId,
}
```

### 4.2 Update Routing Table Build

**Modify `build_routing_table_for_source`:**
```rust
pub fn build_routing_table_for_source_island(
    &mut self,
    source: ClusterIslandId,
) {
    // Dijkstra from (cluster, island) node
    let mut distances: BTreeMap<ClusterIslandId, FixedNum> = BTreeMap::new();
    let mut first_portal: BTreeMap<ClusterIslandId, usize> = BTreeMap::new();
    let mut open_set = BinaryHeap::new();
    
    distances.insert(source, FixedNum::ZERO);
    // ... rest of Dijkstra implementation
    
    // When expanding a portal, check which island it leads to in the neighbor cluster
    // Use cluster.neighbor_connectivity[island_id][direction] to determine
}
```

### 4.3 Update Path Component

**Modify `Path` enum in `types.rs`:**
```rust
#[derive(Component, Debug, Clone)]
pub enum Path {
    Direct(FixedVec2),
    LocalAStar { waypoints: Vec<FixedVec2>, current_index: usize },
    Hierarchical {
        goal: FixedVec2,
        goal_cluster: (usize, usize),
        goal_island: IslandId,  // NEW
    }
}
```

### 4.4 Update `process_path_requests`

**Modify `systems.rs`:**
```rust
pub fn process_path_requests(
    mut path_requests: MessageReader<PathRequest>,
    mut commands: Commands,
    map_flow_field: Res<MapFlowField>,
    graph: Res<HierarchicalGraph>,
    components: Res<ConnectedComponents>,  // For reachability check
) {
    for request in path_requests.read() {
        let goal_node_opt = flow_field.world_to_grid(request.goal);

        if let Some(goal_node) = goal_node_opt {
            let goal_cluster = (goal_node.0 / CLUSTER_SIZE, goal_node.1 / CLUSTER_SIZE);
            
            // NEW: Determine goal island
            let goal_island = if let Some(cluster) = graph.clusters.get(&goal_cluster) {
                let goal_pos = request.goal;
                if let Some(region_id) = get_region_id(cluster, goal_pos) {
                    cluster.regions[region_id.0 as usize].as_ref().unwrap().island_id
                } else {
                    // Goal is in obstacle - snap to nearest walkable
                    warn!("Goal {:?} is in obstacle, snapping", goal_pos);
                    // TODO: Implement snap_to_nearest_walkable
                    continue;
                }
            } else {
                continue;
            };
            
            // NEW: Reachability check
            let start_node_opt = flow_field.world_to_grid(request.start);
            if let Some(start_node) = start_node_opt {
                let start_cluster = (start_node.0 / CLUSTER_SIZE, start_node.1 / CLUSTER_SIZE);
                let start_island = /* similar logic to get start island */;
                
                let start_key = ClusterIslandId { cluster: start_cluster, island: start_island };
                let goal_key = ClusterIslandId { cluster: goal_cluster, island: goal_island };
                
                if !components.are_connected(start_key, goal_key) {
                    warn!("Target {:?} is unreachable from {:?}", request.goal, request.start);
                    // Don't set Path component - fail gracefully
                    continue;
                }
            }
            
            commands.entity(request.entity).insert(Path::Hierarchical {
                goal: request.goal,
                goal_cluster,
                goal_island,  // NEW
            });
        }
    }
}
```

**Commit checkpoint:** "Update global routing to be island-aware (Phase 4 complete)"

---

## Phase 5: Implement Region-Based Movement System

**Goal:** Replace flow field movement with region-based movement

**Estimated Time:** 8-12 hours  
**Risk:** High - core gameplay changes

### 5.1 Update Movement System in `simulation/systems.rs`

**Find the `follow_path` or equivalent movement system and replace:**

```rust
pub fn navigate_hierarchical_path(
    mut query: Query<(&mut Transform, &Path, &mut Velocity), With<Unit>>,
    graph: Res<HierarchicalGraph>,
    time: Res<Time>,
) {
    for (transform, path, mut velocity) in query.iter_mut() {
        let Path::Hierarchical { goal, goal_cluster, goal_island } = path else { continue; };
        
        let current_pos = FixedVec2::from_vec2(transform.translation.xy());
        let current_cluster = get_cluster_id_from_pos(current_pos);
        
        // Case 1: In target cluster (The "Last Mile")
        if current_cluster == *goal_cluster {
            navigate_within_cluster(
                current_pos,
                *goal,
                current_cluster,
                &graph,
                &mut velocity
            );
        } else {
            // Case 2: Macro navigation (move toward next cluster)
            navigate_between_clusters(
                current_pos,
                current_cluster,
                *goal_cluster,
                *goal_island,
                &graph,
                &mut velocity
            );
        }
    }
}

fn navigate_within_cluster(
    current_pos: FixedVec2,
    goal_pos: FixedVec2,
    cluster_id: (usize, usize),
    graph: &HierarchicalGraph,
    velocity: &mut Velocity,
) {
    let Some(cluster) = graph.clusters.get(&cluster_id) else { return; };
    
    let Some(current_region_id) = get_region_id(cluster, current_pos) else {
        warn!("Unit at {:?} is not in any region!", current_pos);
        return;
    };
    
    let Some(goal_region_id) = get_region_id(cluster, goal_pos) else {
        warn!("Goal at {:?} is not in any region!", goal_pos);
        return;
    };
    
    // Case 1a: Same region - convexity guarantees straight line is safe
    if current_region_id == goal_region_id {
        let direction = (goal_pos - current_pos).normalize_or_zero();
        velocity.0 = direction * UNIT_SPEED;
        return;
    }
    
    // Case 1b: Different region - use routing table
    let next_region_id = cluster.local_routing[current_region_id.0 as usize][goal_region_id.0 as usize];
    
    if next_region_id == NO_PATH {
        error!("No path from region {:?} to {:?} in cluster {:?}", 
               current_region_id, goal_region_id, cluster_id);
        velocity.0 = FixedVec2::ZERO;
        return;
    }
    
    // Find portal to next region
    let Some(current_region) = &cluster.regions[current_region_id.0 as usize] else { return; };
    
    let portal = current_region.portals.iter()
        .find(|p| p.next_region.0 == next_region_id);
    
    if let Some(portal) = portal {
        // Clamped projection (approximate funnel algorithm)
        let steer_point = project_and_clamp_to_segment(goal_pos, &portal.edge);
        let direction = (steer_point - current_pos).normalize_or_zero();
        velocity.0 = direction * UNIT_SPEED;
    } else {
        error!("Portal not found from region {} to {}", current_region_id.0, next_region_id);
        velocity.0 = FixedVec2::ZERO;
    }
}

fn navigate_between_clusters(
    current_pos: FixedVec2,
    current_cluster: (usize, usize),
    goal_cluster: (usize, usize),
    goal_island: IslandId,
    graph: &HierarchicalGraph,
    velocity: &mut Velocity,
) {
    // Get current island
    let Some(cluster) = graph.clusters.get(&current_cluster) else { return; };
    let Some(current_region_id) = get_region_id(cluster, current_pos) else { return; };
    let Some(current_region) = &cluster.regions[current_region_id.0 as usize] else { return; };
    let current_island = current_region.island_id;
    
    // Lookup next portal from routing table
    let start_key = ClusterIslandId { cluster: current_cluster, island: current_island };
    let goal_key = ClusterIslandId { cluster: goal_cluster, island: goal_island };
    
    let Some(routing_entry) = graph.routing_table.get(&start_key) else {
        error!("No routing table entry for {:?}", start_key);
        velocity.0 = FixedVec2::ZERO;
        return;
    };
    
    let Some(&next_portal_id) = routing_entry.get(&goal_key) else {
        error!("No path from {:?} to {:?}", start_key, goal_key);
        velocity.0 = FixedVec2::ZERO;
        return;
    };
    
    // Navigate to the portal
    let Some(portal) = graph.nodes.get(next_portal_id) else { return; };
    let portal_pos = FixedVec2::new(
        FixedNum::from_num(portal.node.x as i32),
        FixedNum::from_num(portal.node.y as i32)
    );
    
    let direction = (portal_pos - current_pos).normalize_or_zero();
    velocity.0 = direction * UNIT_SPEED;
}

fn project_and_clamp_to_segment(target: FixedVec2, segment: &LineSegment) -> FixedVec2 {
    // Project target onto line segment and clamp to endpoints
    let segment_vec = segment.end - segment.start;
    let segment_len_sq = segment_vec.length_squared();
    
    if segment_len_sq < FixedNum::EPSILON {
        return segment.start; // Degenerate segment
    }
    
    let to_target = target - segment.start;
    let projection = to_target.dot(segment_vec) / segment_len_sq;
    
    // Clamp to [0, 1]
    let t = projection.clamp(FixedNum::ZERO, FixedNum::ONE);
    
    segment.start + segment_vec * t
}
```

### 5.2 Remove Old Flow Field Movement

**Mark as deprecated / remove:**
- Old flow field lookup logic in movement systems
- `LocalFlowField` generation on-demand (keep for backward compat during transition)

**Commit checkpoint:** "Implement region-based movement (Phase 5 complete)"

---

## Phase 5.5 (Optional): Add Portal-to-Portal Flow Fields for Smoothness

**Goal:** Enhance movement quality with flow fields in large open clusters

**Estimated Time:** 6-10 hours  
**Risk:** Low - optional enhancement, doesn't break existing region-based movement

**When to implement:** Only if visual quality testing shows units making sharp turns in large open areas.

### 5.5.1 Add Portal-to-Portal Flow Field Structure

**Update `types.rs`:**
```rust
/// Portal-to-portal flow field for smooth inter-portal movement
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortalFlowFieldCache {
    /// Cached flow fields: [entry_portal_id][exit_portal_id] = flow_field
    /// Only stored for large/open clusters where smoothness matters
    fields: BTreeMap<(usize, usize), LocalFlowField>,
}

impl PortalFlowFieldCache {
    pub fn new() -> Self {
        Self { fields: BTreeMap::new() }
    }
    
    pub fn get(&self, entry: usize, exit: usize) -> Option<&LocalFlowField> {
        self.fields.get(&(entry, exit))
    }
    
    pub fn should_generate_for_cluster(cluster: &Cluster) -> bool {
        // Only generate for large, open clusters
        // Complex clusters use region navigation instead
        cluster.region_count <= 3 && cluster.island_count == 1
    }
}
```

**Update `cluster.rs`:**
```rust
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Cluster {
    pub id: (usize, usize),
    
    // OLD SYSTEM (keep during transition)
    pub portals: Vec<usize>,
    pub flow_field_cache: BTreeMap<usize, super::types::LocalFlowField>,
    
    // NEW REGION SYSTEM
    pub regions: [Option<Region>; MAX_REGIONS],
    pub region_count: usize,
    pub local_routing: [[u8; MAX_REGIONS]; MAX_REGIONS],
    pub neighbor_connectivity: [[Option<usize>; 4]; MAX_ISLANDS],
    pub islands: [Option<Island>; MAX_ISLANDS],
    pub island_count: usize,
    
    // OPTIONAL: Portal-to-portal fields for smooth movement in open areas
    pub portal_flow_fields: Option<PortalFlowFieldCache>,
}
```

### 5.5.2 Generate Portal-to-Portal Flow Fields

**Create `portal_flow.rs`:**
```rust
use super::cluster::Cluster;
use super::types::{LocalFlowField, PortalFlowFieldCache};

/// Generate directional flow fields between portal pairs
/// Only called for large, open clusters
pub fn generate_portal_flow_fields(
    cluster: &Cluster,
    graph: &HierarchicalGraph,
    flow_field: &FlowField,
) -> PortalFlowFieldCache {
    let mut cache = PortalFlowFieldCache::new();
    
    // For each entry portal
    for &entry_portal_id in &cluster.portals {
        // For each exit portal (excluding entry)
        for &exit_portal_id in &cluster.portals {
            if entry_portal_id == exit_portal_id {
                continue; // Same portal, no field needed
            }
            
            // Generate flow field from entry to exit
            let field = generate_directional_flow_field(
                cluster.id,
                entry_portal_id,
                exit_portal_id,
                graph,
                flow_field,
            );
            
            cache.fields.insert((entry_portal_id, exit_portal_id), field);
        }
    }
    
    cache
}

fn generate_directional_flow_field(
    cluster_id: (usize, usize),
    entry_portal_id: usize,
    exit_portal_id: usize,
    graph: &HierarchicalGraph,
    map_flow_field: &FlowField,
) -> LocalFlowField {
    // Similar to generate_local_flow_field, but with clearance weighting
    let (cx, cy) = cluster_id;
    let min_x = cx * CLUSTER_SIZE;
    let max_x = ((cx + 1) * CLUSTER_SIZE).min(map_flow_field.width);
    let min_y = cy * CLUSTER_SIZE;
    let max_y = ((cy + 1) * CLUSTER_SIZE).min(map_flow_field.height);
    
    let width = max_x - min_x;
    let height = max_y - min_y;
    
    // Precompute clearance map (distance to nearest obstacle)
    let clearance_map = compute_clearance_map(cluster_id, map_flow_field);
    
    // Generate integration field with wall repulsion
    let integration_field = dijkstra_with_clearance(
        cluster_id,
        exit_portal_id,
        &clearance_map,
        map_flow_field,
        graph,
    );
    
    // Convert to flow vectors
    generate_flow_vectors_from_integration(integration_field, width, height)
}

fn compute_clearance_map(cluster_id: (usize, usize), flow_field: &FlowField) -> Vec<f32> {
    // For each tile, compute distance to nearest obstacle
    // This creates a "spine" effect - paths prefer open space
    
    // TODO: Implement using distance transform or brushfire algorithm
    vec![4.0; CLUSTER_SIZE * CLUSTER_SIZE]  // Placeholder
}

fn dijkstra_with_clearance(
    cluster_id: (usize, usize),
    goal_portal_id: usize,
    clearance_map: &[f32],
    flow_field: &FlowField,
    graph: &HierarchicalGraph,
) -> Vec<u32> {
    // Similar to standard Dijkstra, but edge costs include clearance penalty
    
    let mut integration_field = vec![u32::MAX; CLUSTER_SIZE * CLUSTER_SIZE];
    // ... Dijkstra implementation
    
    // When evaluating neighbor cost:
    // let clearance = clearance_map[neighbor_idx];
    // let wall_penalty = if clearance < 2.0 { 5.0 } else if clearance < 4.0 { 2.0 } else { 0.0 };
    // let total_cost = base_cost + wall_penalty;
    
    integration_field
}
```

### 5.5.3 Use Portal Flow Fields When Available

**Update movement system in Phase 5:**
```rust
fn navigate_within_cluster(
    current_pos: FixedVec2,
    goal_pos: FixedVec2,
    cluster_id: (usize, usize),
    graph: &HierarchicalGraph,
    velocity: &mut Velocity,
) {
    let Some(cluster) = graph.clusters.get(&cluster_id) else { return; };
    
    // NEW: Try portal-to-portal flow fields first (if available)
    if let Some(portal_fields) = &cluster.portal_flow_fields {
        if let (Some(entry), Some(exit)) = (unit.entry_portal, unit.exit_portal) {
            if let Some(field) = portal_fields.get(entry, exit) {
                let vector = field.sample(current_pos);
                velocity.0 = vector.normalize() * UNIT_SPEED;
                return;
            }
        }
    }
    
    // FALLBACK: Region-based navigation (always works)
    let Some(current_region_id) = get_region_id(cluster, current_pos) else {
        warn!("Unit at {:?} is not in any region!", current_pos);
        return;
    };
    
    // ... rest of region-based movement from Phase 5
}
```

### 5.5.4 Add Anticipatory Blending

**New file: `movement_smoothing.rs`:**
```rust
use super::types::*;

const BLEND_DIST: FixedNum = FixedNum::from_num(3.0);
const LOOKAHEAD: FixedNum = FixedNum::from_num(2.0);

pub fn get_blended_steering(
    unit: &Unit,
    current_vector: FixedVec2,
    clusters: &ClusterGrid,
) -> FixedVec2 {
    // Check distance to cluster boundary
    let dist_to_boundary = distance_to_cluster_edge(unit.pos, unit.current_cluster);
    
    if dist_to_boundary >= BLEND_DIST {
        return current_vector; // Far from edge, no blending needed
    }
    
    // Project future position
    let future_pos = unit.pos + current_vector.normalize() * LOOKAHEAD;
    let next_cluster = get_cluster_id(future_pos);
    
    if next_cluster == unit.current_cluster {
        return current_vector; // Still in same cluster
    }
    
    // Sample movement vector from next cluster
    let next_vector = get_movement_vector_at_position(future_pos, next_cluster, clusters);
    
    // Blend based on distance to boundary
    let blend_factor = FixedNum::ONE - (dist_to_boundary / BLEND_DIST);
    current_vector.lerp(next_vector, blend_factor)
}

fn distance_to_cluster_edge(pos: FixedVec2, cluster_id: (usize, usize)) -> FixedNum {
    let (cx, cy) = cluster_id;
    let cluster_min_x = FixedNum::from_num((cx * CLUSTER_SIZE) as i32);
    let cluster_max_x = FixedNum::from_num(((cx + 1) * CLUSTER_SIZE) as i32);
    let cluster_min_y = FixedNum::from_num((cy * CLUSTER_SIZE) as i32);
    let cluster_max_y = FixedNum::from_num(((cy + 1) * CLUSTER_SIZE) as i32);
    
    let dist_to_left = pos.x - cluster_min_x;
    let dist_to_right = cluster_max_x - pos.x;
    let dist_to_top = pos.y - cluster_min_y;
    let dist_to_bottom = cluster_max_y - pos.y;
    
    dist_to_left.min(dist_to_right).min(dist_to_top).min(dist_to_bottom)
}
```

**Integrate into movement system:**
```rust
pub fn navigate_hierarchical_path(
    mut query: Query<(&mut Transform, &Path, &mut Velocity, &mut Unit)>,
    graph: Res<HierarchicalGraph>,
) {
    for (transform, path, mut velocity, mut unit) in query.iter_mut() {
        // Get base movement vector (from Phase 5)
        let base_vector = get_movement_vector(&unit, &path, &graph);
        
        // Apply anticipatory blending for smooth cluster transitions
        let smoothed_vector = get_blended_steering(&unit, base_vector, &graph.clusters);
        
        velocity.0 = smoothed_vector;
    }
}
```

### 5.5.5 Testing & Tuning

**Create visual debug:**
```rust
fn debug_draw_flow_fields(
    gizmos: &mut Gizmos,
    cluster: &Cluster,
    entry_portal: usize,
    exit_portal: usize,
) {
    if let Some(fields) = &cluster.portal_flow_fields {
        if let Some(field) = fields.get(entry_portal, exit_portal) {
            // Draw vector field
            for y in 0..field.height {
                for x in 0..field.width {
                    let idx = y * field.width + x;
                    let pos = tile_to_world(x, y);
                    let vec = field.vectors[idx];
                    
                    gizmos.line(pos, pos + vec * 2.0, Color::CYAN);
                }
            }
        }
    }
}
```

**Performance test:**
```rust
#[test]
fn test_portal_flow_field_memory() {
    // Verify memory is acceptable
    // 4 portals × 3 exits = 12 permutations
    // 25×25 × 16 bytes = 10KB per field
    // 12 × 10KB = 120KB per cluster (only for open clusters)
    // Should be < 10% of clusters on typical map
}
```

**Commit checkpoint:** "Add optional portal-to-portal flow fields (Phase 5.5 complete)"

---

## Phase 6: Building Placement Constraints

**Goal:** Prevent problematic building placements

**Estimated Time:** 4-6 hours  
**Risk:** Low - validation layer

### 6.1 Create `placement_validation.rs`

**New file:** `src/game/pathfinding/placement_validation.rs`

```rust
use super::cluster::Cluster;
use super::types::{MAX_REGIONS, MAX_ISLANDS};

pub struct PathfindingConstraints {
    pub max_islands_per_cluster: usize,
    pub min_island_size: usize,
    pub max_regions_per_cluster: usize,
    pub enforce_critical_paths: bool,
}

impl Default for PathfindingConstraints {
    fn default() -> Self {
        Self {
            max_islands_per_cluster: 3,
            min_island_size: 256, // tiles
            max_regions_per_cluster: MAX_REGIONS,
            enforce_critical_paths: true,
        }
    }
}

#[derive(Debug)]
pub enum PlacementError {
    TooManyIslands { current: usize, would_create: usize, max: usize },
    IslandTooSmall { size: usize, min: usize },
    TooManyRegions { would_create: usize, max: usize },
    WouldDisconnectMap { from: (usize, usize), to: (usize, usize) },
}

pub fn validate_building_placement(
    affected_clusters: &[(usize, usize)],
    building_footprint: &[FixedVec2],
    graph: &HierarchicalGraph,
    flow_field: &FlowField,
    constraints: &PathfindingConstraints,
) -> Result<(), PlacementError> {
    for &cluster_id in affected_clusters {
        // Simulate placement
        let simulated_cluster = simulate_placement(cluster_id, building_footprint, graph, flow_field)?;
        
        // Validate constraints
        validate_cluster(&simulated_cluster, constraints)?;
    }
    
    Ok(())
}

fn simulate_placement(
    cluster_id: (usize, usize),
    building_footprint: &[FixedVec2],
    graph: &HierarchicalGraph,
    flow_field: &FlowField,
) -> Result<Cluster, PlacementError> {
    // Create temporary flow field with building placed
    let mut temp_flow_field = flow_field.clone();
    
    // Mark building footprint as obstacle
    for &pos in building_footprint {
        if let Some((x, y)) = temp_flow_field.world_to_grid(pos) {
            let idx = temp_flow_field.get_index(x, y);
            temp_flow_field.cost_field[idx] = 255; // Obstacle
        }
    }
    
    // Re-decompose cluster
    let regions = decompose_cluster_into_regions(cluster_id, &temp_flow_field);
    
    let mut simulated_cluster = Cluster::default();
    simulated_cluster.id = cluster_id;
    
    for (i, region) in regions.into_iter().enumerate() {
        if i >= MAX_REGIONS { break; }
        simulated_cluster.regions[i] = Some(region);
    }
    simulated_cluster.region_count = regions.len().min(MAX_REGIONS);
    
    build_region_connectivity(&mut simulated_cluster);
    identify_islands(&mut simulated_cluster);
    
    Ok(simulated_cluster)
}

fn validate_cluster(
    cluster: &Cluster,
    constraints: &PathfindingConstraints,
) -> Result<(), PlacementError> {
    // Check island count
    if cluster.island_count > constraints.max_islands_per_cluster {
        return Err(PlacementError::TooManyIslands {
            current: cluster.island_count,
            would_create: cluster.island_count,
            max: constraints.max_islands_per_cluster,
        });
    }
    
    // Check region count
    if cluster.region_count > constraints.max_regions_per_cluster {
        return Err(PlacementError::TooManyRegions {
            would_create: cluster.region_count,
            max: constraints.max_regions_per_cluster,
        });
    }
    
    // Check island sizes
    for i in 0..cluster.island_count {
        if let Some(island) = &cluster.islands[i] {
            let island_size: usize = island.regions.iter()
                .filter_map(|&region_id| cluster.regions[region_id.0 as usize].as_ref())
                .map(|region| calculate_region_area(region))
                .sum();
            
            if island_size < constraints.min_island_size && island_size > 0 {
                return Err(PlacementError::IslandTooSmall {
                    size: island_size,
                    min: constraints.min_island_size,
                });
            }
        }
    }
    
    Ok(())
}

fn calculate_region_area(region: &Region) -> usize {
    // For rectangles: (max.x - min.x) * (max.y - min.y)
    let width = region.bounds.max.x - region.bounds.min.x;
    let height = region.bounds.max.y - region.bounds.min.y;
    (width * height).to_num::<usize>()
}
```

### 6.2 Integration with Editor

**Update `editor/actions.rs` or wherever buildings are placed:**
```rust
use crate::game::pathfinding::placement_validation::{
    validate_building_placement, PathfindingConstraints, PlacementError
};

fn place_building_system(
    /* ... existing parameters ... */
    constraints: Res<PathfindingConstraints>,
) {
    // Before placing building:
    match validate_building_placement(
        &affected_clusters,
        &building.footprint,
        &graph,
        &flow_field,
        &constraints,
    ) {
        Ok(()) => {
            // Place building
            apply_building_to_map(building);
            
            // Re-bake affected clusters
            for cluster_id in affected_clusters {
                decompose_and_connect_cluster(&mut graph, &flow_field, cluster_id);
            }
        }
        Err(PlacementError::TooManyIslands { .. }) => {
            warn!("Cannot place building: would create too many disconnected regions");
            // Show red overlay in UI
        }
        Err(e) => {
            warn!("Cannot place building: {:?}", e);
        }
    }
}
```

**Commit checkpoint:** "Add building placement constraints (Phase 6 complete)"

---

## Phase 7: Cleanup & Deprecation

**Goal:** Remove old flow field system, finalize migration

**Estimated Time:** 4-6 hours  
**Risk:** Low

### 7.1 Remove Old Code

**Delete or deprecate:**
- `cluster_flow.rs` - flow field generation (no longer needed)
- `Cluster::flow_field_cache` - remove field
- Old routing table (`cluster_routing_table` without islands)

**Update:**
```rust
// types.rs - mark as deprecated
#[deprecated(note = "Flow fields replaced by region-based navigation")]
pub struct LocalFlowField {
    // ... keep for backward compatibility during transition
}
```

### 7.2 Update Documentation

**Update module docs:**
```rust
//! # Pathfinding Module
//!
//! Implements hierarchical pathfinding with convex region decomposition
//! for 10M+ unit RTS gameplay.
//!
//! ## Architecture
//!
//! 1. **Macro Grid:** Map divided into 25×25 clusters (spatial hash)
//! 2. **Micro Regions:** Each cluster decomposed into convex polygons
//! 3. **Islands:** Regions grouped by connectivity/tortuosity
//! 4. **Routing:** Precomputed (cluster, island) → (cluster, island) paths
//!
//! ## Movement
//!
//! - Same region: Direct movement (convexity guarantee)
//! - Different region: Clamped projection to portal
//! - Different cluster: Routing table lookup + portal navigation
//!
//! See [PATHFINDING.md](documents/Design%20docs/PATHFINDING.md) for details.
```

### 7.3 Performance Testing

**Create benchmark:**
```rust
#[test]
fn benchmark_region_based_movement() {
    // Create 10k units across map
    // Measure movement update time
    // Target: <200ns per unit
}

#[test]
fn benchmark_path_requests() {
    // Issue 100k path requests
    // Measure time
    // Target: <1ms total
}

#[test]
fn test_memory_footprint() {
    // Build graph for 2048×2048 map
    // Measure total memory
    // Target: <50MB (vs old ~500MB)
}
```

**Commit checkpoint:** "Complete migration, remove old flow field system (Phase 7 complete)"

---

## Rollback Plan

If issues arise, we can roll back by phase:

**Phase 1-3:** Additive only, no rollback needed (just don't use new code)

**Phase 4:** Revert routing table changes, keep old cluster-only routing

**Phase 5:** Revert movement system, use old flow field movement

**Phases 6-7:** Revert building constraints and cleanup

---

## Success Metrics

**After Migration:**
- [ ] Memory: <50MB for pathfinding (down from ~500MB)
- [ ] Path requests: <1ms for 100k units
- [ ] Movement update: <200ns per unit
- [ ] No stuck units in same-cluster scenarios
- [ ] Units don't enter wrong side of U-shaped buildings
- [ ] Building placement validates in <1ms

---

## Migration Checklist

### Phase 1: Data Structures ✅
- [ ] Add `Region`, `Island`, `RegionPortal` types
- [ ] Update `Cluster` struct with region arrays
- [ ] Test struct sizes

### Phase 2: Convex Decomposition ✅
- [ ] Implement `region_decomposition.rs`
- [ ] Implement `region_connectivity.rs`
- [ ] Write tests for simple/complex layouts
- [ ] Integrate into graph build

### Phase 3: Island Detection ✅
- [ ] Implement `island_detection.rs`
- [ ] Test tortuosity-based splitting
- [ ] Verify U-shaped obstacles create 2 islands

### Phase 4: Island-Aware Routing ⚠️
- [ ] Update `HierarchicalGraph.routing_table` type
- [ ] Modify routing table build for islands
- [ ] Update `Path::Hierarchical` with island field
- [ ] Update `process_path_requests` with reachability check

### Phase 5: Region-Based Movement ⚠️
- [ ] Implement `navigate_within_cluster`
- [ ] Implement `navigate_between_clusters`
- [ ] Implement clamped projection
- [ ] Remove old flow field movement
- [ ] Test with 10k+ units

### Phase 6: Building Constraints ✅
- [ ] Implement `placement_validation.rs`
- [ ] Integrate with editor/building system
- [ ] Add UI feedback for invalid placements
- [ ] Test edge cases

### Phase 7: Cleanup ✅
- [ ] Remove `cluster_flow.rs`
- [ ] Remove `flow_field_cache` from Cluster
- [ ] Update documentation
- [ ] Run performance benchmarks
- [ ] Verify memory targets met

---

## Notes

**Key Files to Modify:**
- `types.rs` - Add region types
- `cluster.rs` - Add region fields
- `graph.rs` - Update routing table
- `systems.rs` - Update path requests
- `simulation/systems.rs` - Update movement
- `editor/actions.rs` - Add placement validation

**Parallel Work:**
- Phase 1-3 can be done in parallel (additive)
- Phase 4-5 must be sequential (core logic changes)
- Phase 6 can be done anytime after Phase 3

**Testing Strategy:**
- Unit tests for each algorithm
- Integration tests for full pathfinding
- Performance benchmarks throughout
- Keep old system running in parallel until Phase 5 complete

---

## Summary: Regions vs Flow Fields

Based on the Gemini discussion, here's the final clarification on when to use each approach:

### Within-Cluster Movement (The "Last Mile")

**Option 1: Convex Regions + Clamped Projection (Recommended)**
```rust
// Memory: ~3KB per cluster
// When unit in different region than target:
let portal = find_portal_to_next_region();
let crossing_point = project_target_onto_portal(ultimate_target, portal.edge);
move_toward(crossing_point);
```

**Pros:** Solves arbitrary target positions, handles dynamic obstacles, minimal memory  
**Cons:** Requires tuning for smooth cornering

**Option 2: Portal-to-Portal Flow Fields**
```rust
// Memory: ~15KB per cluster (for open clusters)
// When moving between regions:
let vector = flow_field[entry_region][exit_region].sample(position);
move_in_direction(vector);
```

**Pros:** Smoother, more organic movement automatically  
**Cons:** More memory, doesn't handle arbitrary targets well, expensive to regenerate

**Recommendation:** Start with Option 1. Add Option 2 selectively for large open clusters if visual quality demands it.

### Inter-Cluster Movement (Portal Navigation)

**Current System: Generic "To Portal" Flow Fields**
- Units just move toward nearest point on portal
- Causes wall-scraping and sharp turns

**Enhanced Option 1: Clamped Projection with Look-Through**
```rust
// Project ultimate target onto portal, not just next region
// Units "cut corners" intelligently without extra memory
let ideal_crossing = project_line_through_portal(unit.pos, ultimate_target, portal);
```

**Enhanced Option 2: Portal-to-Portal Flow Fields (Directional)**
```rust
// Bake "from South portal to North portal" (not just "to North")
// Creates smooth streamlines
let vector = cluster_flow_fields[entry_portal_id][exit_portal_id].sample(pos);
```

**Memory Cost:** 4 entries × 3 exits = 12 permutations per cluster  
**Size:** 12 × 1.25KB = 15KB per cluster (vs. 3KB for regions)

**Recommendation:** 
- Use clamped projection with look-through (no extra memory)
- Only add directional flow fields if profiling shows movement quality issues

### Smoothing Techniques (Apply to Both)

**Anticipatory Blending (Always Recommended):**
```rust
// At cluster boundaries, blend current and next cluster vectors
if distance_to_boundary < 3.0 {
    blend(current_vector, peek_next_cluster_vector);
}
```
Cost: ~5 operations, eliminates visible "kinks"

**Wall Repulsion (Recommended):**
```rust
// During baking/decomposition, bias away from obstacles
// Creates natural "spine following" through corridors
cost = base_cost + wall_penalty(clearance);
```
Cost: One-time during baking, major visual quality improvement

### Final Recommendations by Scenario

**For 10M+ Entities (RECOMMENDED):**
Pure regions with **minimal smoothing** is the only viable option at massive scale.

| Scenario | Within-Cluster | Inter-Cluster | Smoothing | Per-Entity Cost |
|----------|----------------|---------------|-----------|-----------------|
| **10M+ Entities** | Regions (direct) | Portal center | None | ~50ns |
| **1M Entities** | Regions | Portal center | Wall Repulsion (bake-time) | ~100ns |
| **100K Entities** | Regions | Clamped Proj | Blending | ~200ns |
| **Visual Showcase** | Portal Flow + Regions | Directional Flow | All techniques | ~500ns |

**Why Simple Wins at Scale:**
- **Blending:** Requires cluster boundary check + next cluster lookup + lerp = 15+ operations
  - 10M entities × 15 ops = 150M operations per frame
- **Clamped Projection:** Vector projection + clamping = 10+ operations
  - 10M entities × 10 ops = 100M operations per frame
- **Flow Field Sampling:** Bilinear interpolation = 8+ operations + memory access
  - 10M entities × 8 ops = 80M operations per frame
- **Direct Movement:** 2 vector operations (subtract + normalize)
  - 10M entities × 2 ops = 20M operations per frame

**Performance Scaling Example:**
```
100K entities with all smoothing: 5ms movement update
1M entities with all smoothing: 50ms movement update (unplayable)
10M entities with all smoothing: 500ms movement update (slideshow)

10M entities with direct movement: 20ms movement update (playable!)
```

### Recommended Implementation for Peregrine (10M+ Units)

**Phase 5 Movement System (Complete API):**
```rust
/// Core movement system - called every frame for all units
fn navigate_hierarchical_path(
    mut query: Query<(&mut Velocity, &Transform, &Path)>,
    graph: Res<HierarchicalGraph>,
) {
    for (mut velocity, transform, path) in query.iter_mut() {
        let pos = FixedVec2::from(transform.translation);
        let current_cluster = get_cluster_id(pos);
        let cluster = &graph.clusters[&current_cluster];
        
        // Get current region (O(10) point-in-polygon checks)
        let Some(current_region) = get_region_id(cluster, pos) else {
            // Unit is in obstacle (shouldn't happen) - stand still
            velocity.0 = FixedVec2::ZERO;
            continue;
        };
        
        // Case 1: Same cluster as goal
        if current_cluster == path.goal_cluster {
            navigate_within_cluster(
                pos, current_region, path.goal, cluster, &mut velocity
            );
        } else {
            // Case 2: Different cluster - navigate to exit portal
            navigate_to_exit_portal(
                pos, current_region, current_cluster, path, 
                &graph, cluster, &mut velocity
            );
        }
    }
}

/// Navigate within the same cluster (region-to-region)
fn navigate_within_cluster(
    pos: FixedVec2,
    current_region: RegionId,
    goal: FixedVec2,
    cluster: &Cluster,
    velocity: &mut Velocity,
) {
    // Get target region
    let Some(goal_region) = get_region_id(cluster, goal) else {
        // Goal in obstacle - shouldn't happen (validated at path request)
        velocity.0 = FixedVec2::ZERO;
        return;
    };
    
    // Same region? Direct movement (convexity guarantees straight line is safe)
    if current_region == goal_region {
        let direction = (goal - pos).normalize();
        velocity.0 = direction * UNIT_SPEED;
        return;
    }
    
    // Different region - navigate to next region via local routing
    let next_region = cluster.local_routing
        [current_region.0 as usize]
        [goal_region.0 as usize];
    
    if next_region == NO_PATH {
        // Regions not connected (different islands) - shouldn't happen
        velocity.0 = FixedVec2::ZERO;
        return;
    }
    
    // Find portal to next region
    let portal = cluster.regions[current_region.0 as usize]
        .as_ref()
        .unwrap()
        .portals
        .iter()
        .find(|p| p.next_region.0 == next_region)
        .expect("Routing table points to non-existent portal");
    
    // Move to portal center (simplest, fastest)
    let portal_center = (portal.edge.start + portal.edge.end) / FixedNum::from_num(2.0);
    let direction = (portal_center - pos).normalize();
    velocity.0 = direction * UNIT_SPEED;
}

/// Navigate to cluster exit portal
fn navigate_to_exit_portal(
    pos: FixedVec2,
    current_region: RegionId,
    current_cluster: (usize, usize),
    path: &Path,
    graph: &HierarchicalGraph,
    cluster: &Cluster,
    velocity: &mut Velocity,
) {
    // Look up which inter-cluster portal to use
    let exit_portal_id = graph.routing_table
        [(current_cluster, path.current_island)]
        [(path.goal_cluster, path.goal_island)];
    
    let inter_cluster_portal = &graph.inter_cluster_portals[exit_portal_id];
    
    // The portal is on the edge between two clusters
    // Find which region in our cluster contains this portal
    let portal_region = find_region_containing_point(
        cluster, 
        (inter_cluster_portal.start + inter_cluster_portal.end) / FixedNum::from_num(2.0)
    ).expect("Portal not in any region");
    
    // Now navigate region-to-region to reach the portal's region
    if current_region == portal_region {
        // We're in the region containing the exit portal - move to portal center
        let portal_center = (inter_cluster_portal.start + inter_cluster_portal.end) 
            / FixedNum::from_num(2.0);
        let direction = (portal_center - pos).normalize();
        velocity.0 = direction * UNIT_SPEED;
    } else {
        // Navigate to the region containing the portal
        let next_region = cluster.local_routing
            [current_region.0 as usize]
            [portal_region.0 as usize];
        
        let portal = cluster.regions[current_region.0 as usize]
            .as_ref()
            .unwrap()
            .portals
            .iter()
            .find(|p| p.next_region.0 == next_region)
            .expect("Routing table error");
        
        let portal_center = (portal.edge.start + portal.edge.end) / FixedNum::from_num(2.0);
        let direction = (portal_center - pos).normalize();
        velocity.0 = direction * UNIT_SPEED;
    }
}

/// Find which region contains a point (O(num_regions) point-in-polygon tests)
fn get_region_id(cluster: &Cluster, point: FixedVec2) -> Option<RegionId> {
    for i in 0..cluster.region_count {
        if let Some(region) = &cluster.regions[i] {
            // Fast rejection test
            if !region.bounding_box.contains(point) {
                continue;
            }
            // Precise convex polygon test
            if is_point_in_convex_polygon(point, &region.vertices) {
                return Some(RegionId(i as u8));
            }
        }
    }
    None
}

fn find_region_containing_point(cluster: &Cluster, point: FixedVec2) -> Option<RegionId> {
    get_region_id(cluster, point)
}
```

**Why This is Fast:**
- No region lookups (only needed for path requests, not movement)
- No projection math
- No boundary checks
- No blending
- Just 2 table lookups + 2 vector operations per entity

**Visual Quality:**
- Units will make sharp turns at portals → **This is acceptable**
- Large unit counts create emergent smooth flow from local interactions
- Boid steering (separate system) handles most visual smoothing
- Players care more about 60 FPS with 10M units than perfect movement curves

### Migration Path

1. **Phase 1-4:** Core infrastructure (same for all approaches)
2. **Phase 5:** Region-based movement with **direct navigation only**
3. **Phase 5.5:** Skip entirely (flow fields too expensive for 10M entities)
4. **Optional Enhancement:** Add wall repulsion at **bake time** (no runtime cost)

**After Performance Testing:**
- If you have spare CPU budget with <1M entities, consider adding blending
- Never add per-entity smoothing for >5M entity scenarios

### Wait, What About Flow Fields?

**Q: Don't we need flow fields to navigate within clusters?**

**A: No! That's what regions replace.**

**Old System:**
```rust
// Unit needs to reach North portal
let vector = flow_field_to_north_portal.sample(unit.pos);
move_in_direction(vector);
```
- Memory: 25×25 vectors × 4 portals = ~5KB per cluster
- Problem: Doesn't work for arbitrary targets in same cluster

**New System:**
```rust
// Unit needs to reach North portal (which is actually between regions)
let current_region = get_region_id(cluster, unit.pos);
let portal_region = cluster.portal_locations[PortalDirection::North].region_id;
let next_region = cluster.local_routing[current_region][portal_region];
let portal_edge = cluster.regions[current_region].portals
    .find(|p| p.next_region == next_region);
move_toward(portal_edge.center());
```
- Memory: Just the routing table (32×32 bytes) + region data
- Works: For any target, just navigate region-to-region

**The key insight:**
- Flow fields were a *workaround* for not having proper intra-cluster pathfinding
- Regions give us proper intra-cluster pathfinding via the connectivity graph
- Each step is just "move to next region's portal" - no vector field needed
4. **Tuning:** Add smoothing techniques as needed (Balanced)

**You can always upgrade incrementally based on real gameplay testing.**
