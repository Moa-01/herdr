# Windows version of herdr-cwd.sh: tells the herdr fork's files panel where Claude is
# working. Claude's `cd` only moves its own tracking, never the process cwd, so herdr
# can't see it without this report. Claude Code runs it on SessionStart,
# UserPromptSubmit, Stop and after every Bash tool call, with the hook JSON on stdin.
$ErrorActionPreference = 'SilentlyContinue'
if ($env:HERDR_ENV -ne '1' -or -not $env:HERDR_PANE_ID) { exit 0 }
$cwd = ([Console]::In.ReadToEnd() | ConvertFrom-Json).cwd
if (-not $cwd) { exit 0 }
$herdr = if ($env:HERDR_BIN_PATH) { $env:HERDR_BIN_PATH } else { 'herdr' }
& $herdr pane report-metadata $env:HERDR_PANE_ID --source fork:cwd --token "fork_cwd=$cwd" *> $null
exit 0
