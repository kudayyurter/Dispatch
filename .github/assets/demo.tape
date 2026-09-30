# .github/assets/demo.tape: regenerates demo.gif.
# From the repo root, after `cargo build --release`:  vhs .github/assets/demo.tape
# Needs vhs, ttyd, ffmpeg and a Nerd Font. The agents are a scripted stand-in:
# see demo-env.sh.
Output .github/assets/demo.gif
Set Shell bash
Set FontFamily "JetBrainsMono Nerd Font Mono"
Set FontSize 16
Set Width 1400
Set Height 800
Set Padding 20
Set Theme "Catppuccin Mocha"
Set TypingSpeed 80ms

# Set up off camera: two finished or waiting agents and a shell.
Hide
Type "source .github/assets/demo-env.sh && dispatch api web"
Enter
Sleep 3s
Escape
Sleep 1s
# Agent 1 works, then finishes while you look elsewhere.
Alt+n
Sleep 1500ms
Down@200ms 4
Sleep 500ms
Enter
Sleep 1500ms
# Agent 2 stops at a permission prompt.
Alt+n
Sleep 1500ms
Down@200ms 4
Sleep 500ms
Enter
Sleep 1500ms
# A shell, focused, so both agents finish out of sight.
Alt+n
Sleep 1500ms
Enter
Sleep 1s
Type "ls"
Enter
Sleep 6s
Show

# One done, one waiting on you. Start a third agent...
Sleep 2500ms
Alt+n
Sleep 1s
Down@250ms 4
Sleep 400ms
Enter
Sleep 2500ms
# ...then go to the one that is waiting and answer it.
Ctrl+p
Sleep 500ms
Type "p"
Sleep 700ms
Type "p"
Sleep 700ms
Escape
Sleep 1s
Type "1"
Enter
Sleep 4s

Hide
Ctrl+o
Type "q"
Sleep 1s
