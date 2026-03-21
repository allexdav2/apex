#!/bin/bash
# Fleet SessionEnd cleanup — lightweight maintenance on session exit
# Must complete within 1.5 seconds (SessionEnd timeout)
#
# Input: JSON on stdin with session_id, cwd fields
# Output: none (silent)

set -uo pipefail

# ── Read hook input ───────────────────────────────────────────────────────────
HOOK_INPUT=$(cat)
SESSION_ID=$(printf '%s' "$HOOK_INPUT" | jq -r '.session_id // ""' 2>/dev/null || echo "")
CWD=$(printf '%s' "$HOOK_INPUT" | jq -r '.cwd // ""' 2>/dev/null || echo "")

# ── 1. Remove Ralph counter file for this session ────────────────────────────
if [[ -n "$SESSION_ID" ]]; then
  rm -f "${TMPDIR:-/tmp}/ralph-blocks-${SESSION_ID}" 2>/dev/null
fi

# ── 2. Remove stale GH cache (>24h old) ──────────────────────────────────────
GH_CACHE=~/.claude/fleet-gh-cache.json
if [[ -f "$GH_CACHE" ]] && command -v jq >/dev/null 2>&1; then
  CACHE_TS=$(jq -r '.ts // 0' "$GH_CACHE" 2>/dev/null || echo "0")
  NOW=$(date +%s)
  AGE=$(( NOW - CACHE_TS ))
  if (( AGE > 86400 )); then
    rm -f "$GH_CACHE" 2>/dev/null
  fi
fi

# ── 3. Remove stale hygiene cache ────────────────────────────────────────────
HYG_CACHE=~/.claude/fleet-hygiene-cache.json
if [[ -f "$HYG_CACHE" ]]; then
  rm -f "$HYG_CACHE" 2>/dev/null
fi

# ── 4. Truncate oversized tool usage log (>10000 lines → keep last 5000) ─────
if [[ -n "$CWD" ]] && [[ -f "$CWD/.fleet/tool-usage.jsonl" ]]; then
  LINE_COUNT=$(wc -l < "$CWD/.fleet/tool-usage.jsonl" | tr -d ' ')
  if (( LINE_COUNT > 10000 )); then
    TAIL_FILE=$(mktemp "${CWD}/.fleet/tool-usage.jsonl.XXXXXX" 2>/dev/null || echo "")
    if [[ -n "$TAIL_FILE" ]]; then
      tail -5000 "$CWD/.fleet/tool-usage.jsonl" > "$TAIL_FILE" 2>/dev/null && \
        mv "$TAIL_FILE" "$CWD/.fleet/tool-usage.jsonl" 2>/dev/null || \
        rm -f "$TAIL_FILE"
    fi
  fi
fi

exit 0
