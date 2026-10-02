# Terminal replies Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Programs running in Dispatch panes get answers to the questions they ask their terminal (device attributes, cursor position, size, mode reports), so fish starts at once and yazi exits without a pause.

**Architecture:** `VtTerminal` gains an opt-in answering mode built on libghostty-vt "effects" (`ghostty_terminal_set` with a `WRITE_PTY` callback that collects replies and a `SIZE` callback). Without `--attach`, `PtySession`'s own emulator answers and writes replies to its `Pty`. With `--attach`, each `DaemonPane` gains a reply-only answering emulator (no scrollback) fed in `drain_pane_output`, whose replies go back to the pane's `Pty`; window emulators never answer.

**Tech Stack:** Rust 1.89 workspace; vendored libghostty-vt (C API, Zig-built, statically linked) through hand-written FFI in `crates/dispatch-pty/src/sys.rs`; `cargo test --workspace`.

**Spec:** `docs/superpowers/specs/2026-10-02-terminal-replies-design.md`. Read it before starting any task.

## Global Constraints

- Only an answering emulator replies; a drawing emulator (every window's in `--attach` mode, `VtTerminal::new`) never does.
- Exactly one reply per query reaches the program, however many windows are attached, and replies flow with no window attached.
- Size replies report cells and zero pixels (`cell_width = 0`, `cell_height = 0`).
- Not answered: colour scheme (`CSI ? 996 n`), clipboard reads, OSC 10/11 colour queries. Do not set those callbacks or default colours.
- Callbacks only copy bytes into a buffer: no blocking, no I/O, never call `ghostty_terminal_vt_write` from inside one.
- A reply that cannot be written to the `Pty` is dropped with a `tracing::debug!` log; nothing panics or blocks on it.
- The daemon's answerer has scrollback disabled (`GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES` = 0).
- Every `unsafe` block carries a `// SAFETY:` comment, as the rest of `vt.rs` and `sys.rs` do.
- Run `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings` before each commit; run `cargo build --workspace && cargo test --workspace --no-fail-fast` before each commit. It must be fully green.
- Comments follow the repo voice: full sentences explaining why.
- Every commit message ends with these two lines (the `git commit -m` examples below show only the subject; add a body and these lines):
  ```
  Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_013GkF7AMxpukR9qX1dQFbFK
  ```
- Leave no stray files; `git status` must be clean after each commit.

## Facts established while planning (from the vendored library source)

- `ghostty_terminal_set(terminal, option, const void* value)`: for a callback option, `value` **is** the function pointer cast to `const void*` (not a pointer to it); `src/terminal/c/terminal.zig:1222-1256` casts it straight to the callback type. For `GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES`, `value` is a `const size_t*`. For `GHOSTTY_TERMINAL_OPT_USERDATA`, `value` is the `void*` itself.
- Option ids (`terminal.h`): `USERDATA = 0`, `WRITE_PTY = 1`, `SIZE = 6`, `SCROLLBACK_MAX_BYTES = 27`.
- The C wrapper always installs its device-attributes trampoline (`terminal.zig:670`), which returns the library defaults when no DA callback is set. **So setting `WRITE_PTY` alone makes DA1 answer `ESC [ ? 6 2 ; 2 2 c`** (VT220, ANSI colour). The spec's line saying a `DEVICE_ATTRIBUTES` callback is needed is wrong; Task 1 corrects it. No DA callback is set.
- The size trampoline returns no report when no `SIZE` callback is set, so `SIZE` is needed for `CSI 18 t` and mode 2048.
- `GhosttySizeReportSize` is `{ uint16_t rows; uint16_t columns; uint32_t cell_width; uint32_t cell_height; }` (`size_report.h`).
- Callback types (`terminal.h`):
  - `void write_pty(GhosttyTerminal, void* userdata, const uint8_t* data, size_t len)`
  - `bool size(GhosttyTerminal, void* userdata, GhosttySizeReportSize* out)`

## Review Focus

- **A query split across two reads** (the `ESC` at the end of one chunk, `[c` at the start of the next) must still be answered: the pty delivers output in arbitrary chunks. Pinned in Task 1.
- **A full-screen program on the alternate screen asking for the cursor position** (yazi, vim) must get the alternate screen's position. Pinned in Task 1.
- **A mode query (DECRQM, e.g. bracketed paste `CSI ? 2004 $ p`)** must be answered, since TUIs probe modes at start. Pinned in Task 1.
- **A program that floods queries and never reads its input** must not stall the daemon or panic it: the replies back up and are dropped. Pinned in Task 3.
- **A late window replayed a pane's history** must not cause a second reply: replay goes to windows, which never answer, and the answerer is fed each byte once. Pinned in Task 3.

## File map

| File | Change |
|---|---|
| `crates/dispatch-pty/src/sys.rs` | bind `ghostty_terminal_set`, option ids, `SizeReportSize`, callback types |
| `crates/dispatch-pty/src/vt.rs` | `VtTerminal::answering`, `take_replies`, `disable_scrollback`; size kept for the callback |
| `crates/dispatch-pty/src/vt/tests.rs` or the existing vt test module | unit tests |
| `crates/dispatch-pty/src/session.rs`, `session/tests.rs` | `PtySession` answers |
| `crates/dispatch-daemon/src/pane.rs` | `DaemonPane::answerer` and `answer()` |
| `crates/dispatch-daemon/src/session.rs`, `session/tests.rs` | feed the answerer, resize it, construct it |
| `docs/superpowers/specs/2026-10-02-terminal-replies-design.md` | correct the DA line (Task 1), status (Task 3) |

---

### Task 1: An answering `VtTerminal`

**Files:**
- Modify: `crates/dispatch-pty/src/sys.rs`
- Modify: `crates/dispatch-pty/src/vt.rs`
- Test: the `vt` test module (if `vt.rs` has none, create `crates/dispatch-pty/src/vt/tests.rs` with `#[cfg(test)] mod tests;` at the bottom of `vt.rs`, following how `screen.rs` and `session.rs` hold theirs)
- Modify: `docs/superpowers/specs/2026-10-02-terminal-replies-design.md`

**Interfaces:**
- Produces:
  - `VtTerminal::answering(size: Size) -> Result<VtTerminal, VtError>`: an emulator that answers queries.
  - `VtTerminal::take_replies(&mut self) -> Vec<u8>`: replies produced since the last call; always empty for `VtTerminal::new`.
  - `VtTerminal::disable_scrollback(&mut self) -> Result<(), VtError>`: sets the scrollback byte limit to zero.
  - `VtTerminal::resize` keeps the size the `SIZE` callback reports in step.

- [ ] **Step 1: Write the failing tests**

```rust
use super::*;

fn answering() -> VtTerminal {
    VtTerminal::answering(Size::new(80, 24)).expect("a terminal")
}

#[test]
fn primary_device_attributes_are_answered() {
    let mut terminal = answering();
    terminal.feed(b"\x1b[c");
    assert_eq!(terminal.take_replies(), b"\x1b[?62;22c");
}

#[test]
fn a_drawing_terminal_answers_nothing() {
    let mut terminal = VtTerminal::new(Size::new(80, 24)).expect("a terminal");
    terminal.feed(b"\x1b[c\x1b[6n\x1b[18t");
    assert!(terminal.take_replies().is_empty());
}

#[test]
fn replies_are_taken_once() {
    let mut terminal = answering();
    terminal.feed(b"\x1b[c");
    assert!(!terminal.take_replies().is_empty());
    assert!(terminal.take_replies().is_empty());
}

#[test]
fn the_cursor_position_is_reported_where_the_cursor_is() {
    let mut terminal = answering();
    terminal.feed(b"\x1b[5;10H\x1b[6n");
    assert_eq!(terminal.take_replies(), b"\x1b[5;10R");
}

#[test]
fn the_alternate_screen_reports_its_own_cursor() {
    let mut terminal = answering();
    terminal.feed(b"\x1b[10;10H\x1b[?1049h\x1b[3;4H\x1b[6n");
    assert_eq!(terminal.take_replies(), b"\x1b[3;4R");
}

#[test]
fn the_size_is_reported_in_cells() {
    let mut terminal = answering();
    terminal.feed(b"\x1b[18t");
    assert_eq!(terminal.take_replies(), b"\x1b[8;24;80t");
}

#[test]
fn a_resize_changes_the_reported_size() {
    let mut terminal = answering();
    terminal.resize(Size::new(100, 30)).expect("resizes");
    terminal.feed(b"\x1b[18t");
    assert_eq!(terminal.take_replies(), b"\x1b[8;30;100t");
}

#[test]
fn a_query_split_across_two_feeds_is_answered() {
    let mut terminal = answering();
    terminal.feed(b"hello \x1b");
    terminal.feed(b"[c");
    assert_eq!(terminal.take_replies(), b"\x1b[?62;22c");
}

#[test]
fn a_mode_query_is_answered() {
    let mut terminal = answering();
    terminal.feed(b"\x1b[?2004h\x1b[?2004$p");
    assert_eq!(terminal.take_replies(), b"\x1b[?2004;1$y");
}

#[test]
fn an_answerer_without_scrollback_still_answers() {
    let mut terminal = answering();
    terminal.disable_scrollback().expect("sets");
    for _ in 0..200 {
        terminal.feed(b"a line of output\r\n");
    }
    terminal.feed(b"\x1b[c");
    assert_eq!(terminal.take_replies(), b"\x1b[?62;22c");
}
```

If a reply's exact bytes differ from these because the library encodes them differently (for example the DECRQM value for a set mode), check the library's own encoder in `vendor/libghostty-vt/src/terminal/` and assert what it produces. Each test must still prove the query was answered, with the right row, column or size where it checks one.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p dispatch-pty answer` and `cargo test -p dispatch-pty reported`
Expected: compile errors (`answering`, `take_replies`, `disable_scrollback` missing).

- [ ] **Step 3: Bind the library**

In `crates/dispatch-pty/src/sys.rs`, beside the existing terminal bindings:

```rust
/// `GHOSTTY_TERMINAL_OPT_USERDATA`: the `void*` passed to every callback.
pub const OPT_USERDATA: i32 = 0;
/// `GHOSTTY_TERMINAL_OPT_WRITE_PTY`: replies the terminal writes back.
pub const OPT_WRITE_PTY: i32 = 1;
/// `GHOSTTY_TERMINAL_OPT_SIZE`: the size reported to XTWINOPS and mode 2048.
pub const OPT_SIZE: i32 = 6;
/// `GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES`: a `const size_t*`.
pub const OPT_SCROLLBACK_MAX_BYTES: i32 = 27;

/// Mirrors `GhosttySizeReportSize` in `size_report.h`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SizeReportSize {
    pub rows: u16,
    pub columns: u16,
    pub cell_width: u32,
    pub cell_height: u32,
}

/// `GhosttyTerminalWritePtyFn`.
pub type WritePtyFn =
    unsafe extern "C" fn(terminal: Terminal, userdata: *mut c_void, data: *const u8, len: usize);

/// `GhosttyTerminalSizeFn`.
pub type SizeFn =
    unsafe extern "C" fn(terminal: Terminal, userdata: *mut c_void, out: *mut SizeReportSize) -> bool;
```

and in the `unsafe extern "C"` block:

```rust
    /// `ghostty_terminal_set(GhosttyTerminal, GhosttyTerminalOption, const void*)`
    ///
    /// For a callback option, `value` is the function pointer itself.
    pub fn ghostty_terminal_set(terminal: Terminal, option: i32, value: *const c_void) -> GhosttyResult;
```

Match the existing file's style for option enums (if it declares `GhosttyTerminalOption` values as an enum or a type alias, follow that instead of bare `i32`).

- [ ] **Step 4: Implement answering**

In `crates/dispatch-pty/src/vt.rs`:

```rust
/// What an answering terminal's callbacks write to while it is fed.
///
/// Boxed, so its address stays put for as long as libghostty-vt holds it as
/// userdata, however the `VtTerminal` that owns it moves.
#[derive(Debug, Default)]
struct Answers {
    /// Replies not yet taken.
    replies: Vec<u8>,
    /// The size to report, kept in step with every resize.
    size: Size,
}
```

`VtTerminal` gains `answers: Option<Box<Answers>>` (`None` from `new`). Field order matters: `handle` is freed in `Drop::drop`, which runs before the fields drop, so the library never holds a dangling userdata pointer. Say so in a comment.

```rust
    /// Creates a terminal that answers the questions programs ask it:
    /// device attributes, cursor position, size, mode reports.
    ///
    /// Only one emulator per pane may answer, or a program would get every
    /// answer twice. A window drawing a pane it does not own uses
    /// [`VtTerminal::new`], which answers nothing.
    pub fn answering(size: Size) -> Result<Self, VtError> {
        let mut terminal = Self::new(size)?;
        let mut answers = Box::new(Answers {
            replies: Vec::new(),
            size,
        });
        let userdata: *mut Answers = &raw mut *answers;

        // SAFETY: the handle is live; userdata points into a Box this
        // terminal owns and frees only after the handle (see `Drop`); the
        // callbacks match the C signatures in `terminal.h`, and a callback
        // option takes the function pointer itself as its value.
        unsafe {
            VtError::check(
                "ghostty_terminal_set(USERDATA)",
                sys::ghostty_terminal_set(terminal.handle, sys::OPT_USERDATA, userdata.cast()),
            )?;
            VtError::check(
                "ghostty_terminal_set(WRITE_PTY)",
                sys::ghostty_terminal_set(
                    terminal.handle,
                    sys::OPT_WRITE_PTY,
                    write_pty as sys::WritePtyFn as *const c_void,
                ),
            )?;
            VtError::check(
                "ghostty_terminal_set(SIZE)",
                sys::ghostty_terminal_set(
                    terminal.handle,
                    sys::OPT_SIZE,
                    report_size as sys::SizeFn as *const c_void,
                ),
            )?;
        }

        terminal.answers = Some(answers);
        Ok(terminal)
    }

    /// The replies produced since the last call, to be written to the
    /// program's input. Always empty for a terminal made with
    /// [`VtTerminal::new`].
    pub fn take_replies(&mut self) -> Vec<u8> {
        self.answers
            .as_mut()
            .map(|answers| std::mem::take(&mut answers.replies))
            .unwrap_or_default()
    }

    /// Keeps no scrollback: for a terminal that only answers, which needs the
    /// screen's cursor and modes but never its history.
    pub fn disable_scrollback(&mut self) -> Result<(), VtError> {
        let zero: usize = 0;
        // SAFETY: the handle is live, and this option takes a `const size_t*`,
        // which `zero` outlives for the call.
        let code = unsafe {
            sys::ghostty_terminal_set(
                self.handle,
                sys::OPT_SCROLLBACK_MAX_BYTES,
                (&raw const zero).cast(),
            )
        };
        VtError::check("ghostty_terminal_set(SCROLLBACK_MAX_BYTES)", code)
    }
```

In `resize`, after the library call succeeds: `if let Some(answers) = self.answers.as_mut() { answers.size = size; }`.

The callbacks, as free functions in `vt.rs`:

```rust
/// Collects a reply the terminal writes back to the program.
///
/// Called synchronously from inside `ghostty_terminal_vt_write`: it only
/// copies bytes, and must never touch the terminal itself.
unsafe extern "C" fn write_pty(
    _terminal: sys::Terminal,
    userdata: *mut c_void,
    data: *const u8,
    len: usize,
) {
    if userdata.is_null() || data.is_null() || len == 0 {
        return;
    }
    // SAFETY: userdata is the `Answers` box installed by `answering`, alive
    // for as long as the handle; nothing else borrows it during a feed. The
    // library guarantees `data` is valid for `len` bytes during the call.
    let (answers, bytes) = unsafe {
        (
            &mut *userdata.cast::<Answers>(),
            std::slice::from_raw_parts(data, len),
        )
    };
    answers.replies.extend_from_slice(bytes);
}

/// Reports the size in cells; zero pixels, since nothing here renders glyphs.
unsafe extern "C" fn report_size(
    _terminal: sys::Terminal,
    userdata: *mut c_void,
    out: *mut sys::SizeReportSize,
) -> bool {
    if userdata.is_null() || out.is_null() {
        return false;
    }
    // SAFETY: as in `write_pty`; `out` is a valid out-pointer for the call.
    unsafe {
        let answers = &*userdata.cast::<Answers>();
        *out = sys::SizeReportSize {
            rows: answers.size.rows,
            columns: answers.size.cols,
            cell_width: 0,
            cell_height: 0,
        };
    }
    true
}
```

`Size` needs `Default` for `Answers`' derive; if it has none, drop `Default` from `Answers`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p dispatch-pty`
Expected: the new tests and every existing `dispatch-pty` test pass.

- [ ] **Step 6: Correct the spec**

In the spec's "What answers, and with what" list, replace the `GHOSTTY_TERMINAL_OPT_DEVICE_ATTRIBUTES` bullet with:

```markdown
- Device attributes (DA1, DA2, DA3) need no callback of their own: the
  library's C wrapper answers them with its defaults (`ESC [ ? 62 ; 22 c` for
  DA1: VT220, ANSI colour) as soon as `WRITE_PTY` is set. DA1 is the one fish
  waits on, and the one most programs send last in a batch of queries.
```

- [ ] **Step 7: Commit**

```bash
git add crates/dispatch-pty docs/superpowers/specs/2026-10-02-terminal-replies-design.md
git commit -m "feat(pty): an emulator that answers the questions programs ask their terminal"
```

---

### Task 2: A local session answers

**Files:**
- Modify: `crates/dispatch-pty/src/session.rs`
- Test: `crates/dispatch-pty/src/session/tests.rs`

**Interfaces:**
- Consumes: `VtTerminal::answering`, `VtTerminal::take_replies` (Task 1); `Pty::write(&mut self, &[u8]) -> Result<(), PtyError>`.
- Produces: `PtySession` (used without `--attach`) answers its program's queries.

- [ ] **Step 1: Write the failing test**

Beside `input_written_to_a_pane_reaches_the_child` in `session/tests.rs` (it has helpers `shell(script)`, `cwd()`, `visible(&session)`, `wait_until`):

```rust
#[cfg(unix)]
#[test]
fn a_program_that_asks_its_terminal_gets_an_answer() {
    // Raw mode so the reply is read as it arrives, not held for a newline;
    // the escape is shown as `E` so the screen can be matched as text.
    let mut session = PtySession::spawn(
        &shell("stty raw -echo; printf '\\033[c'; head -c 9 | tr '\\033' E; printf '\\r\\nDONE'"),
        &cwd(),
        Size::new(80, 24),
    )
    .expect("spawns");

    let started = std::time::Instant::now();
    session.drain_until_exit(std::time::Duration::from_secs(5));

    let screen = visible(&session).join("\n");
    assert!(screen.contains("E[?62;22c"), "{screen}");
    assert!(screen.contains("DONE"), "{screen}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "answered at once, not after a timeout"
    );
}
```

Adapt `shell(...)` and `visible(...)` to their real signatures in that file.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p dispatch-pty a_program_that_asks`
Expected: FAIL. `head -c 9` waits forever for a reply, so `drain_until_exit` times out and the screen has no `E[?62;22c`.

- [ ] **Step 3: Implement**

In `session.rs`:
- `PtySession::spawn` builds `terminal: VtTerminal::answering(size)?`.
- Add a private helper used after every `self.terminal.feed(..)` (in `drain_output` and `drain_until_exit`):

```rust
    /// Writes back whatever the program asked its terminal, so it is not
    /// left waiting for an answer that never comes.
    fn answer(&mut self) {
        let replies = self.terminal.take_replies();
        if replies.is_empty() {
            return;
        }
        if let Err(error) = self.pty.write(&replies) {
            // A program not reading its input cannot be answered; a
            // terminal has nothing better to do with the reply.
            tracing::debug!(%error, "dropped a terminal reply");
        }
    }
```

`drain_until_exit` feeds everything only once the child exits, so a program that waits for its answer would never exit. Change it to drain in a loop: feed, answer, and repeat until the child exits or the timeout passes. Use the existing `Pty::drain` and `Pty::state` for this. Keep its return value and contract. Read `Pty::drain_until_exit` first and reuse its timing approach; do not change `Pty::drain_until_exit` itself, which the daemon side does not use for this.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p dispatch-pty`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/dispatch-pty/src/session.rs crates/dispatch-pty/src/session/tests.rs
git commit -m "feat(pty): a local session answers its program's terminal queries"
```

---

### Task 3: The daemon answers

**Files:**
- Modify: `crates/dispatch-daemon/src/pane.rs`
- Modify: `crates/dispatch-daemon/src/session.rs`
- Test: `crates/dispatch-daemon/src/session/tests.rs`
- Modify: `docs/superpowers/specs/2026-10-02-terminal-replies-design.md` (status line)

**Interfaces:**
- Consumes: `VtTerminal::answering`, `take_replies`, `disable_scrollback`, `resize` (Task 1).
- Produces:
  - `DaemonPane::answerer: Option<VtTerminal>`
  - `DaemonPane::answer(&mut self, output: &[u8])`
  - `fn answerer_for(size: Size) -> Option<VtTerminal>` in `session.rs`

- [ ] **Step 1: Write the failing tests**

In `crates/dispatch-daemon/src/session/tests.rs`, add a harness to `harnesses()`: Unix only, since the tests are `#[cfg(unix)]`. Write it the way the other fixture harnesses are written there.

```rust
    // Asks its terminal who it is, shows the answer with the escape as `E`,
    // then shows whatever else arrives within half a second: a second
    // answer would be there.
    if !cfg!(windows) {
        std::fs::write(
            dir.join("asker.toml"),
            "id = \"asker\"\ndisplay_name = \"Asker\"\ncommand = \"sh\"\nargs = [\"-c\", \"stty raw -echo; printf '\\\\033[c'; head -c 9 | tr '\\\\033' E; stty min 0 time 5; extra=$(head -c 64 | wc -c); printf '\\\\r\\\\nEXTRA:%s\\\\r\\\\n' $extra; sleep 30\"]\n",
        )
        .expect("temp dir is writable");
    }
```

Check the escaping by printing the harness file once in a scratch test, or write it with a raw string. The shell must receive `printf '\033[c'`.

Then the tests:

```rust
/// Spawns a pane of `harness` for client 1 and returns its id.
fn spawn_harness(daemon: &mut Daemon, inbox: &Inbox, project: ProjectId, harness: &str) -> PaneId {
    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: harness.into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );
    let seen = wait_for(daemon, inbox, |m| {
        m.iter().any(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
    });
    seen.iter()
        .find_map(|m| match m {
            ServerMessage::PaneSpawned { pane, .. } => Some(*pane),
            _ => None,
        })
        .expect("a pane was spawned")
}

#[cfg(unix)]
#[test]
fn a_program_that_asks_its_terminal_gets_exactly_one_answer() {
    let (mut daemon, project, _dir) = daemon("answer-once");
    let first = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let second = daemon.attach_for_test(2);
    daemon.request_for_test(2, hello());
    daemon.request_for_test(2, ClientMessage::Subscribe);

    let pane = spawn_harness(&mut daemon, &first, project, "asker");
    let seen = wait_for(&mut daemon, &first, |m| output_of(m, pane).contains("EXTRA:"));
    let _ = drain(&second);

    let output = output_of(&seen, pane);
    assert!(output.contains("E[?62;22c"), "{output:?}");
    assert!(output.contains("EXTRA:0"), "one answer, not one per window: {output:?}");
}

#[cfg(unix)]
#[test]
fn a_program_is_answered_with_no_window_watching() {
    let (mut daemon, project, _dir) = daemon("answer-unwatched");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let pane = spawn_harness(&mut daemon, &ui, project, "asker");

    daemon.detach_for_test(1);
    for _ in 0..200 {
        daemon.tick();
        std::thread::sleep(Duration::from_millis(10));
    }

    // A window arriving now is replayed what the pane printed.
    let late = daemon.attach_for_test(3);
    daemon.request_for_test(3, hello());
    daemon.request_for_test(3, ClientMessage::Subscribe);
    let output = output_of(&drain(&late), pane);
    assert!(output.contains("E[?62;22c"), "{output:?}");
    assert!(output.contains("EXTRA:0"), "replay causes no second answer: {output:?}");
}
```

Add a third test for resize. It needs a harness that waits for one input byte before asking for its size. Add it beside `asker`:

```rust
    if !cfg!(windows) {
        std::fs::write(
            dir.join("sizer.toml"),
            "id = \"sizer\"\ndisplay_name = \"Sizer\"\ncommand = \"sh\"\nargs = [\"-c\", \"stty raw -echo; head -c 1 >/dev/null; printf '\\\\033[18t'; head -c 11 | tr '\\\\033' E; sleep 30\"]\n",
        )
        .expect("temp dir is writable");
    }
```

```rust
#[cfg(unix)]
#[test]
fn the_answered_size_follows_a_resize() {
    let (mut daemon, project, _dir) = daemon("answer-resize");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let pane = spawn_harness(&mut daemon, &ui, project, "sizer");

    daemon.request_for_test(1, ClientMessage::ResizePane { pane, size: (100, 30) });
    wait_for(&mut daemon, &ui, |m| {
        m.iter().any(|m| matches!(m, ServerMessage::PaneResized { size: (100, 30), .. }))
    });
    daemon.request_for_test(1, ClientMessage::WritePane { pane, bytes: b"x".to_vec() });

    let seen = wait_for(&mut daemon, &ui, |m| output_of(m, pane).contains("E[8;"));
    let output = output_of(&seen, pane);
    assert!(output.contains("E[8;30;100t"), "{output:?}");
}
```

And the flood case from Review Focus:

```rust
#[cfg(unix)]
#[test]
fn a_program_that_floods_queries_and_never_reads_does_not_stall_the_daemon() {
    let (mut daemon, project, _dir) = daemon("answer-flood");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let _pane = spawn_harness(&mut daemon, &ui, project, "flooder");

    let started = Instant::now();
    for _ in 0..300 {
        daemon.tick();
    }
    daemon.request_for_test(1, ClientMessage::Ping { token: 7 });
    let seen = wait_for(&mut daemon, &ui, |m| {
        m.iter().any(|m| matches!(m, ServerMessage::Pong { token: 7 }))
    });
    assert!(!seen.is_empty());
    assert!(started.elapsed() < Duration::from_secs(10), "the loop kept turning");
}
```

It uses a third Unix-only fixture, added to `harnesses()` beside `asker` and `sizer`. The fixture writes 100,000 queries and never reads its input:

```rust
    if !cfg!(windows) {
        std::fs::write(
            dir.join("flooder.toml"),
            "id = \"flooder\"\ndisplay_name = \"Flooder\"\ncommand = \"sh\"\nargs = [\"-c\", \"stty raw -echo; i=0; while [ $i -lt 100000 ]; do printf '\\\\033[c'; i=$((i+1)); done; sleep 30\"]\n",
        )
        .expect("temp dir is writable");
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p dispatch-daemon answer`
Expected: the asker and sizer tests time out in `wait_for` (no reply ever reaches the program); the flood test may already pass, and that's fine: it guards against a regression this task could introduce.

- [ ] **Step 3: Implement**

`crates/dispatch-daemon/src/pane.rs`: change the "holds no emulator" doc on `DaemonPane` to say it holds no *drawing* emulator. It runs one only to answer: a program's questions must be answered by whoever owns its pseudoterminal, and exactly once. Then add:

```rust
    /// Answers the questions the pane's program asks its terminal. `None`
    /// only if one could not be created, in which case the program goes
    /// unanswered, as it did before Dispatch answered at all.
    pub answerer: Option<dispatch_pty::VtTerminal>,
```

```rust
    /// Feeds `output` to the answerer and writes its replies back to the
    /// program.
    pub fn answer(&mut self, output: &[u8]) {
        let Some(answerer) = self.answerer.as_mut() else {
            return;
        };
        answerer.feed(output);
        let replies = answerer.take_replies();
        if replies.is_empty() {
            return;
        }
        if let Err(error) = self.session.write(&replies) {
            // Not reading its input: the reply has nowhere to go.
            tracing::debug!(pane = %self.id, %error, "dropped a terminal reply");
        }
    }
```

(Export `VtTerminal` from `dispatch_pty` if it is not already public at the crate root.)

`crates/dispatch-daemon/src/session.rs`:

```rust
/// An emulator that only answers a pane's terminal queries, sized to match
/// it. Keeps no scrollback, since answers need the screen, not its history.
fn answerer_for(size: Size) -> Option<dispatch_pty::VtTerminal> {
    let made = dispatch_pty::VtTerminal::answering(size).and_then(|mut answerer| {
        answerer.disable_scrollback()?;
        Ok(answerer)
    });
    match made {
        Ok(answerer) => Some(answerer),
        Err(error) => {
            tracing::warn!(%error, "no answering emulator; this pane's terminal queries go unanswered");
            None
        }
    }
}
```

- Both `DaemonPane { … }` literals (in `spawn_pane` and `approve`) set `answerer: answerer_for(<the size the Pty was spawned at>)` and `..` nothing else new.
- `drain_pane_output` calls `pane.answer(&output)` after `pane.remember(&output)`.
- In `fit`, after `target.session.resize(size)` succeeds, resize the answerer too: `if let Some(answerer) = target.answerer.as_mut() && let Err(error) = answerer.resize(size) { tracing::debug!(%error, "failed to resize an answerer"); }`.

- [ ] **Step 4: Run the tests**

Run: `cargo build --workspace && cargo test --workspace --no-fail-fast`
Expected: all pass.

- [ ] **Step 5: Mark the spec**

Set the spec's `Status:` line to `Status: implemented on branch \`terminal-replies\`; hands-on check with fish and yazi pending.`

- [ ] **Step 6: Commit**

```bash
git add crates/dispatch-daemon crates/dispatch-pty/src/lib.rs docs/superpowers/specs/2026-10-02-terminal-replies-design.md
git commit -m "feat(daemon): answer each pane's terminal queries, once, with or without a window"
```
