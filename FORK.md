# Moa-01/herdr fork

Stock herdr plus sidebar features (foldable agent groups, space tints, files panel). Work lives on the
`sidebar-fold` branch; `master` mirrors upstream.

**Using the fork (features, keys, install, Claude Code hook): see
[fork-setup/README.md](fork-setup/README.md).**

The rest of this file is for maintaining the fork itself.

## Updating the fork (maintainer)

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
