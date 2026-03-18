<!-- status: DONE -->

# Fleet Review Bug Fix Pass

## File Map

| Crew | Files | Bugs |
|------|-------|------|
| mcp-integration | crates/apex-cli/src/mcp.rs | BUG-4 (HIGH), BUG-3 (MEDIUM) |
| exploration | crates/apex-concolic/src/python.rs | BUG-1 (LOW) |
| security-detect | crates/apex-detect/src/dep_graph.rs | BUG-2 (KNOWN) |

## Wave 1 (all parallel -- no dependencies)

### Task 1.1 -- mcp-integration crew
**Files:** crates/apex-cli/src/mcp.rs
- [x] BUG-4: Change `"attack-surface"` to `"reach"` at line 216
- [x] BUG-3: Wire `lang` parameter through to `deploy-score` command
- [x] Run clippy, run tests

### Task 1.2 -- exploration crew
**Files:** crates/apex-concolic/src/python.rs
- [x] BUG-1: Replace `.unwrap()` with `.unwrap_or_else(|e| e.into_inner())` at line 166
- [x] Run clippy, run tests

### Task 1.3 -- security-detect crew
**Files:** crates/apex-detect/src/dep_graph.rs
- [x] BUG-2: Replace DFS with Tarjan's SCC algorithm
- [x] Add test for cycles reachable from already-visited nodes
- [x] Run clippy, run tests
