#!/bin/bash
# PreToolUse hook (Bash matcher): denies git commits and SPRT benchmark runs.
# Both must be done by a human, not Claude - see CLAUDE.md's "Hard rules" section.
cmd=$(jq -r '.tool_input.command // empty' 2>/dev/null)

if echo "$cmd" | grep -qE '(^|[;&|[:space:]])git[[:space:]]+commit([[:space:]]|$)'; then
  echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"git commit is not permitted for Claude in this repo - a human must review and commit manually."}}'
elif echo "$cmd" | grep -qE 'fastchess_sprt\.py'; then
  echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"SPRT benchmark runs (python/benches/fastchess_sprt.py) are not permitted for Claude to run - they are compute-heavy, run for hours, and must be launched manually by a human."}}'
else
  echo '{}'
fi
