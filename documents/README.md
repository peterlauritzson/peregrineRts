# Peregrine Documentation
**Last Updated:** January 10, 2026

## Documentation Structure

### Active Issues
Issues that are currently being investigated or need attention:

- **[performance.md](Active%20Issues/performance.md)** - Performance bottlenecks and optimization opportunities
- **[pathfinding.md](Active%20Issues/pathfinding.md)** - Pathfinding bugs and investigations
- **[editor.md](Active%20Issues/editor.md)** - Map editor bugs and issues (mostly fixed)

### Design Documentation
Architectural designs and technical specifications:

- **[BOIDS_STEERING.md](Design%20docs/BOIDS_STEERING.md)** - Boids flocking algorithm design
- **[MAP_FILE_FORMAT.md](Design%20docs/MAP_FILE_FORMAT.md)** - Map file serialization format
- **[PATHFINDING.md](Design%20docs/PATHFINDING.md)** - Hierarchical pathfinding system
- **[SPATIAL_PARTITIONING.md](Design%20docs/SPATIAL_PARTITIONING.md)** - Spatial hash and proximity queries (supersedes old collision_handling.md)

### Guidelines
Project standards and coding practices:

- **[ARCHITECTURE.md](Guidelines/ARCHITECTURE.md)** - System architecture and design principles
- **[GUIDELINES.md](Guidelines/GUIDELINES.md)** - Development guidelines and best practices

### Planning
Project roadmaps and plans:

- **[PLAN.md](Planning/PLAN.md)** - Development roadmap
- **[POST_PRODUCTION.md](Planning/POST_PRODUCTION.md)** - Post-production tasks and polish

---

## Recent Changes (January 10, 2026)

### Documentation Cleanup
Reorganized documentation to separate active issues from fixed issues:

1. **Deleted deprecated files:**
   - `collision_handling.md` - Superseded by SPATIAL_PARTITIONING.md
   - `CURRENT_IMPROVEMENTS.md` - Contained mostly fixed issues mixed with a few active ones
   - `KNOWN_ISSUES.md` - Had 1 active issue and 1 fixed issue
   - `PERFORMANCE_BOTTLENECKS.md` - Performance data extracted to Active Issues
   - `STRESS_TEST_FINDINGS.md` - Specific bug report extracted to Active Issues

2. **Created Active Issues folder:**
   - Consolidated all unfixed, active issues into topic-specific files
   - Removed already-fixed issues from documentation
   - Organized by domain: performance, pathfinding, editor

3. **Benefits:**
   - Clear separation between active and fixed issues
   - Easier to find current problems that need attention
   - Design docs remain clean reference material
   - No clutter from historical "this was fixed" notes

---

## Quick Reference

### Finding Information

**I want to know about...**
- Current performance problems → [Active Issues/performance.md](Active%20Issues/performance.md)
- Pathfinding bugs → [Active Issues/pathfinding.md](Active%20Issues/pathfinding.md)
- How spatial partitioning works → [Design docs/SPATIAL_PARTITIONING.md](Design%20docs/SPATIAL_PARTITIONING.md)
- Project architecture → [Guidelines/ARCHITECTURE.md](Guidelines/ARCHITECTURE.md)
- Development roadmap → [Planning/PLAN.md](Planning/PLAN.md)
