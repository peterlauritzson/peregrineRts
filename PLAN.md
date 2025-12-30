# Peregrine: Technical Design & Roadmap

## Technical Architecture: The "Starcraft 2" Standard
To achieve a high unit count, deterministic multiplayer, and snappy feel, we will adopt the following architectural pillars:

### 1. Deterministic Simulation (The "Sim")
- **Lockstep Networking**: We will send *inputs*, not unit positions. All clients simulate the game state locally based on these inputs.
- **Fixed Timestep Loop**: 
    - The simulation will run at a configurable fixed tick rate (e.g., 16-20 ticks per second), decoupled from the rendering frame rate.
    - Each tick will execute a strictly ordered sequence of systems (Input -> AI -> Physics -> State Update).
    - This ensures that the simulation state is identical across all clients given the same inputs.
- **Deterministic Math**: We must avoid non-deterministic operations (e.g., standard floating-point behavior across different architectures). We will likely need a fixed-point math library or a strictly controlled float wrapper.
- **Separation of Concerns**:
    - **Sim World**: Contains logical entities (Unit ID, Position, Health, Velocity). No meshes, no textures.
    - **Render World**: Bevy's standard ECS. Reads the Sim World state and interpolates it for smooth 60+ FPS rendering.

### 2. Performance (High Unit Count)
- **Data-Oriented Design**: Leveraging Bevy's ECS to keep unit data contiguous in memory.
- **Flow Field Pathfinding**: For moving hundreds of units (Zerglings/Marines) efficiently, we will use Flow Fields rather than calculating A* for every single unit.
- **Boids / Steering Behaviors**: For local collision avoidance so units flow around each other naturally.

### 3. "Snappy" Multiplayer Feel
- **Input Latency Hiding**:
    - **Immediate Feedback**: When a player clicks, play the "Yes, sir!" sound and show the move marker *immediately*, even if the network command hasn't round-tripped yet.
    - **Command Queueing**: Buffer inputs slightly to ensure smooth execution despite network jitter.

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
   - **Decoupled Loop**: Implement a fixed-timestep loop (e.g., using Bevy's `FixedUpdate`) that runs independently of the frame rate.
   - **System Ordering**: Define a strict execution order for simulation systems (e.g., `InputProcessing` -> `Movement` -> `CollisionDetection` -> `CollisionResolution`).
   - **Tick Speed Control**: Add the ability to adjust the simulation tick rate dynamically (for testing and "fast-forward" replays).
   - **Sim vs. Render State**: Separate logical unit data (e.g., `SimPosition`) from visual data (e.g., `Transform`).

1. **Single Unit, Free Space**
   - One unit.
   - No collisions, no borders.
   - Direct movement to target calculated within the Sim loop.
   - **Visual Interpolation**: Implement basic interpolation so the unit moves smoothly between Sim ticks.

2. **Multiple Units, Ghosts**
   - Spawn multiple units.
   - They move to targets but pass through each other (no collision).
   - Verify that the Sim loop handles multiple units correctly and deterministically.

3. **The Box (Map Borders)**
   - Define map boundaries in the Sim world.
   - Units stop or slide when hitting the edge of the map (Sim-side logic).

4. **Basic Unit-Unit Collision (Detection)**
   - Implement spatial hashing or simple N^2 check in the Sim loop.
   - Detect when units overlap.
   - Visual debug indication of overlap (Render-side feedback).

5. **Basic Unit-Unit Collision (Resolution)**
   - Units push each other apart in the Sim loop.
   - "Soft" collisions (separation force) to prevent stacking.
   - Ensure resolution is deterministic.

6. **External Forces**
   - Apply a global force (e.g., "Gravity" pulling to the center or a specific direction).
   - Units must fight this force or be dragged by it.

7. **Static Obstacles**
   - Place simple shapes (circles/rectangles) as walls.
   - Units collide and slide against these static obstacles.

8. **Steering Behaviors (Boids)**
   - Implement Separation, Alignment, and Cohesion.
   - Units move as a flock rather than individual particles.

9. **Pathfinding (The Labyrinth)**
   - Create a complex map (maze-like).
   - Implement Flow Fields or A* for pathfinding.
   - Units navigate from Start to Goal avoiding walls.

10. **The "Million" Unit Stress Test**
    - Massive unit count.
    - Full simulation: Borders, Unit Collisions, External Forces, Complex Pathfinding.
    - Optimization pass (Spatial Partitioning, Multithreading, GPU Compute if needed).

---

# UI & Interaction Roadmap (Step-by-Step)

1. **Hardcoded View**
   - Fixed camera position and angle.
   - No UI elements, just the rendered scene.

2. **Debug Text Overlay**
   - Simple FPS counter.
   - Display unit count or basic debug info using text on screen.

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
   - Keybinding configuration menu.
   - Graphics settings (Resolution, VSync).
   - Save/Load settings to disk.

10. **The "Polished" Experience**
    - Fully customizable UI.
    - Save/Load Game state.
    - Styled UI themes, animations, and sound effects for interactions.
    - Complete game loop with Win/Loss screens.

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



