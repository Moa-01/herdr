#!/usr/bin/env bash
# Bring this fork up to the newest herdr release while keeping the sidebar-fold changes,
# then build, install, push, and live-hand-off the running server (panes keep running).
#
#   ./update-fork.sh            merge the newest stable upstream tag
#   ./update-fork.sh v0.9.7     merge a specific tag
#   ./update-fork.sh --install  only rebuild + install the current branch (e.g. after fixing conflicts)
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.local/bin:$PATH" # zig + herdr
BRANCH=sidebar-fold
BIN="$HOME/.local/bin/herdr"

[ "$(git branch --show-current)" = "$BRANCH" ] || { echo "switch to $BRANCH first"; exit 1; }
git diff --quiet && git diff --cached --quiet || { echo "commit or stash local changes first"; exit 1; }

if [ "${1:-}" != "--install" ]; then
  git fetch --quiet upstream --tags
  TAG="${1:-$(git tag --list 'v*' --sort=-v:refname | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | head -1)}"
  if git merge-base --is-ancestor "$TAG" HEAD; then
    echo "already includes $TAG (use --install to rebuild anyway)"
    exit 0
  fi
  echo "merging $TAG into $BRANCH"
  if ! git merge --no-edit "$TAG"; then
    echo
    echo "conflicts in:"
    git diff --name-only --diff-filter=U
    echo "fix them, then: git add -A && git commit --no-edit && ./update-fork.sh --install"
    exit 1
  fi
fi

cargo build --release
# fork tests; clear inherited herdr env so tests don't talk to the live session
env -u HERDR_SOCKET_PATH -u HERDR_ENV -u HERDR_PANE_ID -u HERDR_TAB_ID -u HERDR_WORKSPACE_ID \
  cargo test --release --bin herdr -- agent_panel folded_workspace one_cell_custom config:: keybind

cp -p "$BIN" "$BIN-previous"
cp target/release/herdr "$BIN.new" && mv "$BIN.new" "$BIN"
git push origin "$BRANCH"
herdr server live-handoff
echo
echo "installed $(herdr --version) (previous kept at $BIN-previous)"
echo "reconnect your window to load the new sidebar: ctrl+j q, then herdr"
