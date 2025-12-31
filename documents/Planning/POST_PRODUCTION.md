# Post-Production & Polish (The "Don't Forget" List)

These are critical tasks that often get deprioritized but are essential for a shipping product.

### 1. Tooling & Content Pipeline
*   **Map Editor**: We cannot build a campaign in code. We need a visual Level Editor (or integration with Tiled/LDtk/Blender) to place units, terrain, and triggers.
*   **Asset Pipeline**: Automate the import of models and textures. Ensure assets are optimized (compressed textures, LODs) during the build process.

### 2. Audio Experience
*   **Sound Quality**: Replace placeholder "beeps" with high-quality SFX.
*   **Dynamic Mixing**: Implement audio ducking (e.g., lower music volume when combat is intense or voice lines are playing).
*   **Spatial Audio**: Ensure sounds are correctly positioned in 3D space (panning, attenuation).

### 3. Visual Fidelity
*   **Asset Replacement**: Replace all "programmer art" (cubes/capsules) with final 3D models and animations.
*   **VFX Polish**: Add particle effects for impacts, dust trails, explosions, and selection rings.
*   **Lighting & Post-Processing**: Implement bloom, color grading, and dynamic shadows to unify the visual style.

### 4. User Experience (UX)
*   **Accessibility**: Colorblind modes, UI scaling, and remappable controls.
*   **Tutorials**: Interactive onboarding to teach mechanics (not just text boxes).
*   **Loading Screens**: Meaningful loading indicators (no frozen screens).

### 5. Performance Optimization
*   **Profiling**: Rigorous profiling on min-spec hardware.
*   **Load Times**: Optimize asset loading (async loading, texture streaming).
