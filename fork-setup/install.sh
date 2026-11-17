#!/usr/bin/env bash
# Install or update the Moa-01/herdr fork on Linux or macOS from this checkout.
# Your herdr config (~/.config/herdr/config.toml, keybindings included) is not touched.
#
#   fork-setup/install.sh                  build, install, set up the Claude Code cwd hook
#   fork-setup/install.sh --update         git pull first (use this instead of `herdr update`)
#   fork-setup/install.sh --no-claude-hook skip the hook
#   fork-setup/install.sh --hook-only      only set up the hook
#
# Installs to ~/.local/bin/herdr (override with HERDR_FORK_BIN_DIR); the binary it
# replaces is kept as herdr-previous. A running herdr server is handed off live.
set -euo pipefail
cd "$(dirname "$0")/.."
BIN_DIR="${HERDR_FORK_BIN_DIR:-$HOME/.local/bin}"
BIN="$BIN_DIR/herdr"
HOOK_DIR="$HOME/.claude/hooks"
HOOK="$HOOK_DIR/herdr-cwd.sh"

update=0 hook=1 build=1
for arg in "$@"; do
  case "$arg" in
    --update) update=1 ;;
    --no-claude-hook) hook=0 ;;
    --hook-only) build=0 ;;
    -h | --help) sed -n '2,11p' "$0"; exit 0 ;;
    *) echo "unknown option: $arg (see --help)"; exit 1 ;;
  esac
done

need() { command -v "$1" >/dev/null 2>&1 || { echo "missing $1: $2"; exit 1; }; }

install_hook() {
  if ! command -v jq >/dev/null 2>&1; then
    echo "skipping the Claude Code hook: it needs jq (apt install jq / brew install jq), then rerun with --hook-only"
    return
  fi
  mkdir -p "$HOOK_DIR"
  cp fork-setup/claude/herdr-cwd.sh "$HOOK"
  local settings="$HOME/.claude/settings.json" cmd="sh $HOOK"
  [ -f "$settings" ] || echo '{}' >"$settings"
  cp "$settings" "$settings.bak-herdr-fork"
  # Adds the hook to each event once; rerunning changes nothing.
  jq --arg cmd "$cmd" '
    def add(event; matcher):
      if any(.hooks[event][]?.hooks[]?; .command == $cmd) then .
      else .hooks[event] += [{hooks: [{type: "command", command: $cmd, timeout: 5}]}
                             + (if matcher then {matcher: matcher} else {} end)]
      end;
    .hooks //= {}
    | add("SessionStart"; null) | add("UserPromptSubmit"; null)
    | add("Stop"; null) | add("PostToolUse"; "Bash")' \
    "$settings.bak-herdr-fork" >"$settings.tmp" && mv "$settings.tmp" "$settings"
  echo "Claude Code hook: $HOOK (settings backup: $settings.bak-herdr-fork)"
}

if [ "$build" = 1 ]; then
  need git "install git"
  need cargo "install Rust from https://rustup.rs"
  zig="${ZIG:-zig}"
  command -v "$zig" >/dev/null 2>&1 ||
    { echo "missing Zig 0.16.0: https://ziglang.org/download/ (or set ZIG=/path/to/zig)"; exit 1; }
  [ "$("$zig" version)" = 0.16.0 ] ||
    { echo "need Zig 0.16.0, found $("$zig" version) (set ZIG=/path/to/zig-0.16.0)"; exit 1; }

  [ "$update" = 1 ] && git pull --ff-only
  ZIG="$zig" cargo build --release --locked

  mkdir -p "$BIN_DIR"
  [ -f "$BIN" ] && cp -p "$BIN" "$BIN-previous"
  cp target/release/herdr "$BIN.new" && mv "$BIN.new" "$BIN"
  echo "installed $("$BIN" --version) ($(git rev-parse --short HEAD)) to $BIN"
fi

[ "$hook" = 1 ] && install_hook

if [ "$build" = 1 ]; then
  if "$BIN" status server 2>/dev/null | grep -q '^status: running'; then
    # --import-exe: the server's own binary was just replaced, so name the new one.
    "$BIN" server live-handoff --import-exe "$BIN"
    # The handoff drops reported pane tokens; restore Claude's directories.
    [ -f "$HOOK" ] && command -v jq >/dev/null 2>&1 && sh "$HOOK" backfill
    echo "server handed off; reconnect your window to load the new client (prefix+q, then herdr)"
  else
    echo "start herdr to use it"
  fi
  found="$(command -v herdr || true)"
  [ "$found" = "$BIN" ] || echo "warning: 'herdr' on your PATH is ${found:-missing}, not $BIN; put $BIN_DIR first on PATH"
fi
