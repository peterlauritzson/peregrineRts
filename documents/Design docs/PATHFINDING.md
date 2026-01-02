# Peregrine Pathfinding Architecture: Hierarchical Pathfinding (HPA*)

## 1. The End Goal
To support **10 million units** on **large maps (2048x2048+)** with **dynamic obstacles** (building placement), we cannot use standard A* (too slow) or pure Flow Fields (too much memory for unique targets).

The solution is **Hierarchical Pathfinding A* (HPA*)**. This approach divides the map into local clusters and builds a high-level graph for long-distance navigation.

### Key Features
*   **Scalability:** Pathfinding cost depends on the number of *clusters*, not the number of *tiles*.
*   **Dynamic Updates:** Placing a building only triggers a re-calculation for the specific cluster it touches, not the whole map.
*   **Memory Efficient:** We store one abstract graph, and cache small *local* flow fields only for active clusters.
*   **Robust Movement:** Units use **Local Flow Fields** to navigate within a cluster. This makes them immune to getting "stuck" after collisions, as the map itself guides them back to the path.

---

## 2. Architecture & Data Structures

### Level 0: The Grid (The "Territory")
*   **Existing:** `FlowField` / `CostField`.
*   **Data:** A dense array of `u8` (1 = Walkable, 255 = Obstacle).
*   **Usage:** Used for final collision checks and local steering.

### Level 1: The Clusters (The "Counties")
*   **Concept:** The grid is divided into fixed-size square chunks (e.g., 16x16 tiles).
*   **Structure:**
    ```rust
    struct Cluster {
        id: (usize, usize), // Grid coordinates (e.g., 2, 4)
        portals: Vec<PortalId>,
        // Cache of distances between portals inside this cluster
        intra_cluster_costs: HashMap<(PortalId, PortalId), FixedNum>,
        // NEW: Cache of flow fields for specific exit portals
        flow_field_cache: HashMap<PortalId, LocalFlowField>,
    }
    ```

### Level 2: The Abstract Graph (The "Highway Network")
*   **Nodes (Portals):** A `Portal` is a transition point between two adjacent clusters.
    *   *Definition:* A Portal is a **range of tiles** (an edge segment), not a single point. This allows units to use the full width of a hallway.
*   **Edges:**
    1.  **Inter-Cluster:** Connection between two portals that share a border (Cost ~ 1.0).
    2.  **Intra-Cluster:** Connection between two portals *inside* the same cluster (Cost = calculated via local A*).
*   **Structure:**
    ```rust
    struct AbstractGraph {
        nodes: Vec<Portal>,
        edges: HashMap<PortalId, Vec<(PortalId, FixedNum)>>, // Adjacency list
    }
    ```

---

## 3. The Workflow

### A. Path Request
1.  **Unit** wants to go from `Start` to `Goal`.
2.  **System** identifies `StartCluster` and `GoalCluster`.
3.  **System** adds temporary nodes to the Abstract Graph: `StartNode` and `GoalNode`.
4.  **System** connects `StartNode` to all Portals in `StartCluster`, and `GoalNode` to all Portals in `GoalCluster`.

### B. High-Level Search
1.  Run **A*** on the **Abstract Graph**.
2.  **Result:** A list of Portals (e.g., `Start -> Portal A -> Portal B -> Goal`).

### C. Movement & Refinement (Hybrid Approach)
1.  **High-Level:** The Unit receives the list of Portals as "Waypoints" (e.g., `Portal A -> Portal B`).
2.  **Low-Level (Local Flow Fields):**
    *   The unit identifies which Cluster it is currently in.
    *   It requests a **Local Flow Field** for its immediate target (`Portal A`).
    *   If the field is not cached, the System generates it (using the *entire edge* of Portal A as the target, not just the center).
3.  **Physics:**
    *   The unit reads the vector from the tile under its feet.
    *   This vector is combined with Boids/Separation forces.
    *   **Robustness:** If a unit is pushed by a collision, it simply lands on a new tile and follows the new vector. No re-pathing required.

### D. Dynamic Updates (Building Placement)
1.  User places a wall in `Cluster (2, 2)`.
2.  **System** marks `Cluster (2, 2)` as "Dirty".
3.  **Update Step:**
    *   Re-scan the borders of `Cluster (2, 2)`. Did a Portal get blocked? -> Remove it.
    *   Re-calculate paths between remaining Portals inside `Cluster (2, 2)`.
    *   Update the `AbstractGraph` edges.
4.  **Result:** The global navigation mesh is updated instantly.

---

## 4. Implementation Roadmap

We will build this iteratively to ensure the game remains playable at every step.

### Phase 1: The Interface & Baseline (Standard A*)
*Goal: Decouple movement logic from pathfinding logic.*
1.  Define `PathRequest` Event and `Path` Component.
2.  Implement a **Standard A*** system that runs on the raw grid.
3.  Update `Unit` code to follow a list of waypoints instead of a single target.
    *   *Result:* Units can navigate complex mazes (slowly, but correctly).

### Phase 2: The Abstract Graph (Static)
*Goal: Implement the "Google Maps" layer.*
1.  Define `Cluster` size (e.g., 10x10 for now).
2.  Implement `GraphGenerator`:
    *   Iterate clusters.
    *   Create Portals (centers of edges).
    *   Connect Portals.
3.  Implement `HierarchicalPathfinder`:
    *   Replaces the Standard A* from Phase 1.
    *   Runs A* on the Graph nodes.
    *   Returns the list of Portal positions.

### Phase 3: Local Flow Fields (The "Last Mile")
*Goal: Robust movement and collision recovery.*
1.  Implement `LocalFlowField` struct (small grid, e.g., 10x10).
2.  Implement `FlowFieldCache` in the `Cluster` struct.
3.  Update Unit Movement Logic:
    *   Instead of `seek(target_pos)`, use `get_flow_vector(current_pos)`.
    *   Generate flow fields on demand when a unit requests a portal that isn't cached.

### Phase 4: Dynamic Updates
*Goal: Handle map changes.*
1.  Listen for `ObstacleAdded` events.
2.  Identify affected Clusters.
3.  **Invalidate Cache:** Clear the `flow_field_cache` for that cluster.
4.  Trigger `GraphGenerator::update_cluster(cluster_id)`.
5.  Verify that units re-path if their current path is invalidated.

### Phase 5: Optimization & Polish
*Goal: Smooth movement and performance.*
1.  **Portal Refinement:** Instead of "Center of Edge", find the actual walkable gaps.
2.  **Path Smoothing:** Post-process the path to remove "zig-zags" between portals.
3.  **Async Pathfinding:** Ensure the game doesn't stutter if 100 units request paths at once (Time-slicing).
