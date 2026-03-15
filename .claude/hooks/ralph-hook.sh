#!/bin/bash

# Fleet Ralph — SubagentStop hook for crew/officer agents
# Two modes:
#   1. Permanent — ralph_enabled: true in .fleet/bridge.yaml (persists across sessions)
#   2. Session   — .fleet/ralph-session.local file with matching session_id (dies with session)
#
# "do with Ralph" activates session mode. "/fleet ralph on" activates permanent mode.
#
# Returns {"decision": "block", "reason": "yes, continue"} when the agent's
# last output asks for permission. Otherwise exits 0 (allow stop).

set -euo pipefail

# Read hook input first — we need session_id for session mode check
HOOK_INPUT=$(cat)
HOOK_SESSION=$(echo "$HOOK_INPUT" | jq -r '.session_id // ""' 2>/dev/null || echo "")

# --- Mode 1: Permanent (bridge.yaml) ---
PERMANENT=false
BRIDGE_FILE=".fleet/bridge.yaml"
if [[ -f "$BRIDGE_FILE" ]]; then
  RALPH_ENABLED=$(grep -E '^ralph_enabled:\s*' "$BRIDGE_FILE" 2>/dev/null | sed 's/ralph_enabled:\s*//' | tr -d '[:space:]' || echo "false")
  if [[ "$RALPH_ENABLED" == "true" ]]; then
    PERMANENT=true
  fi
fi

# --- Mode 2: Session (.fleet/ralph-session.local) ---
SESSION_ACTIVE=false
SESSION_FILE=".fleet/ralph-session.local"
if [[ -f "$SESSION_FILE" ]] && [[ -n "$HOOK_SESSION" ]]; then
  SESSION_ID=$(grep -E '^session_id:\s*' "$SESSION_FILE" 2>/dev/null | sed 's/session_id:\s*//' | tr -d '[:space:]' || echo "")
  if [[ "$SESSION_ID" == "$HOOK_SESSION" ]]; then
    SESSION_ACTIVE=true
  fi
fi

# If neither mode is active, allow stop
if [[ "$PERMANENT" != "true" ]] && [[ "$SESSION_ACTIVE" != "true" ]]; then
  exit 0
fi

# Extract the agent's last text output
AGENT_OUTPUT=$(echo "$HOOK_INPUT" | jq -r '
  .tool_result // .stop_reason // ""
' 2>/dev/null || echo "")

if [[ -z "$AGENT_OUTPUT" ]]; then
  exit 0
fi

# Get the last 500 chars — questions are at the end
TAIL=$(echo "$AGENT_OUTPUT" | tail -c 500)

# Normalize: lowercase, collapse whitespace
TAIL_LOWER=$(echo "$TAIL" | tr '[:upper:]' '[:lower:]' | tr -s '[:space:]' ' ')

# Patterns that indicate permission-seeking, not completion
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
  "ready to proceed"
  "let me know if"
  "let me know whether"
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

for pattern in "${CONTINUE_PATTERNS[@]}"; do
  if echo "$TAIL_LOWER" | grep -qF "$pattern"; then
    if [[ "$PERMANENT" == "true" ]]; then
      MODE_TAG="permanent"
    else
      MODE_TAG="session"
    fi
    jq -n --arg mode "$MODE_TAG" '{
      "decision": "block",
      "reason": ("Yes, continue. Proceed with the implementation. [Ralph " + $mode + "]")
    }'
    exit 0
  fi
done

# No question detected — allow normal stop
exit 0
