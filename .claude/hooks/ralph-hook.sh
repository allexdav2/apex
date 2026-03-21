#!/bin/bash

# Fleet Ralph — SubagentStop hook for crew/officer agents
# Two modes:
#   1. Permanent — ralph_enabled: true in .fleet/bridge.yaml
#   2. Session   — .fleet/ralph-session.local with matching session_id
#
# Input: JSON on stdin with last_assistant_message field (SubagentStop payload)
# Safety: max 5 consecutive blocks per agent, destructive op blocklist,
# debug logging via FLEET_DEBUG env var.

set -euo pipefail

# ── Dependencies ──────────────────────────────────────────────────────────────
command -v jq >/dev/null 2>&1 || { echo '{"error":"ralph: jq not found"}' >&2; exit 0; }

debug() { [[ -n "${FLEET_DEBUG:-}" ]] && echo "[ralph] $*" >&2 || true; }

# ── Read hook input ───────────────────────────────────────────────────────────
HOOK_INPUT=$(cat)
HOOK_SESSION=$(echo "$HOOK_INPUT" | jq -r '.session_id // ""' 2>/dev/null || echo "")

# ── Guard: skip on re-stop after hook block (e.g. officer-dispatch cycle) ─────
# Prevents ralph from accidentally blocking the crew's second stop if officer
# findings happen to contain ralph trigger phrases like "should I proceed".
STOP_HOOK_ACTIVE=$(printf '%s\n' "$HOOK_INPUT" | jq -r '.stop_hook_active // false' 2>/dev/null || echo "false")
if [[ "$STOP_HOOK_ACTIVE" == "true" ]]; then
  COUNTER_FILE="${TMPDIR:-/tmp}/ralph-blocks-${HOOK_SESSION:-unknown}"
  rm -f "$COUNTER_FILE"
  debug "stop_hook_active — allowing stop, resetting counter"
  exit 0
fi

# ── Mode 1: Permanent (bridge.yaml) ──────────────────────────────────────────
PERMANENT=false
BRIDGE_FILE=".fleet/bridge.yaml"
if [[ -f "$BRIDGE_FILE" ]]; then
  RALPH_ENABLED=$(grep -E '^ralph_enabled:\s*' "$BRIDGE_FILE" 2>/dev/null | head -1 | sed 's/ralph_enabled:\s*//' | tr -d '[:space:]' || echo "false")
  [[ "$RALPH_ENABLED" == "true" ]] && PERMANENT=true
fi

# ── Mode 2: Session (.fleet/ralph-session.local) ─────────────────────────────
SESSION_ACTIVE=false
SESSION_FILE=".fleet/ralph-session.local"
if [[ -f "$SESSION_FILE" ]] && [[ -n "$HOOK_SESSION" ]]; then
  SESSION_ID=$(grep -E '^session_id:\s*' "$SESSION_FILE" 2>/dev/null | head -1 | sed 's/session_id:\s*//' | tr -d '[:space:]' || echo "")
  [[ "$SESSION_ID" == "$HOOK_SESSION" ]] && SESSION_ACTIVE=true
fi

# Neither mode active → allow stop
if [[ "$PERMANENT" != "true" ]] && [[ "$SESSION_ACTIVE" != "true" ]]; then
  debug "inactive — allowing stop"
  exit 0
fi

# ── Consecutive block counter (max 5) ─────────────────────────────────────────
COUNTER_FILE="${TMPDIR:-/tmp}/ralph-blocks-${HOOK_SESSION:-unknown}"
BLOCK_COUNT=0
if [[ -f "$COUNTER_FILE" ]]; then
  BLOCK_COUNT=$(cat "$COUNTER_FILE" 2>/dev/null || echo "0")
  [[ ! "$BLOCK_COUNT" =~ ^[0-9]+$ ]] && BLOCK_COUNT=0
fi

if (( BLOCK_COUNT >= 5 )); then
  debug "max blocks reached ($BLOCK_COUNT) — allowing stop, resetting counter"
  rm -f "$COUNTER_FILE"
  exit 0
fi

# ── Extract agent output ──────────────────────────────────────────────────────
AGENT_OUTPUT=$(echo "$HOOK_INPUT" | jq -r '.last_assistant_message // .tool_result // .stop_reason // ""' 2>/dev/null || echo "")

if [[ -z "$AGENT_OUTPUT" ]]; then
  debug "empty output — allowing stop"
  exit 0
fi

TAIL=$(echo "$AGENT_OUTPUT" | tail -c 500)
TAIL_LOWER=$(echo "$TAIL" | tr '[:upper:]' '[:lower:]' | tr -s '[:space:]' ' ')

# ── Destructive operation blocklist ───────────────────────────────────────────
# NEVER auto-approve these even if they match a continue pattern
DESTRUCTIVE_PATTERNS=(
  "rm -rf"
  "git push --force"
  "git push -f"
  "git reset --hard"
  "drop table"
  "drop database"
  "delete from"
  "force push"
  "git clean -fd"
  "git checkout -- ."
  "git restore ."
)

for pattern in "${DESTRUCTIVE_PATTERNS[@]}"; do
  if echo "$TAIL_LOWER" | grep -qF "$pattern"; then
    debug "destructive pattern detected: $pattern — allowing stop (human decision needed)"
    rm -f "$COUNTER_FILE"
    exit 0
  fi
done

# ── Continue patterns ─────────────────────────────────────────────────────────
CONTINUE_PATTERNS=(
  "should i continue"
  "shall i continue"
  "shall i proceed"
  "should i proceed"
  "want me to continue"
  "want me to proceed"
  "would you like me to continue"
  "would you like me to proceed"
  "would you like me to go ahead"
  "do you want me to"
  "should i go ahead"
  "shall i go ahead"
  "waiting for confirmation"
  "awaiting confirmation"
  "approve to continue"
  "approve to proceed"
  "want me to implement"
  "should i implement"
  "shall i implement"
  "want me to fix"
  "should i fix"
  "shall i fix"
  "want me to make"
  "should i make these changes"
  "shall i make these changes"
)

# Removed: "let me know if", "let me know whether", "ready to proceed"
# These match completion statements, not permission-seeking questions.

for pattern in "${CONTINUE_PATTERNS[@]}"; do
  if echo "$TAIL_LOWER" | grep -qF "$pattern"; then
    # Increment block counter
    BLOCK_COUNT=$((BLOCK_COUNT + 1))
    echo "$BLOCK_COUNT" > "$COUNTER_FILE"

    if [[ "$PERMANENT" == "true" ]]; then
      MODE_TAG="permanent"
    else
      MODE_TAG="session"
    fi

    debug "pattern matched: '$pattern' — blocking stop ($BLOCK_COUNT/5) [$MODE_TAG]"

    jq -n --arg mode "$MODE_TAG" --argjson count "$BLOCK_COUNT" '{
      "decision": "block",
      "reason": ("Yes, continue. Proceed with the implementation. [Ralph " + $mode + " " + ($count | tostring) + "/5]")
    }'
    exit 0
  fi
done

# No pattern matched — reset counter and allow stop
debug "no pattern matched — allowing stop"
rm -f "$COUNTER_FILE"
exit 0
