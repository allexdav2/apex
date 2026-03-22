<!-- status: PARKED -->

# Worktree Agent Hygiene — Preventing Silent Regressions

Date: 2026-03-16
Goal: Close three failure modes in the fleet worktree agent workflow.

## Problem Statement

Three issues hit during parallel agent dispatch on 2026-03-16:

1. **Stale base** — Agents branched from `78ac7e8` (4 commits behind HEAD) instead of current main. Patches were against an older codebase.
2. **Uncommitted changes** — 3 of 5 agents left changes as working tree modifications instead of committing. Patch extraction required `git diff HEAD` guesswork.
3. **Patch revert** — An agent's scope overlapped with files modified after its branch point. Applying its patch silently reverted security fixes made between branching and merge.

## Root Cause Analysis

**Issue 1** is a Claude Code `isolation: "worktree"` behavior question. When the Agent tool creates a worktree, it branches from whatever commit the main checkout is on. If the main checkout hasn't pulled or has detached HEAD, the agent starts behind. The fleet officer's pre-dispatch checklist says "run `git status`" but doesn't say "ensure HEAD is the commit you think it is."

**Issue 2** is an instruction gap. The crew agent template says "commit to your branch" in the Worktree Isolation section and "Run tests, commit" in the plan steps, but there's no hard enforcement. An agent that runs out of context or hits an error may return without committing.

**Issue 3** is the most dangerous. The current CLAUDE.md says "NEVER copy files wholesale, use diffs" and warns about "struct drift." But the guidance assumes the officer notices conflicts during `git apply`. When a patch cleanly applies but silently reverts an intervening change (because the old state and the agent's new state happen to be compatible), there's no conflict and no warning.

## Proposed Solutions

### Solution 1: Pre-Dispatch Commit Anchor

Add to the captain's dispatch protocol and to `CLAUDE.md`:

**Before dispatching any agent, record the exact HEAD commit and verify it:**

```bash
# Captain records the dispatch anchor
DISPATCH_BASE=$(git rev-parse HEAD)
echo "Dispatching agents from base: $DISPATCH_BASE"

# Verify this is actually where we intend to branch from
git log --oneline -3  # visual confirmation
```

**Include the anchor in every agent's prompt:**

```
You are branching from commit $DISPATCH_BASE.
After creating your worktree, verify: git rev-parse HEAD should output $DISPATCH_BASE.
If it does not, STOP and report the mismatch in your FLEET_REPORT.
```

This is cheap — one line in the dispatch prompt, one check in the agent's Phase 1 (Assess).

**CLAUDE.md addition** to Pre-Dispatch Checklist:

```markdown
1. Run `git status` — check for uncommitted changes
2. Record dispatch anchor: `DISPATCH_BASE=$(git rev-parse HEAD)`
3. If changes exist in files crew will touch:
   a. Create a WIP commit: `git commit -am "WIP: pre-dispatch snapshot"`
   b. Update: `DISPATCH_BASE=$(git rev-parse HEAD)`
4. Include $DISPATCH_BASE in every agent's prompt
5. Dispatch crew agents
```

### Solution 2: Commit Enforcement in Crew Agent Template

Add to the crew agent's Phase 3 (Verify + Report) in the agent markdown, right before the FLEET_REPORT section:

```markdown
### Mandatory Commit

Before writing your FLEET_REPORT, you MUST commit all changes:

1. `git add -A` within your worktree
2. `git status` — verify no uncommitted changes remain
3. `git commit -m "<crew>: <one-line summary>"`
4. Record the commit hash for your FLEET_REPORT

If you cannot commit (build fails, tests fail), commit anyway with a WIP prefix:
`git commit -am "WIP: <crew> — <what's broken>"`

An uncommitted worktree is an unrecoverable worktree. The captain cannot reliably
extract your changes without a commit boundary.
```

Also add to the Red Flags table:

```
| "I'll let the captain extract my uncommitted changes" | Commit. Always. Even WIP. |
```

### Solution 3: Patch Safety Check Script

This is the core fix for Issue 3. Create `scripts/fleet-patch-safety.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

# Usage: fleet-patch-safety.sh <worktree-path> <dispatch-base-commit>
#
# Checks whether applying a worktree's changes would revert any
# commits made to main AFTER the dispatch base.
#
# Exit 0: safe to apply
# Exit 1: would revert changes — prints affected files and details

WORKTREE="$1"
DISPATCH_BASE="$2"
CURRENT_HEAD=$(git rev-parse HEAD)

# 1. Find files changed on main SINCE dispatch
MAIN_CHANGED=$(git diff --name-only "$DISPATCH_BASE"..."$CURRENT_HEAD")

if [ -z "$MAIN_CHANGED" ]; then
    echo "No changes on main since dispatch base — safe to apply."
    exit 0
fi

# 2. Find files changed in the worktree
WORKTREE_CHANGED=$(git -C "$WORKTREE" diff --name-only "$DISPATCH_BASE"...HEAD)

# 3. Intersection = danger zone
OVERLAP=$(comm -12 <(echo "$MAIN_CHANGED" | sort) <(echo "$WORKTREE_CHANGED" | sort))

if [ -z "$OVERLAP" ]; then
    echo "No file overlap between main changes and worktree changes — safe to apply."
    exit 0
fi

echo "WARNING: The following files were modified BOTH on main (after dispatch)"
echo "AND in the worktree. Applying the worktree patch may revert main's changes:"
echo ""
echo "$OVERLAP"
echo ""
echo "Main changes since dispatch ($DISPATCH_BASE...$CURRENT_HEAD):"
for f in $OVERLAP; do
    echo "--- $f ---"
    git diff "$DISPATCH_BASE"..."$CURRENT_HEAD" -- "$f" | head -30
    echo "..."
done
echo ""
echo "Worktree changes ($DISPATCH_BASE...worktree HEAD):"
for f in $OVERLAP; do
    echo "--- $f ---"
    git -C "$WORKTREE" diff "$DISPATCH_BASE"...HEAD -- "$f" | head -30
    echo "..."
done

exit 1
```

**Integration into the merge protocol (CLAUDE.md):**

```markdown
### Merging Crew Changes
NEVER copy files wholesale from worktrees. Always use diffs:
1. **Safety check:** `./scripts/fleet-patch-safety.sh <worktree> $DISPATCH_BASE`
   - If exit 0: proceed
   - If exit 1: review overlapping files manually, apply only non-conflicting hunks
2. Generate patch: `git -C <worktree> diff $DISPATCH_BASE -- crates/ > /tmp/crew-name.patch`
3. Apply to main: `git apply /tmp/crew-name.patch`
4. If patch conflicts, read the diff and apply changes manually to specific lines
```

Note: diffing against `$DISPATCH_BASE` (not `HEAD`) in step 2 is critical. The current CLAUDE.md says `diff HEAD` which diffs against the worktree's own HEAD — this is correct for uncommitted changes but wrong for committed changes. If the agent committed (as Solution 2 enforces), the diff should be `$DISPATCH_BASE...<branch>`.

### Solution 4: File Scope Guards in Crew Agent Template

The crew agent template already says "DO NOT edit files outside your owned paths" in the Constraints section. But Issue 3 happened because the agent's *scope assignment* in the plan overlapped with another crew's territory, not because the agent violated its paths.

The fix is in the **captain's planning phase**, not the crew template. Add to the captain protocol:

```markdown
### File Scope Deconfliction (Phase 2: Plan)

Before finalizing the plan, verify no two tasks in the same wave touch the same file:

1. For each wave, collect all files listed in task assignments
2. Check for duplicates across tasks within the same wave
3. If overlap exists, either:
   a. Move one task to a later wave (sequential, not parallel)
   b. Split the file's changes into separate non-overlapping hunks and assign each to one task
   c. Assign both changes to the same crew/task

Also verify: no task assigns files that were modified by a DIFFERENT completed task
in an earlier wave. If Wave 1 modifies `lib.rs`, and Wave 2 also needs `lib.rs`,
the Wave 2 agent must be told: "This file was modified in Wave 1. Your base will
include those changes."
```

## Summary of Changes

| Solution | Where | Effort | Prevents |
|----------|-------|--------|----------|
| 1. Commit anchor | CLAUDE.md + captain protocol + crew prompt | Low | Stale base (Issue 1) |
| 2. Mandatory commit | Crew agent template (all 7 `.md` files) | Low | Uncommitted changes (Issue 2) |
| 3. Patch safety script | New `scripts/fleet-patch-safety.sh` + CLAUDE.md | Medium | Silent reverts (Issue 3) |
| 4. Scope deconfliction | Captain protocol (planning phase) | Low | Overlapping scope (Issue 3 root cause) |

## What This Does NOT Solve

- **Claude Code's worktree implementation** — we can't control how `isolation: "worktree"` picks the base commit. The anchor check is a detection mechanism, not prevention. If Claude Code changes its worktree behavior, we'd need to update.
- **Agent context exhaustion** — if an agent runs out of context mid-implementation, it can't commit. Solution 2 helps (commit early, commit often) but can't guarantee it.
- **Three-way merge** — the patch safety script detects conflicts but doesn't resolve them. The officer still has to do manual hunk-by-hunk merging for overlapping files.

## Implementation Plan

All four solutions are independent and can be implemented in a single commit:

1. Update `CLAUDE.md` — Pre-Dispatch Checklist and Merging Crew Changes sections
2. Update all 7 crew agent templates in `.claude/agents/apex-crew-*.md` — add Mandatory Commit section
3. Create `scripts/fleet-patch-safety.sh`
4. Update captain protocol in `.claude/agents/apex-captain.md` — add scope deconfliction to Phase 2

Estimated effort: 30-45 minutes for all four.
