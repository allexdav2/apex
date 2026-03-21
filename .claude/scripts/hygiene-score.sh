#!/bin/bash
# Fleet hygiene score — compute 0-100 from local state only
# Must complete in <1 second (no remote calls)
# Output: single integer to stdout

set -euo pipefail

CWD="${1:-.}"
SCORE=100

# ── Worktrees: orphaned (branch merged or missing) ───────────────────────────
# Weight: 20 pts
WORKTREE_TOTAL=$(git -C "$CWD" worktree list 2>/dev/null | tail -n +2 | wc -l | tr -d ' ')
MERGED_COUNT=$(git -C "$CWD" branch --merged master --list 'fleet/*' 2>/dev/null | wc -l | tr -d ' ')
ORPHANED=$(( WORKTREE_TOTAL > MERGED_COUNT ? MERGED_COUNT : WORKTREE_TOTAL ))
if (( ORPHANED > 5 )); then
  SCORE=$(( SCORE - 20 ))
elif (( ORPHANED > 0 )); then
  SCORE=$(( SCORE - 10 ))
fi

# ── Branches: stale local fleet/* merged to master ───────────────────────────
# Weight: 15 pts
STALE_BRANCHES=$(git -C "$CWD" branch --merged master --list 'fleet/*' 2>/dev/null | wc -l | tr -d ' ')
if (( STALE_BRANCHES > 10 )); then
  SCORE=$(( SCORE - 15 ))
elif (( STALE_BRANCHES > 0 )); then
  SCORE=$(( SCORE - 8 ))
fi

# ── Tool usage log size ──────────────────────────────────────────────────────
# Weight: 10 pts
TOOL_LOG="$CWD/.fleet/tool-usage.jsonl"
if [[ -f "$TOOL_LOG" ]]; then
  TOOL_LINES=$(wc -l < "$TOOL_LOG" | tr -d ' ')
  if (( TOOL_LINES > 10000 )); then
    SCORE=$(( SCORE - 10 ))
  elif (( TOOL_LINES > 5000 )); then
    SCORE=$(( SCORE - 5 ))
  fi
fi

# ── Plans: done/superseded not archived ──────────────────────────────────────
# Weight: 10 pts
STALE_PLANS=0
if [[ -d "$CWD/.fleet/plans" ]]; then
  STALE_PLANS=$(grep -rl 'status: DONE\|status: SUPERSEDED' "$CWD/.fleet/plans/"*.md 2>/dev/null | wc -l | tr -d ' ' || echo "0")
fi
if (( STALE_PLANS > 5 )); then
  SCORE=$(( SCORE - 10 ))
elif (( STALE_PLANS > 0 )); then
  SCORE=$(( SCORE - 5 ))
fi

# ── Changelog: stale acknowledged entries ────────────────────────────────────
# Weight: 10 pts
STALE_CHANGELOG=0
if [[ -d "$CWD/.fleet/changes" ]]; then
  RETENTION=30
  if [[ -f "$CWD/.fleet/bridge.yaml" ]]; then
    RETENTION=$(grep -E '^changelog_retention_days:' "$CWD/.fleet/bridge.yaml" 2>/dev/null | sed 's/[^0-9]//g' || echo "30")
    [[ -z "$RETENTION" ]] && RETENTION=30
  fi
  CUTOFF=$(date -v-${RETENTION}d +%Y-%m-%d 2>/dev/null || date -d "-${RETENTION} days" +%Y-%m-%d 2>/dev/null || echo "")
  if [[ -n "$CUTOFF" ]]; then
    for f in "$CWD/.fleet/changes/"*.md; do
      [[ -f "$f" ]] || continue
      FILE_DATE=$(basename "$f" | grep -oE '^[0-9]{4}-[0-9]{2}-[0-9]{2}' || echo "")
      if [[ -n "$FILE_DATE" ]] && [[ "$FILE_DATE" < "$CUTOFF" ]]; then
        if grep -q 'acknowledged_by: \[' "$f" 2>/dev/null && ! grep -q 'acknowledged_by: \[\]' "$f" 2>/dev/null; then
          STALE_CHANGELOG=$((STALE_CHANGELOG + 1))
        fi
      fi
    done
  fi
fi
if (( STALE_CHANGELOG > 5 )); then
  SCORE=$(( SCORE - 10 ))
elif (( STALE_CHANGELOG > 0 )); then
  SCORE=$(( SCORE - 5 ))
fi

# ── Unacknowledged notifications ─────────────────────────────────────────────
# Weight: 15 pts
UNACKED=0
if [[ -d "$CWD/.fleet/changes" ]]; then
  for f in "$CWD/.fleet/changes/"*.md; do
    [[ -f "$f" ]] || continue
    grep -q 'acknowledged_by: \[\]' "$f" 2>/dev/null && UNACKED=$((UNACKED + 1))
  done
fi
if (( UNACKED > 3 )); then
  SCORE=$(( SCORE - 15 ))
elif (( UNACKED > 0 )); then
  SCORE=$(( SCORE - 8 ))
fi

# ── Officer catalog drift ───────────────────────────────────────────────────
# Weight: 10 pts
DRIFT=0
CATALOG=""
for candidate in "$CWD/marketplace/catalog.json" "${CLAUDE_PLUGIN_ROOT:-}/marketplace/catalog.json"; do
  if [[ -f "$candidate" ]]; then
    CATALOG="$candidate"
    break
  fi
done
if [[ -n "$CATALOG" ]] && [[ -d "$CWD/.fleet/officers" ]] && command -v jq >/dev/null 2>&1; then
  CATALOG_NAMES=$(jq -r '.packages[].name' "$CATALOG" 2>/dev/null | sort)
  for f in "$CWD/.fleet/officers/"*.yaml; do
    [[ -f "$f" ]] || continue
    ONAME=$(basename "$f" .yaml)
    if ! echo "$CATALOG_NAMES" | grep -qx "$ONAME"; then
      DRIFT=$((DRIFT + 1))
    fi
  done
fi
if (( DRIFT > 3 )); then
  SCORE=$(( SCORE - 10 ))
elif (( DRIFT > 0 )); then
  SCORE=$(( SCORE - 5 ))
fi

# ── Cache freshness ──────────────────────────────────────────────────────────
# Weight: 5 pts
GH_CACHE=~/.claude/fleet-gh-cache.json
if [[ -f "$GH_CACHE" ]] && command -v jq >/dev/null 2>&1; then
  CACHE_TS=$(jq -r '.ts // 0' "$GH_CACHE" 2>/dev/null || echo "0")
  NOW=$(date +%s)
  AGE=$(( NOW - CACHE_TS ))
  if (( AGE > 86400 )); then
    SCORE=$(( SCORE - 5 ))
  elif (( AGE > 3600 )); then
    SCORE=$(( SCORE - 2 ))
  fi
fi

# ── Clamp and output ─────────────────────────────────────────────────────────
(( SCORE < 0 )) && SCORE=0
echo "$SCORE"
