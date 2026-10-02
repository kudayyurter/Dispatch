# Terminal replies: answer the questions programs ask their terminal

Status: design, approved option 1 on 2026-10-02. Not yet implemented.
Date: 2026-10-02.
Branch: `terminal-replies`, cut from local `main` at ef9e7a9.

## The problem

Programs ask their terminal questions by printing escape sequences, and wait
for the answer to come back on their standard input. Dispatch never answers
any of them.

The user found this testing delegation on 2026-10-01. A fish shell pane took
about 20 seconds to show a prompt and printed:

> warning: fish could not read response to Primary Device Attribute query
> after waiting for 10 seconds.

fish 4 sends Primary Device Attributes (`CSI c`) at startup and waits 10
seconds for the reply. The rest of the delay is fastfetch in the user's fish
config, which asks again. yazi freezes for a moment on exit for the same
reason: it asks, and waits.

### Why nothing answers

libghostty-vt, the emulator Dispatch uses, can generate every reply, but only
through "effects": callbacks set with `ghostty_terminal_set`. The library's
own header says so: "By default ... [it] ignores sequences that have side
effect behavior or require responses". Dispatch sets none (`vt.rs` binds no
`ghostty_terminal_set` at all).

Even if a window's emulator answered, in `--attach` mode it could not reach
the program: the daemon owns the pane's pseudoterminal and deliberately runs
no emulator (`crates/dispatch-daemon/src/pane.rs`: "The daemon holds no
emulator"). Windows have emulators but only draw.

The gap predates the delegation work and is on `main`.

## Decision taken with the user

Of three options offered (the daemon answers with an emulator of its own; the
window in use answers; a hybrid of a byte scanner in the daemon and the
window for the rest), the user chose the first:

**The daemon runs a terminal emulator per pane that only answers.** It is fed
every byte the pane prints, and what it writes back goes to the pane's
pseudoterminal. Every answer is correct (the cursor position too), it works
with no window open, and exactly one answer is ever sent, however many
windows are attached. The cost is parsing each pane's output twice.

Without `--attach`, the window owns both the pseudoterminal and an emulator,
so there the window's own emulator answers.

## What answers, and with what

`VtTerminal` (`crates/dispatch-pty/src/vt.rs`) gains an opt-in answering mode.
With it on:

- `GHOSTTY_TERMINAL_OPT_WRITE_PTY` collects every reply into a buffer owned by
  the `VtTerminal`, taken after each `feed`. Callbacks only copy bytes: they
  must not block and must not write to the terminal (no reentrancy).
- Device attributes (DA1, DA2, DA3) need no callback of their own: the
  library's C wrapper answers them with its defaults (`ESC [ ? 62 ; 22 c` for
  DA1: VT220, ANSI colour) as soon as `WRITE_PTY` is set. DA1 is the one fish
  waits on, and the one most programs send last in a batch of queries.
- `GHOSTTY_TERMINAL_OPT_SIZE` answers XTWINOPS size queries and mode 2048 with
  the size in cells, and zero for pixels: the daemon renders no glyphs, so it
  does not know them, and zero is what a terminal that does not know says.
- Device status (`CSI 5n`, `CSI 6n` cursor position), mode reports (DECRQM),
  the kitty keyboard query and XTVERSION (the library's default
  "libghostty") need no callback beyond `WRITE_PTY`.
- Not answered, on purpose: colour scheme (`CSI ? 996 n`), clipboard reads
  (OSC 52 `?`), and colour queries (OSC 10/11) — the daemon does not know the
  user's colours or clipboard, and an invented answer is worse than none.
  Programs that ask these send DA1 after them, so they still stop waiting.

A drawing emulator (every window's, in `--attach` mode) never answers: its
replies would duplicate the daemon's.

## Where it runs

- **Daemon:** `DaemonPane` gains an answering `VtTerminal` with scrollback set
  to zero (`GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES = 0`): it needs the
  screen's cursor and modes, not history. `drain_pane_output` is the one place
  all of a pane's output passes through; it feeds the answerer and writes any
  replies back with `Pty::write`. It is resized wherever the pane's
  pseudoterminal is, so a cursor-position reply matches the real screen.
- **Without `--attach`:** `PtySession` turns answering on for its own
  emulator, and writes replies back to its `Pty` after each feed.

A reply that cannot be written (the program is not reading its input, which
`Pty::write` reports as `InputFull`) is dropped with a debug log: a terminal
that cannot deliver an answer has nothing better to do with it.

## Not in this piece of work

- Answering colour, colour-scheme or clipboard queries.
- Pixel sizes.
- Windows: ConPTY interprets some queries itself. The change applies there as
  everywhere, and CI runs the tests, but no Windows-specific behaviour is
  designed or verified by hand.

## Testing

- **Unit (`dispatch-pty`):** an answering `VtTerminal` fed `CSI c` yields a
  DA1 reply starting `ESC [ ?`; `CSI 6n` after moving the cursor yields the
  right row and column; `CSI 18t` yields the size in cells; a non-answering
  one yields nothing; replies are taken once.
- **Local session:** a `PtySession` running a program that prints `CSI c` and
  reads the reply gets it, well inside a second.
- **Daemon:** a pane whose program prints `CSI c` and echoes what it reads
  back receives the DA1 reply; with two windows attached the program still
  receives exactly one; with no window attached it still gets one; after a
  resize, `CSI 18t` reports the new size.
- **By hand:** a fish pane in `--attach` mode shows its prompt at once with no
  warning; yazi exits without a pause.
