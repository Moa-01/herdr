# Setting up the Moa-01/herdr fork

This fork is stock [herdr](https://github.com/herdrdev/herdr) plus a few sidebar
features. It follows upstream releases, and the fork's code lives on the
`sidebar-fold` branch.

**Your config stays yours.** The installer never touches
`~/.config/herdr/config.toml`, so your prefix, keybindings, theme and plugins carry
over. The new features work without any config. Their keys use the fork's defaults,
on whatever prefix you already have.

## What you get

| Feature | How to use it |
| --- | --- |
| Agents grouped under their space, foldable | Click a space header in the agent panel, or press `prefix+f` |
| Each space gets its own background tint; its agents share it | Automatic |
| The spaces section grows with the number of spaces, up to the divider | Drag the divider to change the cap |
| Files panel under the agents (the focused pane's folder as a tree) | Click a folder to expand it. Click a file to open it in `$VISUAL`/`$EDITOR` beside the pane, or in a new tab when there is no room |
| Git colors in the files panel | yellow `M` changed, green `U` new, teal `S` staged, blue `↑` committed but not pushed, red `!` conflict. Folders get a dot |
| Files panel follows Claude's folder, or the space's | Click `⇄ claude` / `⇄ space` next to the `files` title |
| Open the folder in your file manager | Click the folder name with `↗` at the right of the `files` title |
| Refresh the files panel | Click the `files` title. It also refreshes every 2 seconds |

### Keys

The fork adds one action. It sits on your prefix like every other herdr key:

| Action | Default | Config name |
| --- | --- | --- |
| Fold or unfold the focused space's agents | `prefix+f` | `keys.toggle_agent_group` |

If you already use `prefix+f`, herdr keeps your binding, disables the clashing one and
reports the clash. To move the fold key, add this to `~/.config/herdr/config.toml`:

```toml
[keys]
toggle_agent_group = "prefix+shift+f"
```

## Install on Linux or macOS

You need git, [Rust](https://rustup.rs) and [Zig 0.16.0](https://ziglang.org/download/)
(exactly that version: set `ZIG=/path/to/zig` if it isn't the `zig` on your PATH).
The Claude Code hook also needs `jq`.

```bash
git clone -b sidebar-fold https://github.com/Moa-01/herdr ~/herdr-fork
~/herdr-fork/fork-setup/install.sh
```

The installer:
- builds herdr and installs it to `~/.local/bin/herdr`. The binary it replaces is kept
  as `herdr-previous`.
- if a herdr server is running, hands it over live, so your panes keep running. Then
  reconnect: `prefix+q`, then `herdr`.
- sets up the Claude Code hook (see below).

Make sure `~/.local/bin` comes first on your PATH. The installer warns if it doesn't.

### Updating

Don't run `herdr update`: it replaces the fork with stock herdr. Run this instead:

```bash
~/herdr-fork/fork-setup/install.sh --update
```

To make `herdr update` do that for you, add this to `~/.bashrc` or `~/.zshrc`:

```bash
herdr() {
  if [ "$1" = update ]; then ~/herdr-fork/fork-setup/install.sh --update; else command herdr "$@"; fi
}
```

## Install on Windows

You don't need Rust or Zig. The installer takes the newest Windows build of this fork
from its GitHub Actions. You need PowerShell 7.2+ and the GitHub CLI, logged in
(`winget install GitHub.cli`, then `gh auth login`).

```powershell
git clone -b sidebar-fold https://github.com/Moa-01/herdr $HOME\herdr-fork
herdr server stop   # if herdr is running: Windows has no live handoff
& $HOME\herdr-fork\fork-setup\install.ps1
```

The installer:
- installs to `~\.local\herdr-fork`, keeping the binary it replaces as
  `herdr-previous.exe`.
- puts that folder first on your user PATH. Open a new terminal afterwards.
- sets up the Claude Code hook.

Smart App Control must be off, because the build is unsigned.

Fork builds are kept for 90 days. If none is left, the installer says so. Then ask
the fork owner to rebuild, or fork the repo, run its "Build artifacts (manual)"
workflow with `build_group=windows`, and install with
`install.ps1 -Repo <you>/herdr`.

To update, `git pull` in `$HOME\herdr-fork`, quit herdr, and run `install.ps1` again.

## The Claude Code hook: how the files panel finds Claude's folder

When Claude runs `cd`, it only updates its own record of the folder. The Claude
process itself never moves, so herdr would always see the folder Claude started in.
The hook reports Claude's real folder to herdr instead:

1. Claude Code runs the hook at session start, on every prompt, at the end of every
   turn, and after every Bash command. Each time, it passes JSON that includes
   Claude's current `cwd`.
2. The hook stores that folder on Claude's pane as the `fork_cwd` value, with
   `herdr pane report-metadata <pane> --source fork:cwd --token fork_cwd=<cwd>`.
   herdr doesn't display this value anywhere.
3. In `⇄ claude` mode, the files panel uses `fork_cwd` when the focused pane has one.
   Otherwise it uses the pane's own folder, which is what `⇄ space` always uses.

The hook only does anything inside herdr: when `HERDR_ENV=1` and `HERDR_PANE_ID` is
set. Everywhere else it exits without doing anything.

| | Linux / macOS | Windows |
| --- | --- | --- |
| Script | `~/.claude/hooks/herdr-cwd.sh` | `~/.claude/hooks/herdr-cwd.ps1` |
| Source | `fork-setup/claude/herdr-cwd.sh` | `fork-setup/claude/herdr-cwd.ps1` |
| Registered in | `~/.claude/settings.json` | `~/.claude/settings.json` |
| Events | `SessionStart`, `UserPromptSubmit`, `Stop`, `PostToolUse` (matcher `Bash`) | same |

The installer adds each hook entry once and leaves your other hooks alone. It backs up
your settings first, to `settings.json.bak-herdr-fork`. Running it again changes
nothing. To set up only the hook, run `install.sh --hook-only` (or `install.ps1
-HookOnly`). To skip it, use `--no-claude-hook` (`-NoClaudeHook`).

A live handoff (on Linux/macOS updates) drops these values. The installer then runs
`herdr-cwd.sh backfill`, which reads each Claude pane's latest `cwd` from its
transcript and reports it again. You can run it yourself at any time:

```bash
sh ~/.claude/hooks/herdr-cwd.sh backfill
```

Other agents can report their folder the same way. Any pane with a `fork_cwd` value
is followed.

## Undo

- **Linux/macOS:** move `~/.local/bin/herdr-previous` back to `~/.local/bin/herdr`, or
  install stock herdr again.
- **Windows:** delete `~\.local\herdr-fork` and remove it from your user PATH.
- **The hook:** delete the `herdr-cwd` entries from `~/.claude/settings.json` (or
  restore the backup) and delete the script.
