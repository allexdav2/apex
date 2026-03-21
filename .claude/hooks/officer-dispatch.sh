#!/bin/bash

# Fleet Officer Dispatch — SubagentStop hook for crew agents
# Detects implementation work in crew agent output and blocks the stop
# with instructions to dispatch matching officers for review.
#
# Input: JSON on stdin with last_assistant_message field (SubagentStop payload)
# Output: JSON block decision if officers matched, silent exit otherwise
#
# Flow:
#   1. If stop_hook_active is true, exit 0 (crew already ran officer reviews)
#   2. Detect implementation work in crew output
#   3. Extract crew name, read sdlc_concerns
#   4. Match concerns against officer triggers
#   5. Output {"decision":"block","reason":"..."} with dispatch instructions

set -euo pipefail

# ── Dependencies ──────────────────────────────────────────────────────────────
command -v jq >/dev/null 2>&1 || { echo '{"error":"officer-dispatch: jq not found"}' >&2; exit 0; }

debug() { [[ -n "${FLEET_DEBUG:-}" ]] && echo "[officer-dispatch] $*" >&2 || true; }

# ── Read hook input ───────────────────────────────────────────────────────────
HOOK_INPUT=$(cat)

# ── Guard: stop_hook_active prevents infinite loops ───────────────────────────
# When a crew is blocked and re-stops after dispatching officers, Claude Code
# sets stop_hook_active: true so hooks don't block again.
STOP_HOOK_ACTIVE=$(printf '%s\n' "$HOOK_INPUT" | jq -r '.stop_hook_active // false' 2>/dev/null || echo "false")
if [[ "$STOP_HOOK_ACTIVE" == "true" ]]; then
  debug "stop_hook_active — allowing stop (officer reviews already completed)"
  exit 0
fi

# ── Extract agent output ──────────────────────────────────────────────────────
AGENT_OUTPUT=$(printf '%s\n' "$HOOK_INPUT" | jq -r '.last_assistant_message // .tool_result // .stop_reason // ""' 2>/dev/null || echo "")

if [[ -z "$AGENT_OUTPUT" ]]; then
  debug "empty output — skipping"
  exit 0
fi

OUTPUT_LOWER=$(printf '%s\n' "$AGENT_OUTPUT" | tr '[:upper:]' '[:lower:]')

# ── Check for implementation work ─────────────────────────────────────────────
# Positive signals: evidence of code changes
IMPL_PATTERNS=(
  "fleet_report"
  "files_changed"
  "commit"
  "branch"
  "created"
  "modified"
  "implemented"
  "refactored"
  "fixed"
)

# Negative signals: non-implementation responses
NON_IMPL_PATTERNS=(
  "no action needed"
  "no changes required"
  "acknowledged"
)

# Check for non-implementation responses first
for pattern in "${NON_IMPL_PATTERNS[@]}"; do
  if printf '%s\n' "$OUTPUT_LOWER" | grep -qF "$pattern"; then
    # Only skip if there are NO positive implementation signals
    HAS_IMPL=false
    for impl_pat in "${IMPL_PATTERNS[@]}"; do
      if printf '%s\n' "$OUTPUT_LOWER" | grep -qF "$impl_pat"; then
        HAS_IMPL=true
        break
      fi
    done
    if [[ "$HAS_IMPL" != "true" ]]; then
      debug "non-implementation response detected — skipping"
      exit 0
    fi
  fi
done

# Check for positive implementation signals
FOUND_IMPL=false
for pattern in "${IMPL_PATTERNS[@]}"; do
  if printf '%s\n' "$OUTPUT_LOWER" | grep -qF "$pattern"; then
    FOUND_IMPL=true
    debug "implementation signal: $pattern"
    break
  fi
done

# Also check for file paths with extensions (e.g., src/foo.rs, hooks/bar.sh)
if [[ "$FOUND_IMPL" != "true" ]]; then
  if printf '%s\n' "$AGENT_OUTPUT" | grep -qE '[a-zA-Z0-9_/]+\.[a-z]{1,5}' 2>/dev/null; then
    FOUND_IMPL=true
    debug "implementation signal: file path with extension"
  fi
fi

if [[ "$FOUND_IMPL" != "true" ]]; then
  debug "no implementation work detected — skipping"
  exit 0
fi

# ── Extract crew name ─────────────────────────────────────────────────────────
CREW_NAME=""

# Try branch name pattern: fleet/crew/<name>/
CREW_NAME=$(printf '%s\n' "$AGENT_OUTPUT" | grep -oE 'fleet/crew/([a-zA-Z0-9_-]+)/' | head -1 | sed 's|fleet/crew/||' | sed 's|/$||' || true)

# Try "crew: <name>" pattern
if [[ -z "$CREW_NAME" ]]; then
  CREW_NAME=$(printf '%s\n' "$AGENT_OUTPUT" | grep -oE '^crew:[[:space:]]*[a-zA-Z0-9_-]+' | head -1 | sed 's/^crew:[[:space:]]*//' || true)
fi

# Try FLEET_REPORT crew field
if [[ -z "$CREW_NAME" ]]; then
  CREW_NAME=$(printf '%s\n' "$AGENT_OUTPUT" | sed -n '/FLEET_REPORT/,/-->/p' | grep -E '^crew:' | head -1 | sed 's/^crew:[[:space:]]*//' | tr -d '[:space:]' || true)
fi

if [[ -z "$CREW_NAME" ]]; then
  debug "could not determine crew name — skipping"
  exit 0
fi

debug "crew: $CREW_NAME"

# ── Read crew config ──────────────────────────────────────────────────────────
CREW_FILE=".fleet/crews/${CREW_NAME}.yaml"
if [[ ! -f "$CREW_FILE" ]]; then
  debug "crew config not found: $CREW_FILE — skipping"
  exit 0
fi

# Extract sdlc_concerns list — read items until next non-list-item line
CONCERNS_LINE=$(grep -n '^sdlc_concerns:' "$CREW_FILE" 2>/dev/null | head -1 | cut -d: -f1)

CONCERN_LIST=""
if [[ -n "$CONCERNS_LINE" ]]; then
  FIRST_LINE=$(sed -n "${CONCERNS_LINE}p" "$CREW_FILE")
  if printf '%s' "$FIRST_LINE" | grep -q '\['; then
    # Inline format: sdlc_concerns: [qa, security, plugin]
    CONCERN_LIST=$(printf '%s' "$FIRST_LINE" | sed 's/sdlc_concerns:[[:space:]]*//' | tr -d '[]' | tr ',' '\n' | sed 's/^[[:space:]]*//' | sed 's/[[:space:]]*$//' | grep -v '^$')
  else
    # Block format: sdlc_concerns:\n  - qa\n  - security
    # Read subsequent lines that start with whitespace + dash
    CONCERN_LIST=$(tail -n +"$((CONCERNS_LINE + 1))" "$CREW_FILE" | while IFS= read -r cline; do
      if printf '%s' "$cline" | grep -qE '^[[:space:]]*-[[:space:]]'; then
        printf '%s\n' "$cline" | sed 's/^[[:space:]]*-[[:space:]]*//' | sed 's/[[:space:]]*$//'
      else
        break
      fi
    done)
  fi
fi

if [[ -z "$CONCERN_LIST" ]]; then
  debug "no sdlc_concerns for crew $CREW_NAME — skipping"
  exit 0
fi

debug "concerns: $(printf '%s\n' "$CONCERN_LIST" | tr '\n' ',' | sed 's/,$//')"

# ── Match against officers ────────────────────────────────────────────────────
OFFICERS_DIR=".fleet/officers"
if [[ ! -d "$OFFICERS_DIR" ]]; then
  debug "no officers directory — skipping"
  exit 0
fi

MATCHED_OFFICERS=""

for OFFICER_FILE in "$OFFICERS_DIR"/*.yaml; do
  [[ -f "$OFFICER_FILE" ]] || continue

  OFFICER_NAME=$(basename "$OFFICER_FILE" .yaml)

  # Skip manual-dispatch-only officers (tier: manual)
  OFFICER_TIER=$(grep -E '^tier:' "$OFFICER_FILE" 2>/dev/null | head -1 | sed 's/tier:[[:space:]]*//' | tr -d '[:space:]')
  if [[ "$OFFICER_TIER" == "manual" ]]; then
    debug "officer $OFFICER_NAME is tier:manual — skipping auto-dispatch"
    continue
  fi

  # Extract triggers list
  TRIGGERS_LINE=$(grep -n '^triggers:' "$OFFICER_FILE" 2>/dev/null | head -1 | cut -d: -f1)
  TRIGGER_LIST=""
  if [[ -n "$TRIGGERS_LINE" ]]; then
    FIRST_TRIG=$(sed -n "${TRIGGERS_LINE}p" "$OFFICER_FILE")
    if printf '%s' "$FIRST_TRIG" | grep -q '\['; then
      TRIGGER_LIST=$(printf '%s' "$FIRST_TRIG" | sed 's/triggers:[[:space:]]*//' | tr -d '[]' | tr ',' '\n' | sed 's/^[[:space:]]*//' | sed 's/[[:space:]]*$//' | grep -v '^$')
    else
      TRIGGER_LIST=$(tail -n +"$((TRIGGERS_LINE + 1))" "$OFFICER_FILE" | while IFS= read -r tline; do
        if printf '%s' "$tline" | grep -qE '^[[:space:]]*-[[:space:]]'; then
          printf '%s\n' "$tline" | sed 's/^[[:space:]]*-[[:space:]]*//' | sed 's/[[:space:]]*$//'
        else
          break
        fi
      done)
    fi
  fi

  # Check for matches between concerns and triggers
  while IFS= read -r concern; do
    [[ -z "$concern" ]] && continue
    while IFS= read -r trigger; do
      [[ -z "$trigger" ]] && continue
      if [[ "$concern" == "$trigger" ]]; then
        if [[ -z "$MATCHED_OFFICERS" ]]; then
          MATCHED_OFFICERS="$OFFICER_NAME"
        else
          MATCHED_OFFICERS="$MATCHED_OFFICERS,$OFFICER_NAME"
        fi
        debug "match: concern=$concern trigger=$trigger officer=$OFFICER_NAME"
        break 2
      fi
    done <<< "$TRIGGER_LIST"
  done <<< "$CONCERN_LIST"
done

if [[ -z "$MATCHED_OFFICERS" ]]; then
  debug "no officer matches — skipping"
  exit 0
fi

debug "dispatching officers: $MATCHED_OFFICERS for crew: $CREW_NAME"

# ── Block stop with officer dispatch instructions ─────────────────────────────
# The crew agent receives this as feedback and dispatches officers itself.
# On its next stop, stop_hook_active will be true and we exit early above.
OFFICER_LIST=$(printf '%s\n' "$MATCHED_OFFICERS" | tr ',' '\n' | while IFS= read -r oname; do
  [[ -z "$oname" ]] && continue
  printf '  - %s (.fleet/officers/%s.yaml)\n' "$oname" "$oname"
done)

REASON="Fleet SubagentStop hook — officer dispatch required. This overrides the 'do not dispatch agents' constraint per the Officer Auto-Review exception in your instructions.

You MUST dispatch officer reviews before completing. Officers matched by your sdlc_concerns:
${OFFICER_LIST}

For each officer above:
1. Read .fleet/officers/<name>.yaml for their full config
2. Dispatch via Agent tool with subagent_type: 'fleet:officer' and this prompt:
   'You are the <officer title> (<officer name>). Your config: <full officer YAML>
   ---
   Auto-dispatched after ${CREW_NAME}-crew completed work.
   Crew config: <contents of .fleet/crews/${CREW_NAME}.yaml>
   Crew FLEET_REPORT: <your FLEET_REPORT from above>
   ---
   Review the crew changes against your review_checklist and expertise. Return structured findings with confidence-based filtering (≥80 in Findings, <80 in Long Tail).'
3. Collect their findings

After ALL officer reviews complete, append a summary of their findings to your output, then stop."

jq -n --arg reason "$REASON" '{
  "decision": "block",
  "reason": $reason
}'
exit 0
