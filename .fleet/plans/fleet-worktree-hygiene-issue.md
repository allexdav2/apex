# Worktree agent dispatch has 3 failure modes causing silent data loss

## Problem

During a parallel crew dispatch session (4 Wave 1 crews + 5 hunter agents), we hit three failure modes in the worktree isolation workflow.

### 1. Agents branch from stale commits

When dispatching worktree agents with `isolation: "worktree"`, some agents branched from commits 2-4 behind HEAD instead of current main. This caused patches to be against an older codebase, requiring manual conflict resolution during merge.

**Impact:** Moderate — patches fail to apply cleanly, requiring manual intervention.

### 2. Agents don't always commit their work

3 of 5 hunter agents left changes as uncommitted working tree modifications instead of committing. This made patch extraction unreliable — `git diff HEAD` from the worktree captured changes, but `git diff <base>..<branch>` showed nothing.

**Impact:** Moderate — makes the fleet officer's merge workflow fragile and non-deterministic.

### 3. Overlapping file patches silently revert prior fixes

A hunter agent was assigned `crates/apex-cli/` scope, which overlapped with files (`mcp.rs`, `lib.rs`) that had been modified by a security fix committed *after* the agent branched. When the fleet officer applied the agent's patch, it silently reverted the security validations — caught only by a system notification, not by `git apply`.

**Impact:** Critical — security fixes were silently reverted. This is data loss.

## Root Cause Analysis

- The Pre-Dispatch Checklist in CLAUDE.md captures `git status` but never records or communicates the dispatch base commit hash to agents
- No agent template enforces committing before returning results
- `git apply` has no concept of "this patch should not touch files modified after the branch point"
- The captain's planning phase doesn't deconflict file scope across parallel tasks in the same wave

## Proposed Solutions

### 1. Commit Anchor (prevents stale base)

Record `DISPATCH_BASE=$(git rev-parse HEAD)` before dispatch, inject into agent prompts, verify worktree HEAD matches on startup. This is one line in the dispatch sequence and one check in the agent's assess phase.

### 2. Mandatory Commit (prevents uncommitted worktrees)

Agent templates require commit before returning (even `WIP:` prefix). An uncommitted worktree is an unrecoverable worktree — the captain can't reliably diff against a known base without a commit boundary.

### 3. Patch Safety Script (prevents silent reverts)

New script that computes the intersection of:
- Files changed on main since `DISPATCH_BASE`
- Files changed in the worktree

If there's overlap, it exits non-zero and prints both diffs for manual review. Replaces blind `git apply` with a gated workflow.

### 4. Scope Deconfliction (prevents overlapping assignments)

Captain's plan phase rejects parallel tasks touching the same file in the same wave. If two tasks must touch the same file, one moves to the next wave.

## Reproduction

1. Commit changes to main (e.g., security fixes touching `crates/foo/src/lib.rs`)
2. Dispatch 2+ worktree agents where at least one has scope overlapping the modified files
3. Apply patches from all agents sequentially with `git apply`
4. Observe that the first commit's changes are silently reverted

## Environment

- Fleet plugin version: 0.4.0
- Claude Code: Opus 4.6 (1M context)
- Repo: Rust workspace, 17 crates, 5087 tests
- Session: 4 parallel crew agents (Wave 1) + 5 parallel hunter agents (hunt round)

## Filing

```bash
gh auth login -h github.com
gh issue create --repo <fleet-plugin-repo> \
  --title "Worktree agent dispatch has 3 failure modes causing silent data loss" \
  --body-file .fleet/plans/fleet-worktree-hygiene-issue.md \
  --label bug
```
