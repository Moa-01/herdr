# Windows version of herdr-cwd.sh: tells the herdr fork where Claude is, its working
# directory (`fork_cwd`, for the files panel) and its transcript (`fork_transcript`, for
# subagents and forks in the agent panel). Claude's `cd` only moves its own tracking,
# never the process cwd, so herdr can't see either without this report. Claude Code runs
# it on SessionStart, UserPromptSubmit, Stop and after every Bash tool call, with the
# hook JSON on stdin.
$ErrorActionPreference = 'SilentlyContinue'
if ($env:HERDR_ENV -ne '1' -or -not $env:HERDR_PANE_ID) { exit 0 }
$hook = [Console]::In.ReadToEnd() | ConvertFrom-Json
# Hooks fired inside a subagent (agent_id set) describe the subagent, not the session.
if ($hook.agent_id -or -not $hook.cwd) { exit 0 }
$herdr = if ($env:HERDR_BIN_PATH) { $env:HERDR_BIN_PATH } else { 'herdr' }
& $herdr pane report-metadata $env:HERDR_PANE_ID --source fork:cwd `
    --token "fork_cwd=$($hook.cwd)" --token "fork_transcript=$($hook.transcript_path)" *> $null
exit 0
