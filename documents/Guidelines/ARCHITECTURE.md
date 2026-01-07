# Peregrine: Technical Architecture & Guidelines

# 🛑 CRITICAL ARCHITECTURAL GUIDELINES (DO NOT VIOLATE) 🛑

### 1. Absolute Determinism (The "Golden Rule")
*   **Definition**: The Simulation State MUST evolve identically on all clients given the same inputs. A checksum of the SimState at Tick X must match across all machines.
*   **Math**: Avoid standard `f32`/`f64` for simulation logic if cross-platform determinism is required. Prefer **Fixed-Point Arithmetic** (e.g., `fixed` crate) for positions, velocities, and physics.
    *   *Exception*: Visuals/Rendering can use floats freely.
*   **Collections**: **NEVER** iterate over `HashMap` or `HashSet` in the simulation loop. Iteration order is undefined and non-deterministic. Use `BTreeMap`, sorted `Vec`, or `IndexMap`.
*   **RNG**: **NEVER** use `rand::thread_rng()`. Use a deterministic PRNG (e.g., `ChaCha8`) seeded from the game config/match setup, stored in the Sim resource.
*   **Time**: **NEVER** use `Time::delta_seconds()` in the simulation. Use a fixed delta time constant (e.g., `1.0/60.0`).

### 2. Strict Sim/Render Separation
*   **Simulation Layer**: Contains *only* logical data (SimPosition, Health, Cooldowns). Runs at a fixed tick rate (e.g., 20Hz or 60Hz).
*   **Presentation Layer**: Contains visual data (Transform, Meshes, Audio). Runs at variable frame rate (unlocked).
*   **Rule**: The Presentation Layer *reads* the Simulation Layer to interpolate visuals. The Simulation Layer **NEVER** knows about the Presentation Layer.

### 3. Performance & Scalability (The "10M Unit" Goal)
*   **Data-Oriented**: Keep "hot" simulation components small and contiguous. Avoid `Box<T>` or heavy nesting in components.
*   **Batching**: Do not run A* pathfinding for 10,000 units individually. Use **Flow Fields** or group steering.
*   **No Allocations in Hot Loops**: Avoid memory allocation (Vec::new(), HashMap::new(), String::from()) inside per-frame simulation systems.
    *   **Why**: Memory allocation is expensive (10-1000ns per allocation) and non-deterministic (timing varies based on allocator state).
    *   **Heap Fragmentation**: Allocating/deallocating every frame causes memory fragmentation, leading to cache misses and slower allocations over time.
    *   **GC Pressure**: While Rust has no GC, frequent allocations still trigger OS-level memory management overhead.
    *   **Solution**: 
        - Use `Local<T>` resources to cache pre-allocated buffers across frames
        - Use `Vec::with_capacity()` at startup, then `.clear()` instead of recreating
        - Prefer `query.get()` for lookups over building temporary HashMaps
    *   **Example**: Building a HashMap of 3500 units every frame = 3500 allocations + hashing + copying. Instead, use direct query lookups or cached Local<HashMap>.
    *   **Measurement**: With 3500 units, eliminating one Vec/HashMap per frame saves ~0.5-2ms per tick.

### 4. Responsiveness (The "E-Sport" Feel)
*   **Instant Feedback**: When a player clicks, play the sound and show the marker *immediately* (Frame 0), even if the network command takes 100ms to execute.
*   **Crisp Movement**: Physics should be snappy (high acceleration/deceleration), not "floaty".

---

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
