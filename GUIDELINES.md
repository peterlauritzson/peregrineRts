# Code Quality Guidelines

To maintain a high standard for a AAA RTS codebase, we adhere to the following guidelines. These are enforced to ensure scalability, maintainability, and stability.

### 1. Configurability & Data-Driven Design
*   **No Hardcoded Magic Numbers**: Avoid `const SPEED: f32 = 10.0;` inside systems.
    *   *Solution*: Move all tunable values to `assets/game_config.ron` (or similar config resources).
    *   *Benefit*: Designers can tweak balance without recompiling.
*   **Variable Behaviors**: Avoid hardcoding logic branches based on specific unit types (e.g., `if unit.type == Marine`).
    *   *Solution*: Use Components to define behavior (e.g., `HasWeapon`, `CanMove`).
    *   *Benefit*: New units can be created by composition without touching code.

### 2. File Structure & Organization
*   **Avoid "God Files"**: Do not dump all logic into `game.rs` or `main.rs`.
    *   *Rule*: If a file exceeds 300-400 lines, consider splitting it.
*   **Folder-First Architecture**: Prefer `src/game/combat/damage.rs` over `src/game/combat_damage.rs`.
    *   *Structure*: Group related systems and components into modules (folders) with a `mod.rs` exposing the public API.
    *   *Benefit*: Easier navigation and logical grouping of features.

### 3. Testing Strategy
*   **Unit Tests**: Every helper function and complex logic block (especially math/physics) MUST have unit tests.
    *   *Location*: `#[cfg(test)] mod tests { ... }` at the bottom of the file.
*   **Integration Tests**: Critical game loops (e.g., "Unit moves to target") should be tested as integration tests.
    *   *Tool*: Use Bevy's testing tools to simulate a few ticks and assert state changes.
*   **Determinism Tests**: Automated tests to verify that running the simulation twice with the same seed produces identical results.

### 4. Type Safety & Clarity
*   **NewType Pattern**: Avoid passing raw `u32` or `f32` everywhere.
    *   *Example*: Use `struct UnitId(u32);` instead of `u32` for IDs. Use `struct Health(f32);` instead of `f32`.
    *   *Benefit*: Prevents accidental swapping of arguments (e.g., passing `damage` into `health`).
*   **Explicit States**: Use `enum`s for state machines (e.g., `UnitState::Idle`, `UnitState::Moving`) rather than boolean flags (`is_moving`, `is_idle`).

### 5. Documentation & Comments
*   **Public API**: All `pub` structs, enums, and functions must have `///` doc comments explaining *what* they do and *why*.
*   **Complex Logic**: Inline comments `//` should explain the *why* of a complex algorithm, not the *how* (the code shows the how).
