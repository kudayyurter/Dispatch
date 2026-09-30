#!/usr/bin/env bash
# .github/assets/demo-env.sh: the clean environment demo.tape records in.
#
# Run (hidden) by the tape. It makes a throwaway directory with a fresh HOME,
# a fresh clone of Dispatch on its default branch, and empty config for
# Dispatch, Claude Code and Codex, then starts Dispatch there under `env -i`.
# Nothing of yours shows: no shell history, dotfiles, prompt, agent settings,
# memories or instructions, and no real path.
#
# The agents are real and logged in as you: Claude Code's credentials are
# linked in and Codex's auth.json is copied, so a recording spends a little of
# both accounts' quota. No credentials are kept in the repo, and the throwaway
# directory is deleted when Dispatch quits.
#
# DISPATCH_BIN overrides the binary (default: target/release/dispatch);
# DEMO_REPO the repository cloned (default: the public one on GitHub).
set -euo pipefail

bin=${DISPATCH_BIN:-$PWD/target/release/dispatch}
repo=${DEMO_REPO:-https://github.com/namelessmonarch0/Dispatch.git}
claude_bin=$(readlink -f "$(command -v claude)")
codex_bin=$(readlink -f "$(command -v codex)")

demo=$(mktemp -d)
trap 'rm -rf "$demo"' EXIT
home=$demo/home clone=$demo/home/code/Dispatch
mkdir -p "$home/code" "$demo/bin" "$demo/dispatch" "$demo/claude" "$demo/codex"
ln -s "$bin" "$demo/bin/dispatch"
ln -s "$claude_bin" "$demo/bin/claude"
ln -s "$codex_bin" "$demo/bin/codex"

git clone -q --depth 1 "$repo" "$clone"

# Dispatch: Claude asks before it runs a command (so it can be blocked on
# you), both agents think briefly, and shell panes skip rc files.
cat > "$demo/dispatch/harness-settings.toml" <<'EOF'
[claude]
effort = "low"
permissions = "manual"

[codex]
effort = "low"
EOF
cat > "$demo/dispatch/config.toml" <<'EOF'
[shell]
command = "/bin/bash"
args = ["--norc", "--noprofile"]
login = "never"
EOF

# Claude Code: logged in, onboarding done, the clone trusted.
ln -s "$HOME/.claude/.credentials.json" "$demo/claude/.credentials.json"
cat > "$demo/claude/.claude.json" <<EOF
{"hasCompletedOnboarding": true, "theme": "dark",
 "projects": {"$clone": {"hasTrustDialogAccepted": true}}}
EOF

# Codex: logged in, the clone trusted, no update box, and no status line or
# rate-limit nudge (both can name the model).
cp "$HOME/.codex/auth.json" "$demo/codex/auth.json"
chmod 600 "$demo/codex/auth.json"
cat > "$demo/codex/config.toml" <<EOF
check_for_update_on_startup = false

[projects."$clone"]
trust_level = "trusted"

[notice]
hide_rate_limit_model_nudge = true

[tui]
status_line = []
EOF

cd "$clone"
clear
env -i \
  HOME="$home" PATH="$demo/bin:/usr/bin:/bin" \
  TERM="${TERM:-xterm-256color}" COLORTERM=truecolor LANG="${LANG:-C.UTF-8}" \
  SHELL=/bin/bash PS1='$ ' \
  DISPATCH_CONFIG_DIR="$demo/dispatch" \
  CLAUDE_CONFIG_DIR="$demo/claude" CLAUDE_CODE_HIDE_CWD=1 DISABLE_AUTOUPDATER=1 \
  CODEX_HOME="$demo/codex" \
  RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}" CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" \
  dispatch .
