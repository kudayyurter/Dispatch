# .github/assets/demo-env.sh: a throwaway environment for demo.tape.
# Sourced (hidden) by the tape. Nothing here touches your real config or agents:
# HOME and DISPATCH_CONFIG_DIR are temporary, and the only agent the tape opens
# is a scripted stand-in ("Demo agent") that prints canned output, so no agent
# runs and no API is called. The real harnesses still appear in the picker.
#
# DISPATCH_BIN overrides the binary (default: target/release/dispatch).

bin=${DISPATCH_BIN:-$PWD/target/release/dispatch}
demo=$(mktemp -d)
export HOME=$demo/home DISPATCH_CONFIG_DIR=$demo/config
mkdir -p "$HOME" "$DISPATCH_CONFIG_DIR/harnesses" "$demo/bin"
ln -s "$bin" "$demo/bin/dispatch"
export PATH=$demo/bin:/usr/bin:/bin SHELL=/bin/bash PS1='$ '

# Two sample projects.
for p in api web; do
  mkdir -p "$HOME/code/$p/src"
  touch "$HOME/code/$p/src/main.rs" "$HOME/code/$p/Cargo.toml"
  printf '# %s\n' "$p" > "$HOME/code/$p/README.md"
  git -C "$HOME/code/$p" init -q -b main
done

# The stand-in agent: each pane it opens plays the next of three scripts.
cat > "$demo/bin/demo-agent" <<'EOF'
#!/bin/bash
n=$(( $(cat "$HOME/.demo-count" 2>/dev/null || echo 0) + 1 )); echo $n > "$HOME/.demo-count"
say() { printf '%s\n' "$1"; sleep "${2:-0.5}"; }
case $n in
  1) say "> add retries to the http client"; say "reading src/http/client.rs"; say "editing src/http/client.rs"
     say "running cargo test"; say "  42 passed" 1; say "Done: 2 files changed."; printf '\n> ' ;;
  2) say "> bump the lockfile"; say "reading Cargo.lock"
     printf '\nDo you want to proceed?\n❯ 1. Yes\n  2. No\n'; read -r _
     say "updating Cargo.lock"; say "  18 packages updated"; say "running cargo test"; say "  42 passed"
     say "Done: lockfile bumped."; printf '\n> ' ;;
  *) say "> write docs for the api"; while :; do say "writing docs/api.md ($((i+=1)))" 0.4; done ;;
esac
exec cat > /dev/null
EOF
chmod +x "$demo/bin/demo-agent"

cat > "$DISPATCH_CONFIG_DIR/harnesses/demo.toml" <<'EOF'
id = "demo"
icon = "󰚩"
display_name = "Demo agent"
command = "demo-agent"
args = []

[[status.rules]]
state = "blocked"
region = "bottom:5"
contains = ["do you want to proceed?"]
priority = 990
EOF

# Shell panes skip rc files, so no system prompt or title names you or this machine.
cat > "$DISPATCH_CONFIG_DIR/config.toml" <<'EOF'
[shell]
command = "/bin/bash"
args = ["--norc", "--noprofile"]
login = "never"
EOF

cd "$HOME/code"
clear
