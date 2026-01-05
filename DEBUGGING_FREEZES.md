# Debugging Game Freezes - Quick Reference

## What Was Added

Enhanced logging to diagnose freezing when rapidly issuing move commands on large maps.

## Key Scenarios to Watch

### 1. **Path Request Accumulation** (Most Likely Cause)
When you rapidly click to move units multiple times:

**Look for:**
```
[PATHFINDING] High path request count: 50 pending requests!
```

**What it means:** Path requests are being created faster than they're being processed.

**Typical pattern:**
- First few requests process quickly
- Queue starts growing
- Each request takes longer
- Game freezes as queue explodes

### 2. **Infinite A* Loop**
**Look for:**
```
[PATHFINDING] A* exceeded max iterations (10000) - possible infinite loop!
```

**What it means:** Local A* pathfinding got stuck in an infinite loop, usually due to:
- Incorrect bounds calculation
- Corrupted flow field
- Edge case in pathfinding logic

### 3. **Slow Individual Path Requests**
**Look for:**
```
[PATHFINDING] Slow path request: 500ms
Processing path request 1/15 from ...
```

**What it means:** Single pathfinding request taking very long (> 50ms). Could be:
- Very complex path on large map
- Many portals to check
- Expensive A* calculation

### 4. **High A* Iteration Count**
**Look for:**
```
[PATHFINDING] A* used 5000 iterations (high!)
```

**What it means:** A* is exploring a huge search space. Might indicate:
- Large cluster size
- Poor heuristic
- Many obstacles creating complex paths

## Log Output Examples

### Normal Operation:
```
[SIM STATUS] Tick: 100 | Units: 5 | Active Paths: 3 | Last sim duration: 2ms
Processing path request 1/1 from (50.0, 30.0) to (120.0, 80.0)
Path found in 15ms: Hierarchical { ... }
```

### Problem - Request Accumulation:
```
[PATHFINDING] High path request count: 25 pending requests!
Processing path request 1/25 from ...
Processing path request 2/25 from ...
[PATHFINDING] Slow batch processing: 1500ms for 25 requests
[PATHFINDING] High path request count: 40 pending requests!  ⚠️ GROWING!
```

### Problem - Infinite Loop:
```
Processing path request 1/1 from (50.0, 30.0) to (120.0, 80.0)
[PATHFINDING] A* exceeded max iterations (10000) - possible infinite loop!
  Start: Node { x: 50, y: 30 }, Goal: Node { x: 120, y: 80 }
  Bounds: (0, 0) to (200, 200)
```

## What to Do When You See These

1. **High pending requests:**
   - Add a request limit (e.g., max 5 pending per unit)
   - Cancel old requests when issuing new ones
   - Process requests across multiple frames

2. **Infinite loop:**
   - Check the logged bounds - are they valid?
   - Check if goal is reachable
   - Look at flow field around that area

3. **Slow requests:**
   - Reduce map size or cluster size
   - Increase `pathfinding_build_batch_size` in config
   - Consider path caching

## Testing

To reproduce the freeze:
1. `cargo run`
2. Go to Map Editor
3. Generate a large map (e.g., 512x512) with many obstacles (e.g., 200+)
4. Create 5-10 units
5. Rapidly click to move them to different locations (click 3-4 times quickly)

Watch the console output carefully!
