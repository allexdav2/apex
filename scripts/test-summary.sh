#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/test-summary.sh [cargo test args...]
# Example: ./scripts/test-summary.sh -p apex-lang -- cpp
#
# Friendly test runner that replaces cargo's confusing multi-binary output
# with a clean, color-coded summary.

export PATH="$HOME/.cargo/bin:$PATH"

# ── Colors (disabled if piped) ──────────────────────────────────────
if [ -t 1 ]; then
  GREEN='\033[32m' RED='\033[31m' YELLOW='\033[33m'
  DIM='\033[90m' BOLD='\033[1m' RESET='\033[0m'
else
  GREEN='' RED='' YELLOW='' DIM='' BOLD='' RESET=''
fi

# ── Parse args for display ──────────────────────────────────────────
CRATE="" FILTER="" PREV="" SEEN_SEP=false
for arg in "$@"; do
  if $SEEN_SEP; then
    [ -z "$FILTER" ] && FILTER="$arg"
  elif [ "$arg" = "--" ]; then
    SEEN_SEP=true
  elif [ "$PREV" = "-p" ]; then
    CRATE="$arg"
  fi
  PREV="$arg"
done

# Build header label
LABEL="${BOLD}${CRATE:-workspace}${RESET}"
[ -n "$FILTER" ] && LABEL="$LABEL ${DIM}filter: ${FILTER}${RESET}"

# ── Run tests ───────────────────────────────────────────────────────
printf "\n${DIM}──${RESET} Testing %b ${DIM}──────────────────────────────────${RESET}\n\n" "$LABEL"

OUTPUT=$(cargo test "$@" 2>&1) || true
EXIT_CODE=${PIPESTATUS[0]:-$?}

# ── Parse results (skip empty test binaries) ────────────────────────
RESULTS=$(echo "$OUTPUT" | grep "^test result:" | grep -v "0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out" || true)

if [ -z "$RESULTS" ]; then
  printf "  ${DIM}No matching tests found.${RESET}\n\n"
  exit 0
fi

# Also treat "all zeros except filtered" as no real matches
ALL_ZERO=$(echo "$RESULTS" | awk '{for(i=1;i<=NF;i++) if($i=="passed;") s+=$(i-1)} END{print s+0}')
ALL_FAIL=$(echo "$RESULTS" | awk '{for(i=1;i<=NF;i++) if($i=="failed;") s+=$(i-1)} END{print s+0}')
if [ "$ALL_ZERO" -eq 0 ] && [ "$ALL_FAIL" -eq 0 ]; then
  printf "  ${YELLOW}No tests matched the filter.${RESET}\n\n"
  exit 0
fi

# Aggregate totals
TOTAL_PASSED=$(echo "$RESULTS" | awk '{for(i=1;i<=NF;i++) if($i=="passed;") s+=$(i-1)} END{print s+0}')
TOTAL_FAILED=$(echo "$RESULTS" | awk '{for(i=1;i<=NF;i++) if($i=="failed;") s+=$(i-1)} END{print s+0}')
TOTAL_IGNORED=$(echo "$RESULTS" | awk '{for(i=1;i<=NF;i++) if($i=="ignored;") s+=$(i-1)} END{print s+0}')

# Extract timing from last result line
TIME=$(echo "$RESULTS" | tail -1 | sed 's/.*finished in //' | sed 's/s$/s/')

# ── Display results ─────────────────────────────────────────────────

# Show passed
if [ "$TOTAL_PASSED" -gt 0 ]; then
  printf "  ${GREEN}✓ %d passed${RESET}" "$TOTAL_PASSED"
else
  printf "  ${DIM}0 passed${RESET}"
fi

# Show failed
if [ "$TOTAL_FAILED" -gt 0 ]; then
  printf "  ${RED}✗ %d failed${RESET}" "$TOTAL_FAILED"
fi

# Show ignored
if [ "$TOTAL_IGNORED" -gt 0 ]; then
  printf "  ${YELLOW}○ %d skipped${RESET}" "$TOTAL_IGNORED"
fi

# Timing
printf "  ${DIM}%s${RESET}\n" "$TIME"

# ── Show failure details ────────────────────────────────────────────
if [ "$TOTAL_FAILED" -gt 0 ]; then
  FAILURES=$(echo "$OUTPUT" | awk '/^failures:$/,/^test result:/' | grep '^ ' || true)
  if [ -n "$FAILURES" ]; then
    printf "\n  ${RED}Failed:${RESET}\n"
    echo "$FAILURES" | while read -r line; do
      printf "  ${RED}  ✗${RESET} %s\n" "$line"
    done
  fi
fi

# ── Summary line ────────────────────────────────────────────────────
printf "\n"
if [ "$TOTAL_FAILED" -gt 0 ]; then
  printf "${RED}${BOLD}  FAIL${RESET}${RED} — %d passed, %d failed${RESET}\n" "$TOTAL_PASSED" "$TOTAL_FAILED"
else
  printf "${GREEN}${BOLD}  PASS${RESET}${GREEN} — %d tests${RESET}\n" "$TOTAL_PASSED"
fi
printf "\n"

exit "$EXIT_CODE"
