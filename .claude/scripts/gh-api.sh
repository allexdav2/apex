#!/bin/bash
# Fleet GitHub API helper — tries gh CLI first, falls back to curl + token
#
# Usage:
#   gh-api.sh GET /repos/owner/repo/pulls
#   gh-api.sh POST /repos/owner/repo/releases '{"tag_name":"v1.0"}'
#
# Auth chain:
#   1. gh api (if gh installed and authenticated)
#   2. curl + GITHUB_TOKEN env var
#   3. curl + GH_TOKEN env var
#   4. curl + git credential helper
#   5. Unauthenticated (rate-limited, public repos only)

set -euo pipefail

METHOD="${1:-GET}"
ENDPOINT="${2:-}"
BODY="${3:-}"

if [[ -z "$ENDPOINT" ]]; then
  echo '{"error":"usage: gh-api.sh METHOD /endpoint [json-body]"}' >&2
  exit 1
fi

API_BASE="https://api.github.com"

# ── Strategy 1: gh CLI ────────────────────────────────────────────────────────
if command -v gh >/dev/null 2>&1; then
  if [[ -n "$BODY" ]]; then
    RESULT=$(gh api "$ENDPOINT" --method "$METHOD" --input - <<< "$BODY" 2>/dev/null) && { echo "$RESULT"; exit 0; }
  else
    RESULT=$(gh api "$ENDPOINT" --method "$METHOD" 2>/dev/null) && { echo "$RESULT"; exit 0; }
  fi
fi

# ── Resolve token ─────────────────────────────────────────────────────────────
TOKEN=""

# Strategy 2: GITHUB_TOKEN env
if [[ -n "${GITHUB_TOKEN:-}" ]]; then
  TOKEN="$GITHUB_TOKEN"
# Strategy 3: GH_TOKEN env (used by gh CLI)
elif [[ -n "${GH_TOKEN:-}" ]]; then
  TOKEN="$GH_TOKEN"
# Strategy 4: git credential helper
elif command -v git >/dev/null 2>&1; then
  TOKEN=$(printf 'protocol=https\nhost=github.com\n' | git credential fill 2>/dev/null | grep '^password=' | head -1 | sed 's/^password=//' || true)
fi

# ── curl request ──────────────────────────────────────────────────────────────
CURL_ARGS=(
  -s -L
  --max-time 10
  -H "Accept: application/vnd.github+json"
  -H "X-GitHub-Api-Version: 2022-11-28"
)

if [[ -n "$TOKEN" ]]; then
  CURL_ARGS+=(-H "Authorization: Bearer $TOKEN")
fi

URL="${API_BASE}${ENDPOINT}"

if [[ "$METHOD" == "GET" ]]; then
  curl "${CURL_ARGS[@]}" "$URL" 2>/dev/null
elif [[ -n "$BODY" ]]; then
  curl "${CURL_ARGS[@]}" -X "$METHOD" -H "Content-Type: application/json" -d "$BODY" "$URL" 2>/dev/null
else
  curl "${CURL_ARGS[@]}" -X "$METHOD" "$URL" 2>/dev/null
fi
