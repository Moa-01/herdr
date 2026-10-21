# Moa-01/herdr fork

Stock herdr plus a foldable, workspace-grouped agent panel. Work lives on the
`sidebar-fold` branch; `master` mirrors upstream.

- Workspace headers in the agent panel (grouped sort). Click a header or press
  `prefix+f` (`keys.toggle_agent_group`) to fold it to one summary line.
- Folded workspaces count as one stop for next/previous agent.
- Fold state is remembered across restarts.

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

Remotes: `origin` = this fork, `upstream` = herdrdev/herdr (push disabled).
