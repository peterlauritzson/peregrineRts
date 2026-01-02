# Peregrine: Technical Design & Roadmap

This document contains the step-by-step roadmaps for the development of Peregrine.

**Related Documents:**
*   [Architecture & Technical Design](ARCHITECTURE.md)
*   [Code Quality Guidelines](GUIDELINES.md)
*   [Post-Production & Polish](POST_PRODUCTION.md)

---

# Peregrine - Milestone 0: The RTS Foundation

Goal: Create a minimal, testable RTS loop where a player can select a unit and command it to move.

## 1. Project Initialization & Window Setup
- [x] Verify Bevy dependency and basic application loop.
- [x] Configure the Window (Title, Resolution, Resizable).
- [x] Set up a basic "Game" plugin structure to keep code organized.

## 2. The RTS Camera
- [x] Implement a top-down camera (3D Perspective or Orthographic).
- [x] Position it at an angle (classic RTS view).
- [x] (Optional for M0) Basic camera panning with WASD or edge scrolling.

## 3. The Environment
- [x] Spawn a simple ground plane (The "Map").
- [x] Add basic lighting so 3D objects are visible.

## 4. The Unit
- [x] Create a `Unit` component.
- [x] Spawn a simple 3D primitive (Cube or Capsule) to represent a "Marine" or "Zergling".
- [x] Ensure the unit sits correctly on the ground plane.

## 5. Input & Selection (The "Click")
- [x] Implement Raycasting from the mouse cursor to the world.
- [x] **Left Click**:
    - Check if the ray hits a Unit.
    - If yes, mark it as `Selected`.
    - Visual Feedback: Change the unit's color or add a selection circle when selected.
    - If clicking empty ground, deselect.

## 6. Movement Command (The "Right-Click")
- [x] **Right Click**:
    - Check if the ray hits the Ground.
    - If a unit is `Selected`, calculate the target position.
    - Store the target position in a `MoveTarget` component on the unit.
- [x] **Movement System**:
    - Move the unit towards the `MoveTarget` every frame.
    - Stop when close enough to the target.
    - (Bonus) Simple rotation to face the target.

## 7. Testing & Verification
- [x] Run the game.
- [x] Select the unit (color changes).
- [x] Right-click ground (unit moves there).

---

# Physics & Simulation Roadmap (Step-by-Step)

Once the RTS Foundation (Milestone 0) is complete, we will iterate on the simulation engine following this progression:

0. **The Simulation Foundation (Fixed Timestep)**
   - [x] **Decoupled Loop**: Implement a fixed-timestep loop (e.g., using Bevy's `FixedUpdate`) that runs independently of the frame rate.
   - [x] **System Ordering**: Define a strict execution order for simulation systems (e.g., `InputProcessing` -> `Movement` -> `CollisionDetection` -> `CollisionResolution`).
   - [x] **Tick Speed Control**: Add the ability to adjust the simulation tick rate dynamically (for testing and "fast-forward" replays).
   - [x] **Sim vs. Render State**: Separate logical unit data (e.g., `SimPosition`) from visual data (e.g., `Transform`).

1. **Single Unit, Free Space**
   - [x] One unit.
   - [x] No collisions, no borders.
   - [x] Direct movement to target calculated within the Sim loop.
   - [x] **Visual Interpolation**: Implement basic interpolation so the unit moves smoothly between Sim ticks.

2. **Multiple Units, Ghosts**
   - [x] Spawn multiple units.
   - [x] They move to targets but pass through each other (no collision).
   - [x] Verify that the Sim loop handles multiple units correctly and deterministically.

3. **The Box (Map Borders)**
   - [x] Define map boundaries in the Sim world.
   - [x] Units stop or slide when hitting the edge of the map (Sim-side logic).

4. **Basic Unit-Unit Collision (Detection)**
   - [x] Implement spatial hashing or simple N^2 check in the Sim loop.
   - [x] Detect when units overlap.
   - [x] Visual debug indication of overlap (Render-side feedback).

5. **Basic Unit-Unit Collision (Resolution)**
   - [x] Units push each other apart in the Sim loop.
   - [x] "Soft" collisions (separation force) to prevent stacking.
   - [x] Ensure resolution is deterministic.

6. **External Forces (Wind/Flow Field)**
   - [x] Add a global resource `GlobalFlow` (vector field or just a constant wind for now).
   - [x] Apply this force to all units in the Sim loop.
   - [x] Verify units drift over time.

7. **Static Obstacles**
   - [x] Place simple shapes (circles/rectangles) as walls.
   - [x] Units collide and slide against these static obstacles.

8. **Steering Behaviors (Boids)**
   - [x] Implement Separation, Alignment, and Cohesion.
   - [x] Units move as a flock rather than individual particles.

9. **Pathfinding (The Labyrinth)**
   - [x] Create a complex map (maze-like).
   - [x] Implement Hierarchical Pathfinding (HPA*) for scalable navigation.
   - [x] Units navigate from Start to Goal avoiding walls using high-level graph and local A*.

   > **Note:** Before attempting the 10M unit goal, we must replace the $O(N^2)$ collision checks with a Spatial Hash Grid or Quadtree to improve performance.
   > **Note:** HPA* implementation is complete (Level 0, 1, 2). Units now use the hierarchical graph for long-distance planning.

10. **The "Million" Unit Stress Test**
    - [ ] **A. Baseline CPU Stress Test**:
        - Configure environment: Set logging to `WARN`+ and ensure FPS/TPS counters are active.
        - Spawn 10k-100k units with current CPU implementation.
        - Gradually increase map size along with unit count to maintain reasonable density.
        - Profile performance bottlenecks (collision, pathfinding, rendering).
    - [ ] **B. Spatial Partitioning Optimization**: Ensure Spatial Hash Grid is fully optimized and cache-friendly. Implement multithreading for simulation steps if not already present.
    - [ ] **C. GPU Compute Foundation**: Move core simulation logic (movement, collision) to Compute Shaders (WGPU). Verify data roundtrip between CPU and GPU.
    - [ ] **D. GPU-Based Rendering**: Implement instanced rendering directly from GPU simulation buffers (avoiding CPU readback).
    - [ ] **E. The 10M Goal**: Scale to 10M units. Tune simulation parameters and grid sizes for maximum throughput. Achieve 1000 FPS / 100 TPS target.

---

# Physics Refinement & Polish

1. **Physics-Based Movement**
   - [ ] Implement elastic collisions (bouncing) as a configurable option.
   - [ ] Allow walls/obstacles to have different physics materials (bouncy vs. sticky).
   - [ ] "Pinball" feel: Units should feel like physical objects influenced by forces (gravity, wind) rather than just steering agents. This should be configurable.

2. **Configurable Simulation**
   - [ ] Move all hardcoded constants (starting units, speed, friction, restitution) to `game_config.ron`.
   - [ ] Allow runtime reloading of physics parameters to tweak the "feel".

---

# UI & Interaction Roadmap (Step-by-Step)

1. **Hardcoded View**
   - Fixed camera position and angle.
   - No UI elements, just the rendered scene.

2. **Debug Text Overlay**
   - Simple FPS counter.
   - Display unit count or basic debug info using text on screen.
   - Ensure high FPS is maintainable (performance monitoring).

3. **Basic Camera Control**
   - WASD movement.
   - Zoom in/out.
   - Clamp camera to map bounds.

4. **Selection Visuals**
   - Draw a selection box (drag-select).
   - Highlight selected units (circles or outlines).

5. **Simple HUD (Heads-Up Display)**
   - Bottom bar panel.
   - Display info of the currently selected unit (e.g., "Marine", Health: 100).

6. **Command Card**
   - Buttons for actions (Move, Stop, Attack).
   - Clicking a button triggers the action for selected units.

7. **Minimap Prototype**
   - A small rectangle showing unit positions as dots.
   - Click on minimap to move camera.

8. **Main Menu & Game States**
   - Start Screen (Play, Quit).
   - Pause Menu (Resume, Quit).
   - State transitions (Menu -> Game -> Pause).

9. **Settings & Configuration**
   - Keybinding configuration menu (move keys to config).
   - Graphics settings (Resolution, VSync).
   - Save/Load settings to disk.

10. **The "Polished" Experience**
    - Fully customizable UI.
    - Save/Load Game state.
    - Styled UI themes, animations, and sound effects for interactions.
    - Complete game loop with Win/Loss screens.

---

# Gameplay: Simple RTS

1. **Combat Basics**
   - [ ] Units have Health and Damage.
   - [ ] Attack range checks.
   - [ ] Projectile simulation (if not hitscan).

2. **Unit Interactions**
   - [ ] Friendly fire logic (optional).
   - [ ] Reaction to being hit (knockback, flashing).
   - [ ] Destruction/Death effects.

3. **Lifelike Behaviors (Emergent Complexity)**
   - [ ] **Predator/Prey Dynamics**: Define factions (e.g., "Wolves" vs "Sheep"). Wolves auto-hunt Sheep; Sheep auto-flee Wolves.
   - [ ] **Self-Preservation**: Units flee from enemies if HP is low (< 20%).
   - [ ] **Social Aggro**: If a unit is attacked, nearby allies automatically target the attacker ("Help call").
   - [ ] **Idle Wandering**: Units shouldn't stand perfectly still. They should patrol or wander slightly when idle.
   - [ ] **Vision/Awareness**: Units only react to things within a certain "Vision Radius" (Fog of War logic).


---

# Multiplayer & Networking Roadmap (Step-by-Step)

1. **Single Player (Local Loop)**
   - Game runs entirely locally.
   - Input directly modifies the local simulation state (no separation yet).

2. **Local "Fake" Multiplayer**
   - Simulate two players on one machine (e.g., Split-screen or just two sets of units).
   - Refactor code to distinguish between "Local Player" (Input source) and "Player ID" (Unit owner).
   - Ensure Player 1 cannot control Player 2's units.

3. **Basic Transport Layer**
   - Integrate a networking library (e.g., `matchbox` for WebRTC or `renet` for UDP).
   - Establish a connection between two clients (Host/Client or P2P).
   - Send simple text messages ("Ping", "Pong") to verify connectivity.

4. **Naive Input Forwarding**
   - Send input commands (e.g., "Move Unit X to Y") to the other peer.
   - Apply remote inputs immediately upon receipt.
   - *Note: This will likely desync quickly, but proves data transfer.*

5. **The Lockstep Protocol (Basic)**
   - Implement a "Turn" or "Tick" system.
   - Game simulation pauses until inputs for the current tick are received from all players.
   - "Stop-and-Wait" implementation.

6. **Determinism Verification**
   - Implement a state hasher (Checksum).
   - Compare checksums between clients every N ticks.
   - Log "Desync Detected" if they mismatch.

7. **Command Queue & Latency Buffer**
   - Instead of pausing every tick, buffer inputs for future ticks (e.g., execute input at Tick T+3).
   - Smooth out network jitter.

8. **Latency Hiding (Visuals)**
   - Implement the "Client-Side Prediction" for visuals only (Audio/Markers).
   - Unit acknowledges command immediately (sound/animation), even if movement waits for the lockstep tick.

9. **Robustness & Reconnection**
   - Handle packet loss (resend logic if not handled by transport).
   - Handle temporary disconnects (pause game, wait for reconnect).
   - Lobby system for finding peers.

10. **Snappy, Deterministic Lockstep**
    - Ultra-high demand multiplayer.
    - Optimized serialization for minimal bandwidth.
    - Rock-solid determinism across different hardware.
    - Smooth experience even with moderate latency.






