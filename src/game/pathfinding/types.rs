use bevy::prelude::*;
use crate::game::fixed_math::{FixedVec2, FixedNum};
use serde::{Serialize, Deserialize};
use smallvec::SmallVec;

/// Fixed cluster size for hierarchical pathfinding (25×25 cells).
///
/// Maps are divided into clusters of this size. Larger clusters reduce graph size
/// but increase intra-cluster pathfinding cost. 25×25 provides good balance.
pub const CLUSTER_SIZE: usize = 25;

/// Maximum number of regions per cluster.
/// Typical clusters: 1-10 regions (open terrain vs. complex rooms)
/// Complex clusters: up to 32 regions (mazes, tight corridors)
pub const MAX_REGIONS: usize = 32;

/// Maximum number of islands (connected components) per cluster.
/// Most clusters: 1 island (fully connected)
/// Split clusters: 2-3 islands (river, wall, U-shaped building)
/// Complex obstacle layouts: up to 16 islands
/// Increased from 4 to handle complex random obstacle scenarios
pub const MAX_ISLANDS: usize = 16;

/// Tortuosity threshold for splitting islands.
/// If path_distance / euclidean_distance > this value, regions are separate islands.
pub const TORTUOSITY_THRESHOLD: f32 = 3.0;

/// Value indicating no path exists between two regions (different islands).
pub const NO_PATH: u8 = 255;

/// Directions for portal/neighbor connectivity (cardinal + diagonal).
/// 
/// This enum ensures type-safe direction indexing and prevents documentation/implementation mismatches.
/// The repr(u8) ensures zero-cost conversion to array indices.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    North = 0,
    South = 1,
    East = 2,
    West = 3,
    NorthEast = 4,
    NorthWest = 5,
    SouthEast = 6,
    SouthWest = 7,
}

impl Direction {
    /// Convert to array index for neighbor_connectivity lookups
    #[inline]
    pub fn as_index(self) -> usize {
        self as usize
    }
    
    /// All eight directions (cardinal + diagonal)
    pub const ALL: [Direction; 8] = [
        Direction::North,
        Direction::South,
        Direction::East,
        Direction::West,
        Direction::NorthEast,
        Direction::NorthWest,
        Direction::SouthEast,
        Direction::SouthWest,
    ];
}

#[derive(Event, Message, Debug, Clone)]
pub struct PathRequest {
    pub entity: Entity,
    pub goal: FixedVec2,
}

/// Cached navigation cell for the goal position
/// Precomputed during path request processing to avoid repeated lookups
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct GoalNavCell(pub crate::game::pathfinding::navigation_lookup::NavigationCell);

/// Path component with state machine to avoid expensive component removal in hot loop
#[derive(Component, Debug, Clone)]
pub enum Path {
    /// No active path - unit is idle
    Inactive,
    /// Active path - unit is navigating
    Active(PathState),
    /// Path completed - unit arrived at destination (marked for cleanup)
    Completed,
    /// Path blocked - no route exists (marked for cleanup)
    Blocked,
}

/// Active path variants
#[derive(Debug, Clone)]
pub enum PathState {
    Direct(FixedVec2),
    LocalAStar { waypoints: Vec<FixedVec2>, current_index: usize },
    Hierarchical {
        goal: FixedVec2,
        goal_cluster: ClusterId,
        goal_region: Option<RegionId>,  // Cached goal region (None if not in any region)
        goal_island: IslandId,
        
        // PERF: Cached navigation state to avoid recomputing every frame
        /// Current cluster the unit is in (revalidated each frame cheaply)
        current_cluster: Option<ClusterId>,
        /// Current region the unit is in (revalidated each frame cheaply)
        current_region: Option<RegionId>,
        /// Next expected cluster after crossing boundary (for fast transition)
        next_expected_cluster: Option<ClusterId>,
        /// Next expected region after crossing boundary (for fast transition)
        next_expected_region: Option<RegionId>,
        /// Current target position to navigate toward
        current_target: Option<FixedVec2>,
        /// Whether current target is an inter-cluster portal
        is_inter_cluster_target: bool,
    }
}

impl Default for Path {
    fn default() -> Self {
        Path::Active(PathState::Direct(FixedVec2::ZERO))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize, PartialOrd, Ord)]
pub struct Node {
    pub x: usize,
    pub y: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Portal {
    pub id: usize,
    pub node: Node,
    pub range_min: Node,
    pub range_max: Node,
    pub cluster: (usize, usize),
    /// Cached world position (precomputed to avoid grid_to_world in hot path)
    pub world_pos: FixedVec2,
}

// ============================================================================
// NEW: Region-Based Pathfinding Types
// ============================================================================

/// A convex polygon/rectangle representing a navigable region within a cluster.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Region {
    /// Unique identifier within the cluster
    pub id: RegionId,
    /// Bounding rectangle (for point-in-region fast rejection)
    pub bounds: Rect,
    /// Vertices of the convex polygon (typically 4 for rectangles)
    pub vertices: SmallVec<[FixedVec2; 8]>,
    /// Which island this region belongs to
    pub island: IslandId,
    /// Connections to other regions (shared edges/portals)
    pub portals: SmallVec<[RegionPortal; 8]>,
    /// Whether this region is non-convex or complex (requires special pathfinding)
    /// Non-convex regions cannot guarantee straight-line movement is obstacle-free
    pub is_dangerous: bool,
}

/// A portal connecting two regions within a cluster
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegionPortal {
    /// The shared edge between this region and the next
    pub edge: LineSegment,
    /// Midpoint of the edge (for navigation)
    pub center: FixedVec2,
    /// ID of the connected region
    pub next_region: RegionId,
}

/// Cluster identifier (x, y coordinates in cluster grid)
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClusterId(pub usize, pub usize);

impl ClusterId {
    pub fn new(x: usize, y: usize) -> Self {
        Self(x, y)
    }
    
    pub fn as_tuple(&self) -> (usize, usize) {
        (self.0, self.1)
    }
}

/// Region identifier (0-31 within a cluster)
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RegionId(pub u8);

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IslandId(pub u8);

/// A line segment representing a portal edge
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct LineSegment {
    pub start: FixedVec2,
    pub end: FixedVec2,
}

impl LineSegment {
    pub fn center(&self) -> FixedVec2 {
        (self.start + self.end) / FixedNum::from_num(2)
    }
    
    pub fn length(&self) -> FixedNum {
        (self.end - self.start).length()
    }
}

/// Axis-aligned bounding box
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Rect {
    pub min: FixedVec2,
    pub max: FixedVec2,
}

impl Rect {
    pub fn new(min: FixedVec2, max: FixedVec2) -> Self {
        Self { min, max }
    }
    
    pub fn contains(&self, point: FixedVec2) -> bool {
        point.x >= self.min.x && point.x <= self.max.x &&
        point.y >= self.min.y && point.y <= self.max.y
    }
    
    pub fn center(&self) -> FixedVec2 {
        (self.min + self.max) / FixedNum::from_num(2)
    }
    
    pub fn width(&self) -> FixedNum {
        self.max.x - self.min.x
    }
    
    pub fn height(&self) -> FixedNum {
        self.max.y - self.min.y
    }
}

/// Island (connected component of regions)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Island {
    pub id: IslandId,
    /// Representative position (center of a region in this island)
    pub representative: FixedVec2,
    /// Regions belonging to this island
    pub regions: SmallVec<[RegionId; MAX_REGIONS]>,
}

/// Unique identifier for a (cluster, island) pair in the macro graph
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ClusterIslandId {
    pub cluster: (usize, usize),
    pub island: IslandId,
}

impl ClusterIslandId {
    pub fn new(cluster: (usize, usize), island: IslandId) -> Self {
        Self { cluster, island }
    }
}

// ============================================================================
// Type-Safe Arena Index Wrappers (Zero-Cost Abstractions)
// ============================================================================

/// Cluster arena index in the global cluster grid
/// Computed as: cluster_y * clusters_x + cluster_x
/// 
/// Distinct from ClusterId which stores (x, y) tuple - this is the linearized index
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClusterArenaIdx(pub u32);

impl ClusterArenaIdx {
    /// Compute cluster arena index from cluster coordinates
    #[inline]
    pub fn from_coords(cluster_x: usize, cluster_y: usize, clusters_x: usize) -> Self {
        Self((cluster_y * clusters_x + cluster_x) as u32)
    }
}

/// Global region arena index
/// Computed as: cluster_idx * MAX_REGIONS + local_region_id
/// 
/// Distinct from RegionId which is a local ID (0..MAX_REGIONS) within a cluster
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionArenaIdx(pub u32);

impl RegionArenaIdx {
    /// Compute global region arena index from cluster index and local region ID
    #[inline]
    pub fn from_cluster_and_local(cluster_idx: ClusterArenaIdx, local_region_id: RegionId) -> Self {
        Self(cluster_idx.0 * MAX_REGIONS as u32 + local_region_id.0 as u32)
    }
}

impl IslandArenaIdx {
    /// Compute global island arena index from cluster index and local island ID
    #[inline]
    pub fn from_cluster_and_local(cluster_idx: ClusterArenaIdx, local_island_id: IslandId) -> Self {
        Self(cluster_idx.0 * MAX_ISLANDS as u32 + local_island_id.0 as u32)
    }
}

/// Global island arena index
/// Computed as: cluster_idx * MAX_ISLANDS + local_island_id
/// 
/// Distinct from IslandId which is a local ID (0..MAX_ISLANDS) within a cluster
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IslandArenaIdx(pub u32);

/// Local region ID within a cluster (0..MAX_REGIONS)
/// Type-safe wrapper to prevent confusion with global RegionArenaIdx
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalRegionId(pub u8);
