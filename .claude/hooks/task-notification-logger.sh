#!/bin/bash

# Fleet Task Notification Logger — TaskCompleted hook for Agent Teams
# Extracts FLEET_NOTIFICATION blocks from completed task output and
# persists them as .fleet/changes/ entries.
#
# Input: JSON on stdin with TaskCompleted payload
# Output: none (silent)

set -euo pipefail

# ── Dependencies ──────────────────────────────────────────────────────────────
command -v jq >/dev/null 2>&1 || exit 0

debug() { [[ -n "${FLEET_DEBUG:-}" ]] && echo "[task-notification-logger] $*" >&2 || true; }

# ── Guard: .fleet/ must exist ─────────────────────────────────────────────────
if [[ ! -d .fleet ]]; then
  debug ".fleet/ directory not found — silent exit"
  exit 0
fi

# ── Read hook input ───────────────────────────────────────────────────────────
HOOK_INPUT=$(cat)

# ── Extract task output (try multiple field names) ────────────────────────────
TASK_OUTPUT=$(printf '%s\n' "$HOOK_INPUT" | jq -r '.task_output // .result // .last_message // ""' 2>/dev/null || echo "")

if [[ -z "$TASK_OUTPUT" ]]; then
  debug "empty task output — nothing to log"
  exit 0
fi

# ── Check for FLEET_NOTIFICATION blocks ───────────────────────────────────────
if ! printf '%s\n' "$TASK_OUTPUT" | grep -q '<!-- FLEET_NOTIFICATION'; then
  debug "no FLEET_NOTIFICATION blocks found"
  exit 0
fi

# ── Parse and process each notification block ─────────────────────────────────
# Extract all notification blocks (content between <!-- FLEET_NOTIFICATION and -->)
BLOCKS=$(printf '%s\n' "$TASK_OUTPUT" | sed -n '/<!-- FLEET_NOTIFICATION/,/-->/p')

if [[ -z "$BLOCKS" ]]; then
  debug "notification markers found but extraction failed"
  exit 0
fi

TODAY=$(date +%Y-%m-%d)

# Process blocks one at a time by splitting on the opening marker
BLOCK_NUM=0
CURRENT_BLOCK=""
printf '%s\n' "$BLOCKS" | while IFS= read -r line; do
  # Accumulate lines into a block
  if printf '%s' "$line" | grep -q '<!-- FLEET_NOTIFICATION'; then
    BLOCK_NUM=$((BLOCK_NUM + 1))
    CURRENT_BLOCK=""
    continue
  fi

  if printf '%s' "$line" | grep -q '^-->'; then
    # End of block — process it
    if [[ -z "${CURRENT_BLOCK:-}" ]]; then
      continue
    fi

    # Extract fields from the accumulated block
    CREW=$(printf '%s\n' "$CURRENT_BLOCK" | grep -E '^crew:' | head -1 | sed 's/^crew:[[:space:]]*//' | tr -d '[:space:]')
    AT_COMMIT=$(printf '%s\n' "$CURRENT_BLOCK" | grep -E '^at_commit:' | head -1 | sed 's/^at_commit:[[:space:]]*//' | tr -d '[:space:]')
    PARTNERS=$(printf '%s\n' "$CURRENT_BLOCK" | grep -E '^affected_partners:' | head -1 | sed 's/^affected_partners:[[:space:]]*//' | sed 's/[][]//g' | tr -d '[:space:]')
    SEVERITY=$(printf '%s\n' "$CURRENT_BLOCK" | grep -E '^severity:' | head -1 | sed 's/^severity:[[:space:]]*//' | tr -d '[:space:]')
    SUMMARY=$(printf '%s\n' "$CURRENT_BLOCK" | grep -E '^summary:' | head -1 | sed 's/^summary:[[:space:]]*//' | sed 's/^[[:space:]]*//' | sed 's/[[:space:]]*$//')

    # Extract detail (everything after "detail: |" or "detail:" to end of block)
    DETAIL=$(printf '%s\n' "$CURRENT_BLOCK" | sed -n '/^detail:/,$ p' | tail -n +2)
    # Sanitize detail — strip lines that are exactly ---
    DETAIL=$(printf '%s\n' "$DETAIL" | grep -v '^---$' || true)
    # Strip leading indentation (2 spaces typical in YAML blocks)
    DETAIL=$(printf '%s\n' "$DETAIL" | sed 's/^  //')

    if [[ -z "$CREW" ]] || [[ -z "$SEVERITY" ]]; then
      debug "block $BLOCK_NUM missing required fields (crew=$CREW, severity=$SEVERITY)"
      continue
    fi

    # Generate slug from summary
    SLUG=$(printf '%s' "${SUMMARY:-notification}" | tr '[:upper:]' '[:lower:]' | tr -cs '[:alnum:]' '-' | head -c 40 | sed 's/-$//')

    # Ensure .fleet/changes/ exists
    mkdir -p .fleet/changes

    # Write changelog entry
    CHANGELOG_FILE=".fleet/changes/${TODAY}-${CREW}-${SLUG}.md"
    cat > "$CHANGELOG_FILE" <<ENTRY_EOF
---
date: ${TODAY}
crew: ${CREW}
at_commit: ${AT_COMMIT}
affected_partners: [${PARTNERS}]
severity: ${SEVERITY}
acknowledged_by: []
---

${SUMMARY}

${DETAIL}
ENTRY_EOF

    debug "wrote changelog: $CHANGELOG_FILE"

    continue
  fi

  # Accumulate line into current block
  CURRENT_BLOCK="${CURRENT_BLOCK:-}
${line}"
done

exit 0
