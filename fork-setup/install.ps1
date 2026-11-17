# Install or update the Moa-01/herdr fork on Windows. No Rust or Zig needed: it takes
# the newest Windows build of the sidebar-fold branch from the fork's GitHub Actions.
# Your herdr config (keybindings included) is not touched.
#
#   .\fork-setup\install.ps1                  install the newest build + the Claude Code cwd hook
#   .\fork-setup\install.ps1 -NoClaudeHook    skip the hook
#   .\fork-setup\install.ps1 -HookOnly        only set up the hook
#   .\fork-setup\install.ps1 -Repo you/herdr  take builds from your own fork instead
#
# Needs the GitHub CLI, logged in (gh auth login). Installs to ~\.local\herdr-fork and
# puts that folder first on your user PATH. The binary it replaces is kept as
# herdr-previous.exe. Quit herdr first (herdr server stop): Windows has no live handoff.
#Requires -Version 7.2
param([string]$Repo = 'Moa-01/herdr', [switch]$NoClaudeHook, [switch]$HookOnly)
$ErrorActionPreference = 'Stop'
$Branch = 'sidebar-fold'
$Dest = Join-Path $HOME '.local\herdr-fork'
$Workflow = 'build-artifacts-manual.yml'

function Install-ClaudeHook {
    $hookDir = Join-Path $HOME '.claude\hooks'
    $hook = Join-Path $hookDir 'herdr-cwd.ps1'
    New-Item -ItemType Directory -Force $hookDir | Out-Null
    Copy-Item (Join-Path $PSScriptRoot 'claude\herdr-cwd.ps1') $hook -Force
    $command = "powershell -NoProfile -ExecutionPolicy Bypass -File `"$($hook -replace '\\', '/')`""
    $settingsPath = Join-Path $HOME '.claude\settings.json'
    $settings = if (Test-Path $settingsPath) {
        Copy-Item $settingsPath "$settingsPath.bak-herdr-fork" -Force
        Get-Content $settingsPath -Raw | ConvertFrom-Json -AsHashtable
    } else { [ordered]@{} }
    if (-not $settings) { $settings = [ordered]@{} }
    if (-not $settings.hooks) { $settings.hooks = [ordered]@{} }
    # Adds the hook to each event once; rerunning changes nothing.
    foreach ($hookEvent in 'SessionStart', 'UserPromptSubmit', 'Stop', 'PostToolUse') {
        $entries = @($settings.hooks[$hookEvent] | Where-Object { $_ })
        if ($entries | Where-Object { $_.hooks | Where-Object { $_.command -eq $command } }) { continue }
        $entry = [ordered]@{ hooks = @([ordered]@{ type = 'command'; command = $command; timeout = 5 }) }
        if ($hookEvent -eq 'PostToolUse') { $entry.matcher = 'Bash' }
        $settings.hooks[$hookEvent] = @($entries + $entry)
    }
    $settings | ConvertTo-Json -Depth 50 | Set-Content $settingsPath -Encoding utf8NoBOM
    Write-Host "Claude Code hook: $hook"
}

if ($HookOnly) { Install-ClaudeHook; exit 0 }

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    throw 'needs the GitHub CLI: winget install GitHub.cli, then gh auth login'
}
$running = Get-Process herdr -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$Dest\*" }
if ($running) { throw "herdr is running from $Dest. Quit it (herdr server stop) and rerun." }

# Newest successful build of the branch whose Windows artifact has not expired yet.
$runs = gh run list -R $Repo -w $Workflow -b $Branch -s success -L 20 --json 'databaseId,headSha' | ConvertFrom-Json
if ($LASTEXITCODE) { throw "could not list builds of $Repo" }
$run = $null
foreach ($candidate in $runs) {
    $artifacts = gh api "repos/$Repo/actions/runs/$($candidate.databaseId)/artifacts" | ConvertFrom-Json
    if ($artifacts.artifacts | Where-Object { $_.name -like 'herdr-windows-x86_64*' -and -not $_.expired }) {
        $run = $candidate; break
    }
}
if (-not $run) {
    throw "no unexpired Windows build of $Branch in $Repo. Ask the fork owner to rebuild, or fork it and run the 'Build artifacts (manual)' workflow with build_group=windows, then pass -Repo you/herdr."
}
$sha = $run.headSha
Write-Host "installing build $($sha.Substring(0, 8)) from https://github.com/$Repo/actions/runs/$($run.databaseId)"

$tmp = Join-Path ([IO.Path]::GetTempPath()) "herdr-fork-$($run.databaseId)"
Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
gh run download $run.databaseId -R $Repo -p 'herdr-windows-x86_64*' -D $tmp
if ($LASTEXITCODE) { throw 'download failed' }
$info = Get-ChildItem $tmp -Recurse -Filter BUILD_INFO.txt | Select-Object -First 1
$zip = Join-Path $info.DirectoryName 'herdr-windows-x86_64.zip'
$text = Get-Content $info.FullName -Raw
if ($text -notmatch "commit=$sha") { throw "artifact is not $sha`n$text" }
$want = [regex]::Match($text, 'sha256=([0-9a-f]{64})').Groups[1].Value
if ((Get-FileHash $zip -Algorithm SHA256).Hash.ToLower() -ne $want) { throw 'zip sha256 mismatch' }

New-Item -ItemType Directory -Force $Dest | Out-Null
$exe = Join-Path $Dest 'herdr.exe'
if (Test-Path $exe) { Copy-Item $exe (Join-Path $Dest 'herdr-previous.exe') -Force }
Expand-Archive $zip -DestinationPath $Dest -Force
Copy-Item $info.FullName $Dest -Force
Remove-Item $tmp -Recurse -Force

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$parts = @($userPath -split ';' | Where-Object { $_ -and $_ -ne $Dest })
if (($userPath -split ';')[0] -ne $Dest) {
    [Environment]::SetEnvironmentVariable('Path', (@($Dest) + $parts) -join ';', 'User')
    Write-Host "put $Dest first on your user PATH (open a new terminal)"
}

if (-not $NoClaudeHook) { Install-ClaudeHook }
Write-Host "`ninstalled $(& $exe --version) ($($sha.Substring(0, 8))) to $Dest"
Write-Host 'start herdr to use it. Smart App Control must be off: the build is unsigned.'
