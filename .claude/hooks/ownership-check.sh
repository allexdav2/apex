#!/usr/bin/env bash
# PostToolUse hook: check file ownership against Fleet crews
# Exits silently if .fleet/ doesn't exist (e.g., inside worktrees)

FLEET_DIR=".fleet/crews"

# Silent exit if no Fleet config
if [ ! -d "$FLEET_DIR" ]; then
  exit 0
fi

# Fleet exists — output prompt for LLM to check ownership
cat <<'PROMPT'
Check if the file that was just written/edited falls within any crew's owned paths. Read .fleet/crews/*.yaml to get all crews' paths. If the file matches a crew AND the change could affect partner crews, suggest: 'This file is owned by <crew-name>-crew. Consider notifying partners: <partner-list>.' If the file is not owned by any crew, say: 'This file is not covered by any Fleet crew.' Be brief — one line.
PROMPT
