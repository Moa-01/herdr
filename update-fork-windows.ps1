# Windows counterpart of update-fork.sh. This machine has no Zig/MSVC, so the build
# runs on the fork's GitHub Actions ("Build artifacts (manual)") and the zip is installed here.
#
#   .\update-fork-windows.ps1              merge newest upstream tag (if new), push, build, install
#   .\update-fork-windows.ps1 -Tag v0.9.7  merge a specific tag
#   .\update-fork-windows.ps1 -Install     skip the merge; build + install origin/sidebar-fold as is
#
# Installs to ~\.local\herdr-fork (must be first on PATH). Previous binary kept as herdr-previous.exe.
#Requires -Version 7.2
param([string]$Tag, [switch]$Install)
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
$Repo = 'Moa-01/herdr'
$Branch = 'sidebar-fold'
$Dest = Join-Path $HOME '.local\herdr-fork'

function Run { & $args[0] $args[1..($args.Count - 1)]; if ($LASTEXITCODE) { throw "failed: $args" } }

if ((git branch --show-current) -ne $Branch) { throw "switch to $Branch first" }
if (git status --porcelain --untracked-files=no) { throw 'commit or stash local changes first' }

# No live handoff on Windows: replacing conpty.dll under a running server fails, and stopping it kills panes.
$running = Get-Process herdr -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$Dest\*" }
if ($running) { throw "herdr is running from $Dest. Quit it (herdr server stop) and rerun." }

Run git pull --ff-only --quiet origin $Branch
if (-not $Install) {
    Run git fetch --quiet upstream --tags
    if (-not $Tag) {
        $Tag = git tag --list 'v*' --sort=-v:refname | Where-Object { $_ -match '^v\d+\.\d+\.\d+$' } | Select-Object -First 1
    }
    git merge-base --is-ancestor $Tag HEAD
    if ($LASTEXITCODE -eq 0) {
        Write-Host "already includes $Tag (use -Install to rebuild anyway)"
        exit 0
    }
    Write-Host "merging $Tag into $Branch"
    git merge --no-edit $Tag
    if ($LASTEXITCODE) {
        Write-Host "`nconflicts in:"; git diff --name-only --diff-filter=U
        Write-Host 'fix them, then: git add -A; git commit --no-edit; git push origin sidebar-fold; .\update-fork-windows.ps1 -Install'
        exit 1
    }
    Run git push origin $Branch
}

$sha = git rev-parse HEAD
$out = gh workflow run build-artifacts-manual.yml -R $Repo --ref $Branch -f build_group=windows 2>&1 | Out-String
if ($LASTEXITCODE) { throw $out }
$runId = [regex]::Match($out, 'runs/(\d+)').Groups[1].Value
if (-not $runId) { throw "could not read run id from: $out" }
Write-Host "building $($sha.Substring(0, 8)) on CI: https://github.com/$Repo/actions/runs/$runId (~10 min)"
gh run watch $runId -R $Repo --exit-status --interval 30 | Out-Null
if ($LASTEXITCODE) { throw "CI build failed: https://github.com/$Repo/actions/runs/$runId" }

$tmp = Join-Path ([IO.Path]::GetTempPath()) "herdr-fork-$runId"
Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
Run gh run download $runId -R $Repo -D $tmp
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

$first = ([Environment]::GetEnvironmentVariable('Path', 'User') -split ';')[0]
if ($first -ne $Dest) { Write-Warning "$Dest is not first on your user PATH; 'herdr' may run another build" }
Write-Host "`ninstalled $(& $exe --version) ($($sha.Substring(0, 8))) to $Dest"
Write-Host 'start herdr again to use it'
