#!/bin/sh
# Tells the herdr fork's files panel where Claude is working. Claude's `cd` only moves
# its own tracking, never the process cwd, so herdr can't see it without this report.
#
#   herdr-cwd.sh            hook: reads the hook JSON on stdin, reports for this pane
#   herdr-cwd.sh backfill   reports every Claude pane from its transcript; run after a
#                           server handoff, which drops reported tokens
herdr="${HERDR_BIN_PATH:-herdr}"
report() {
  "$herdr" pane report-metadata "$1" --source fork:cwd --token "fork_cwd=$2" >/dev/null 2>&1
}

if [ "${1:-}" = backfill ]; then
  "$herdr" pane list 2>/dev/null |
    jq -r '.result.panes[] | select(.agent == "claude" and .agent_session.value) | "\(.pane_id) \(.agent_session.value)"' |
    while read -r pane session; do
      for transcript in "$HOME"/.claude/projects/*/"$session".jsonl; do
        [ -f "$transcript" ] || continue
        cwd=$(tail -n 200 "$transcript" | jq -r 'select(.cwd) | .cwd' 2>/dev/null | tail -n 1)
        [ -n "$cwd" ] && report "$pane" "$cwd"
      done
    done
  exit 0
fi

[ "${HERDR_ENV:-}" = 1 ] && [ -n "${HERDR_PANE_ID:-}" ] || exit 0
cwd=$(jq -r '.cwd // empty' 2>/dev/null) || exit 0
[ -n "$cwd" ] && report "$HERDR_PANE_ID" "$cwd"
exit 0
