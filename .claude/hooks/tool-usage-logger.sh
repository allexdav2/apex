#!/bin/bash

# Fleet Tool Usage Logger — PostToolUse hook
# Logs which tools each agent type actually uses to .fleet/tool-usage.jsonl.
# Data is consumed by the tool-usage-analyzer bridge agent to recommend
# allowed-tools tightening or expansion.
#
# Input: JSON on stdin with tool_name, session_id, agent_type fields
# Output: none (silent — PostToolUse hooks cannot block)

set -euo pipefail

# ── Skip if no fleet config (e.g., inside worktrees without .fleet/) ──────────
[[ -d ".fleet" ]] || exit 0

# ── Dependencies ──────────────────────────────────────────────────────────────
command -v jq >/dev/null 2>&1 || exit 0

# ── Read hook input ───────────────────────────────────────────────────────────
HOOK_INPUT=$(cat)

TOOL_NAME=$(printf '%s\n' "$HOOK_INPUT" | jq -r '.tool_name // ""' 2>/dev/null || echo "")
[[ -z "$TOOL_NAME" ]] && exit 0

SESSION_ID=$(printf '%s\n' "$HOOK_INPUT" | jq -r '.session_id // ""' 2>/dev/null || echo "")
AGENT_TYPE=$(printf '%s\n' "$HOOK_INPUT" | jq -r '.agent_type // "main"' 2>/dev/null || echo "main")
TIMESTAMP=$(date -u +%Y-%m-%dT%H:%M:%SZ)

# ── Append to tool usage log ─────────────────────────────────────────────────
# Only log tool_name and metadata — never log tool_input/tool_response (privacy)
USAGE_FILE=".fleet/tool-usage.jsonl"

jq -n -c \
  --arg ts "$TIMESTAMP" \
  --arg tool "$TOOL_NAME" \
  --arg agent "$AGENT_TYPE" \
  --arg session "$SESSION_ID" \
  '{ts: $ts, tool: $tool, agent: $agent, session: $session}' >> "$USAGE_FILE"

exit 0
