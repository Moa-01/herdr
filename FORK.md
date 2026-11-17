# Moa-01/herdr fork

Stock herdr plus a foldable, workspace-grouped agent panel. Work lives on the
`sidebar-fold` branch; `master` mirrors upstream.

- Workspace headers in the agent panel (grouped sort). Click a header or press
  `prefix+f` (`keys.toggle_agent_group`) to fold it to one summary line.
- Folded workspaces count as one stop for next/previous agent.
- Fold state is remembered across restarts.
- Each space gets a background tint; its agent rows share it (stronger on focus).
- The spaces section sizes to its content, capped by the dragged divider.
- A files panel under the agents lists the focused pane's directory. Click a folder
  to expand it, click a file to open it in `$VISUAL`/`$EDITOR` (default `vi`; on
  Windows/PowerShell, `edit`, else `notepad`) beside the focused pane, or in a new tab
  when a split would leave a half under 60 columns.
  Click the `files` title to refresh; it also refreshes every few seconds. Click the
  folder name with `↗` at the right of the title to open it in the system file manager
  (`xdg-open` on Linux, Explorer on Windows, Finder on macOS).
- For an agent pane the panel follows the agent's working directory when the agent
  reports it as the `fork_cwd` pane token, since Claude Code's `cd` never moves its
  process cwd. Claude Code reports it with a hook (`~/.claude/hooks/herdr-cwd.sh` on
  SessionStart and PostToolUse/Bash, needs `jq`):

  ```sh
  cwd=$(jq -r '.cwd // empty') && [ -n "$cwd" ] &&
    herdr pane report-metadata "$HERDR_PANE_ID" --source fork:cwd --token "fork_cwd=$cwd"
  ```
- Files are colored by git state, with a letter on the right (folders get a dot in
  the color of their most urgent change): yellow `M` modified and not staged, green
  `U` untracked, teal `S` staged, blue `↑` committed but not pushed, red `!` merge
  conflict. Once the commit is pushed, the file goes back to its normal color.
  This covers the repo holding the folder and every listed subfolder that is a repo
  of its own. Git runs off the UI thread.

## Updating

Do not run `herdr update`: it replaces this build with stock herdr.

```bash
cd ~/herdr-src && ./update-fork.sh
```

It merges the newest upstream release tag, builds, runs the fork tests,
installs to `~/.local/bin/herdr` (previous binary kept as `herdr-previous`),
pushes the branch, and live-hands-off the running server. On conflicts it
stops; fix them, commit, then run `./update-fork.sh --install`.

Build needs Rust and Zig 0.16.0 (`~/.local/bin/zig`).

### Windows

No local toolchain needed; the build runs on this fork's GitHub Actions.

```powershell
cd ~\herdr-fork; .\update-fork-windows.ps1     # -Install to rebuild without merging
```

Same merge step, then it builds `sidebar-fold` with the "Build artifacts (manual)"
workflow, checks the zip's commit and sha256, and installs to `~\.local\herdr-fork`
(previous binary kept as `herdr-previous.exe`). That folder must be first on the user
PATH. Quit herdr first (`herdr server stop`): Windows has no live handoff.
Smart App Control must be off, since the build is unsigned.

Remotes: `origin` = this fork, `upstream` = herdrdev/herdr (push disabled).
