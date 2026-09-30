# .github/assets/demo.tape: regenerates demo.gif.
# From the repo root, after `cargo build --release`:  vhs .github/assets/demo.tape
# Needs vhs, ttyd, ffmpeg, a Nerd Font, git, and claude and codex logged in.
# The agents are real: Dispatch works on a fresh clone of itself (see
# demo-env.sh), Claude Code reviews and tests a crate and Codex explains a
# module, which spends a little of both accounts' quota. The real waits are
# cut with Hide/Show, so the GIF runs about 20 s. The Wait patterns match
# Claude Code 2.1 and Codex 0.158; check a dry run after upgrading either.
Output ".github/assets/demo.gif"
Set Shell bash
Set FontFamily "JetBrainsMono Nerd Font Mono"
Set FontSize 16
Set Width 1800
Set Height 1000
Set Padding 16
Set Theme "Catppuccin Mocha"
Set TypingSpeed 10ms
Set WaitTimeout 4m

Hide
Type "bash .github/assets/demo-env.sh"
Enter
Wait+Screen /Projects/
Sleep 2s
Escape

# Claude Code: a small review that ends in running the tests, which it asks
# about first (demo-env.sh turns its permission prompts on).
Alt+n
Sleep 1s
Down 2
Enter
Wait+Screen /for shortcuts/
Sleep 2s
Type "First read crates/dispatch-layout/src/lib.rs and explain in three short bullets how it tiles panes. Only then run cargo test -p dispatch-layout and report the result."
Sleep 500ms
Enter
# Blocked on the permission prompt: the status row counts it.
Wait+Screen /waiting on you/

# Codex: a read-only question about Dispatch itself.
Alt+n
Sleep 1s
Down 3
Enter
Wait+Screen /Ask Codex/
Sleep 2s
Type "How does Dispatch decide that a pane is blocked? Answer in four short bullets, citing files."
Sleep 500ms
Enter
Sleep 1s

# A shell, focused, so both agents carry on out of sight.
Alt+n
Sleep 1s
Enter
Sleep 1500ms
Type "ls crates"
Enter
Sleep 2s
Show

# Claude waits on you while Codex works.
Sleep 4s

# Cut to Codex finishing out of sight: it is marked done.
Hide
Wait+Screen /\x{f058}/
Show
Sleep 2500ms

# Go to the agent that is waiting and let it run the tests.
Ctrl+p
Sleep 400ms
Type "p"
Sleep 400ms
Escape
Sleep 1500ms
Enter
Sleep 3s

# Read Codex's answer while Claude works.
Alt+l
Sleep 4s

# Cut to Claude finishing out of sight: its report is in, and it is marked done.
Hide
Wait+Screen /for \d+s · done/
Wait+Screen /\x{f058}/
Sleep 500ms
Show
Sleep 2s
Alt+h
Sleep 4s

Hide
Ctrl+o
Type "q"
Sleep 2s
