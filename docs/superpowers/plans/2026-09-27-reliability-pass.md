# Reliability Pass: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix five problems a day-long Dispatch session trips over. Each is pinned by a test that fails before its fix.
1. Accepting stops for good after a passing error.
2. Pane output is limited per read rather than per byte.
3. A long-exited pane's process group number can be reused.
4. A new subagent takes focus.
5. A closed listener leaves its socket file behind.

**Architecture:**
- Tasks 1 and 5 change the listener in `dispatch-os/src/ipc.rs`.
- Task 2 changes the reader/drain queue in `dispatch-pty/src/session.rs`.
- Task 3 moves the end of a pane's tree into `dispatch-os/src/pty.rs`, whose `Child` now keeps an exited leader unreaped until the pane closes. `dispatch-pty` shares that `Child` between its waiter thread and its close.
- Task 4 changes one rule in `AppState::adopt_pane`.

**Tech Stack:** Rust 2024; `libc` on Unix, `windows-sys` 0.61 on Windows (both already dependencies of `dispatch-os`); `portable-pty` for Unix panes.

**Spec:** `docs/superpowers/specs/2026-09-27-reliability-pass-design.md`

## Global Constraints

- No new dependencies. The protocol does not change.
- Platform `#[cfg]` attributes live only in `dispatch-os`. Tests may carry `#[cfg(...)]`.
- `rust-version` stays 1.89: no std API newer than 1.89.
- CI: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --target <t> -- -D warnings` and `cargo test --workspace --no-fail-fast` on ubuntu, macos and windows, plus msrv 1.89 and `cargo audit --deny warnings`. All must stay green.
- Passing accept errors:
  - Unix: `EMFILE`, `ENFILE`, `ENOBUFS`, `ENOMEM`, `ECONNABORTED`, `EPROTO`, `EINTR`.
  - Windows: `ERROR_NOT_ENOUGH_MEMORY`, `ERROR_OUTOFMEMORY`, `ERROR_NO_SYSTEM_RESOURCES`, `ERROR_TOO_MANY_OPEN_FILES`.
- Accept backoff: starts at 10 ms, doubles, caps at 1 s; a success resets it. The first passing error of a run is logged at `warn`, and recovery logs one line with the count.
- Output queue: `OUTPUT_BUDGET` = 256 KiB waiting; `DRAIN_BUDGET` stays 128 KiB; the daemon `TICK` stays 8 ms.
- Ending a pane's tree on Unix, in order:
  1. `SIGTERM` the group while the leader is unreaped.
  2. Wait up to grace for the leader.
  3. `SIGKILL` the group if the leader ignored `SIGTERM`.
  4. Reap the leader.
  5. Wait out the rest of the grace for the group.
  6. `SIGKILL` if members remain.

  Nothing signals a group once a probe has found it gone (`ESRCH`).
- A new subagent (a pane with a `parent`) takes focus only when nothing is focused, and never clears the zoom.
- The socket file is removed on drop only if its device and inode are the ones recorded at bind.
- Comments explain why, in the surrounding voice.
- Commits are `type(scope): lowercase summary`, ending with the session's attribution trailer lines.

## Review Focus

1. **A pane that writes one byte per write** (a spinner, a progress bar) must not buffer unbounded tiny allocations. Each read is charged `READ_OVERHEAD` besides its bytes. Pinned in Task 2 by `a_flood_of_tiny_reads_is_held_to_the_budget_too`.
2. **A pane whose program ignores `SIGTERM`** must still end: `SIGKILL` after the grace, and its leader reaped. Pinned in Task 3 by `a_leader_that_ignores_sigterm_is_killed_and_reaped`.
3. **A long run of accept failures** must back off rather than spin a core. Pinned in Task 1 by `a_run_of_passing_failures_backs_off_rather_than_spinning`.
4. **A reattach that replays a parent and then its children** must leave focus on the parent. Pinned in Task 4 by `a_reattach_leaves_focus_on_the_parent_not_its_last_child`.
5. **Windows: a failure to create the next pipe instance** must still serve the client that connected, and the next accept makes the instance. This can't be injected in a test; it is for the reviewer to read, and Task 1 spells out the code.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/dispatch-os/src/ipc.rs` | Task 1: accept retries (`passes`, `accept_all` backoff, `bind_with`, Windows next-instance). Task 5: socket identity and removal on drop. Tests for both. |
| `crates/dispatch-pty/src/session.rs` + `session/tests.rs` | Task 2: `Backlog`, `OUTPUT_BUDGET`, `READ_OVERHEAD`, reader and drain. Task 3: `Pty` holds `Arc<Child>`; the waiter uses `wait_exit`; `terminate` uses `end_tree`. |
| `crates/dispatch-os/src/pty.rs` | Task 3: `Child::wait_exit`, `Child::end_tree`; the Unix `Child` keeps the leader unreaped. Tests. |
| `crates/dispatch-os/src/pty/windows.rs` | Task 3: the Windows `Child` gains `pid`, `wait_exit(&self)` and `end_tree`. |
| `crates/dispatch-os/src/process.rs` | Task 3: `signal_group`, `wait_for_group_to_exit` and `KILL_TIMEOUT` become `pub(crate)` for `pty.rs`. |
| `crates/dispatch-core/src/state.rs` | Task 4: `adopt_pane`'s focus rule. Tests. |
| `dispatch/src/app.rs` | Task 4: an app test (test module only). |
| `README.md` | Task 4: one sentence in `## Delegation`. |

---

### Task 1: The daemon keeps accepting after a passing error

**Files:**
- Modify: `crates/dispatch-os/src/ipc.rs`:
  - add `passes` and the backoff constants;
  - `accept_all` takes an accept step and backs off;
  - `Listener::bind_to` goes through a new private `bind_with`;
  - the Unix and Windows `imp` modules gain `passes`;
  - the Windows `imp::accept` handles a failed next instance;
  - tests in the file's `mod tests`.

**Interfaces:**
- Produces: `fn passes(error: &IpcError) -> bool` (private); `Listener::bind_with(path, accept)` (private). Task 5 edits `bind_with` and `Listener` again.

- [ ] **Step 1: Write the failing tests**

Add to `crates/dispatch-os/src/ipc.rs`'s `mod tests`, next to `dropping_a_listener_frees_its_endpoint`:

```rust
    /// An accept failure that passes: too many files open, for a moment.
    fn a_passing_failure() -> IpcError {
        #[cfg(unix)]
        let code = libc::EMFILE;
        #[cfg(windows)]
        let code = windows_sys::Win32::Foundation::ERROR_TOO_MANY_OPEN_FILES as i32;
        IpcError::io(
            "accepting a connection",
            std::io::Error::from_raw_os_error(code),
        )
    }

    /// An accept failure that does not pass: one with no OS code at all.
    fn a_fatal_failure() -> IpcError {
        IpcError::io(
            "accepting a connection",
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        )
    }

    /// A listener on this test's endpoint whose first accepts fail with
    /// `failures`, in order, before the platform's own accept takes over.
    fn listener_failing_with(failures: Vec<IpcError>) -> Listener {
        let mut failures = std::collections::VecDeque::from(failures);
        Listener::bind_with(&endpoint().expect("resolves"), move |inner| {
            failures.pop_front().map_or_else(|| imp::accept(inner), Err)
        })
        .expect("binding succeeds")
    }

    #[test]
    fn accepting_carries_on_after_a_failure_that_passes() {
        // A daemon that ran out of file descriptors for a moment must still
        // be reachable once they are back.
        let _guard = crate::env_lock();
        let _endpoint = Endpoint::new("passing");

        let listener = listener_failing_with(vec![a_passing_failure(), a_passing_failure()]);
        let server = std::thread::spawn(move || listener.accept());

        let _client = Connection::connect().expect("connecting succeeds");
        server
            .join()
            .expect("the server thread finishes")
            .expect("the connection after the failures is delivered");
    }

    #[test]
    fn a_failure_that_does_not_pass_ends_accepting() {
        let _guard = crate::env_lock();
        let _endpoint = Endpoint::new("fatal");

        let listener = listener_failing_with(vec![a_fatal_failure()]);

        assert!(
            matches!(listener.accept(), Err(IpcError::Io { .. })),
            "the failure is reported, not retried"
        );
    }

    #[test]
    fn a_run_of_passing_failures_backs_off_rather_than_spinning() {
        // Five failures in a row wait 10 + 20 + 40 + 80 + 160 ms between
        // them. A loop that retried at once would spin a core for as long as
        // the descriptors stayed short.
        let _guard = crate::env_lock();
        let _endpoint = Endpoint::new("backoff");

        let started = Instant::now();
        let listener = listener_failing_with((0..5).map(|_| a_passing_failure()).collect());
        let server = std::thread::spawn(move || listener.accept());
        let _client = Connection::connect().expect("connecting succeeds");
        server
            .join()
            .expect("the server thread finishes")
            .expect("the connection is delivered");

        assert!(
            started.elapsed() >= Duration::from_millis(300),
            "five failures were retried in {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn dropping_a_listener_that_is_backing_off_returns_promptly() {
        // Many failures push the backoff to its full second; a drop must not
        // wait one out.
        let _guard = crate::env_lock();
        let _endpoint = Endpoint::new("backoff-drop");

        let listener = listener_failing_with((0..1000).map(|_| a_passing_failure()).collect());
        std::thread::sleep(Duration::from_millis(1500));

        let (dropped, done) = std::sync::mpsc::channel();
        let started = Instant::now();
        std::thread::spawn(move || {
            drop(listener);
            let _ = dropped.send(());
        });

        assert!(
            done.recv_timeout(Duration::from_millis(500)).is_ok(),
            "the drop waited out a backoff"
        );
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn only_the_failures_that_pass_are_retried() {
        #[cfg(unix)]
        let (passing, fatal) = (
            vec![
                libc::EMFILE,
                libc::ENFILE,
                libc::ENOBUFS,
                libc::ENOMEM,
                libc::ECONNABORTED,
                libc::EPROTO,
                libc::EINTR,
            ],
            vec![libc::EBADF, libc::EINVAL, libc::ENOTSOCK],
        );
        #[cfg(windows)]
        let (passing, fatal) = {
            use windows_sys::Win32::Foundation::{
                ERROR_ACCESS_DENIED, ERROR_INVALID_HANDLE, ERROR_NO_SYSTEM_RESOURCES,
                ERROR_NOT_ENOUGH_MEMORY, ERROR_OUTOFMEMORY, ERROR_TOO_MANY_OPEN_FILES,
            };
            (
                vec![
                    ERROR_NOT_ENOUGH_MEMORY as i32,
                    ERROR_OUTOFMEMORY as i32,
                    ERROR_NO_SYSTEM_RESOURCES as i32,
                    ERROR_TOO_MANY_OPEN_FILES as i32,
                ],
                vec![ERROR_ACCESS_DENIED as i32, ERROR_INVALID_HANDLE as i32],
            )
        };

        let failure = |code| IpcError::io("accepting", std::io::Error::from_raw_os_error(code));
        for code in passing {
            assert!(passes(&failure(code)), "{code} passes");
        }
        for code in fatal {
            assert!(!passes(&failure(code)), "{code} does not pass");
        }
        assert!(!passes(&a_fatal_failure()), "an error with no OS code does not pass");
        assert!(
            !passes(&IpcError::NotRunning("x".into())),
            "only an I/O failure can pass"
        );
    }
```

If `Instant` or `Duration` is not yet imported in `mod tests`, add `use std::time::{Duration, Instant};` there.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p dispatch-os ipc::tests`
Expected: FAIL to compile, because `bind_with` and `passes` are not found.

- [ ] **Step 3: Implement**

(a) Beside `PREAMBLE_PATIENCE` near the top of `ipc.rs`, add:

```rust
/// How long accepting waits after a failure that passes before it tries
/// again, the first time. Each failure in a row doubles it, up to
/// [`ACCEPT_BACKOFF_MAX`].
const ACCEPT_BACKOFF_FIRST: Duration = Duration::from_millis(10);

/// The longest accepting waits between tries while failures keep coming.
const ACCEPT_BACKOFF_MAX: Duration = Duration::from_secs(1);

/// How long one slice of a backoff sleeps, so a listener being dropped is
/// never held up by a whole backoff.
const ACCEPT_BACKOFF_SLICE: Duration = Duration::from_millis(10);

/// Whether an accept failure passes: descriptors or memory short for a
/// moment, or a client that hung up while it was being accepted.
///
/// Accepting waits and tries again after one of these. Anything else is the
/// listener's end, reported once. Ending on a passing failure left a daemon
/// that ran short of descriptors for a moment running its panes with no way
/// for a client to reach them.
fn passes(error: &IpcError) -> bool {
    match error {
        IpcError::Io { source, .. } => imp::passes(source),
        _ => false,
    }
}
```

If `Duration` is not imported at the top of `ipc.rs`, add it to the `std::time` import.

(b) Replace `Listener::bind_to` with this, and add `bind_with` after it:

```rust
    pub fn bind_to(path: &Path) -> Result<Self, IpcError> {
        Self::bind_with(path, imp::accept)
    }

    /// [`Listener::bind_to`], taking each stream through `accept`: a test
    /// passes one that fails on cue before handing over to the platform's.
    fn bind_with(
        path: &Path,
        accept: impl FnMut(&imp::Listener) -> Result<imp::Stream, IpcError> + Send + 'static,
    ) -> Result<Self, IpcError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| IpcError::io(format!("creating {}", parent.display()), e))?;
        }

        let inner = imp::bind(path)?;
        let (sender, paired) = channel();
        let stopping = Arc::new(AtomicBool::new(false));

        let accepting = {
            let stopping = Arc::clone(&stopping);
            std::thread::spawn(move || accept_all(&inner, accept, &sender, &stopping))
        };

        Ok(Self {
            paired: Mutex::new(paired),
            stopping,
            endpoint: path.to_path_buf(),
            accepting: Some(accepting),
        })
    }
```

Keep `bind_to`'s existing doc comment on `bind_to`.

(c) Replace the top of `accept_all`, from its doc comment to the `let stream = match … };` statement, with the following. The rest of the loop body, from `if stopping.load(Ordering::Relaxed) {` onward, stays exactly as it is:

```rust
/// Accepts until the listener fails for good or is dropped, reading each
/// preamble on a thread of its own and handing on each connection once both
/// of its halves are in.
///
/// A failure that [`passes`] is waited out, backing off from
/// [`ACCEPT_BACKOFF_FIRST`] to [`ACCEPT_BACKOFF_MAX`]. It is logged once per
/// run rather than once per try, and the recovery says how many there were.
fn accept_all(
    inner: &imp::Listener,
    mut accept: impl FnMut(&imp::Listener) -> Result<imp::Stream, IpcError>,
    paired: &Sender<Result<Connection, IpcError>>,
    stopping: &AtomicBool,
) {
    let halves = Arc::new(Mutex::new(pairing::Halves::new()));
    let announcing = Arc::new(AtomicUsize::new(0));
    let mut backoff = ACCEPT_BACKOFF_FIRST;
    let mut failures = 0usize;

    loop {
        let stream = match accept(inner) {
            Ok(stream) => {
                if failures > 0 {
                    tracing::info!(failures, "accepting connections again");
                }
                failures = 0;
                backoff = ACCEPT_BACKOFF_FIRST;
                stream
            }
            Err(error) if passes(&error) => {
                if failures == 0 {
                    tracing::warn!(%error, "accepting failed for now; trying again");
                }
                failures += 1;
                if !wait_unless_stopping(backoff, stopping) {
                    return;
                }
                backoff = (backoff * 2).min(ACCEPT_BACKOFF_MAX);
                continue;
            }
            Err(error) => {
                let _ = paired.send(Err(error));
                return;
            }
        };
```

Then add after `accept_all`:

```rust
/// Sleeps for `wait` a slice at a time. Returns false as soon as `stopping`
/// is set.
fn wait_unless_stopping(wait: Duration, stopping: &AtomicBool) -> bool {
    let deadline = Instant::now() + wait;
    loop {
        if stopping.load(Ordering::Relaxed) {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        std::thread::sleep((deadline - now).min(ACCEPT_BACKOFF_SLICE));
    }
}
```

(d) In the Unix `mod imp`, after `accept`:

```rust
    /// Whether a failed accept is worth trying again: short of descriptors,
    /// buffers or memory for a moment, or a client gone before it was taken.
    pub(super) fn passes(error: &std::io::Error) -> bool {
        matches!(
            error.raw_os_error(),
            Some(
                libc::EMFILE
                    | libc::ENFILE
                    | libc::ENOBUFS
                    | libc::ENOMEM
                    | libc::ECONNABORTED
                    | libc::EPROTO
                    | libc::EINTR
            )
        )
    }
```

(e) In the Windows `mod imp`, after `accept`:

```rust
    /// Whether a failed accept is worth trying again: memory, system
    /// resources or handles short for a moment.
    pub(super) fn passes(error: &std::io::Error) -> bool {
        use windows_sys::Win32::Foundation::{
            ERROR_NO_SYSTEM_RESOURCES, ERROR_NOT_ENOUGH_MEMORY, ERROR_OUTOFMEMORY,
            ERROR_TOO_MANY_OPEN_FILES,
        };

        error.raw_os_error().is_some_and(|code| {
            [
                ERROR_NOT_ENOUGH_MEMORY,
                ERROR_OUTOFMEMORY,
                ERROR_NO_SYSTEM_RESOURCES,
                ERROR_TOO_MANY_OPEN_FILES,
            ]
            .iter()
            .any(|&known| code == known as i32)
        })
    }
```

(f) In the Windows `imp::accept`, make a missing instance this accept's first job, and let a failed next instance leave the connected client served. Replace

```rust
        let mut pending = listener.pending.lock().unwrap_or_else(|e| e.into_inner());

        let handle = *pending;
```

with

```rust
        let mut pending = listener.pending.lock().unwrap_or_else(|e| e.into_inner());

        // The last accept could not open the next instance, so none is
        // waiting. Opening it is this accept's first job, and a failure here
        // is reported like any other, to be waited out if it passes.
        if *pending == INVALID_HANDLE_VALUE as isize {
            *pending = create_instance(&listener.name, false, &listener.security)
                .map_err(|e| IpcError::io(format!("reopening {}", listener.name), e))?;
        }

        let handle = *pending;
```

and replace

```rust
        *pending = create_instance(&listener.name, false, &listener.security)
            .map_err(|e| IpcError::io(format!("reopening {}", listener.name), e))?;
```

with

```rust
        // A failure here is not this client's: it has connected and is
        // served. The next accept opens the instance, and backs off if it
        // still cannot. Returning the error instead would leave this handle
        // connected and still pending, to be handed out a second time.
        *pending = match create_instance(&listener.name, false, &listener.security) {
            Ok(next) => next,
            Err(error) => {
                tracing::warn!(%error, "could not open the next pipe instance yet");
                INVALID_HANDLE_VALUE as isize
            }
        };
```

The Windows `Listener`'s `Drop` already skips `INVALID_HANDLE_VALUE`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p dispatch-os ipc::tests`
Expected: PASS, including the 5 new tests. Also run `cargo clippy -p dispatch-os --all-targets --target x86_64-pc-windows-gnu -- -D warnings` for the Windows half.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`

```bash
git add crates/dispatch-os/src/ipc.rs
git commit -m "fix(os): keep accepting after a failure that passes"
```

---

### Task 2: Pane output bounded by bytes, not by reads

**Files:**
- Modify: `crates/dispatch-pty/src/session.rs`:
  - `OUTPUT_CHUNKS` goes, replaced by `OUTPUT_BUDGET` and `READ_OVERHEAD`;
  - new `Backlog`;
  - `Drained.reads`;
  - `Pty` gains `backlog`;
  - `spawn`, `drain`, `drain_until_exit`, `Drop`, `spawn_reader` and `spawn_waiter` change.
- Modify: `crates/dispatch-pty/src/session/tests.rs`

**Interfaces:**
- Produces: `spawn_reader(reader, tx: Sender<PtyEvent>, backlog: Arc<Backlog>) -> JoinHandle<()>`; `spawn_waiter(child, tx: Sender<PtyEvent>)`. Task 3 changes `spawn_waiter`'s first parameter to `Arc<dispatch_os::pty::Child>`.

- [ ] **Step 1: Write the failing tests**

In `crates/dispatch-pty/src/session/tests.rs`, change `a_drain_stops_at_its_budget_and_the_next_takes_the_rest` to build its channel with `channel()` rather than `sync_channel(OUTPUT_CHUNKS)`, and to fill it with `tx.send(...)` rather than `tx.try_send(...)`. Keep the `.expect(...)` messages. Then append:

```rust
/// A reader that hands out `left` reads of `size` bytes each, then ends.
struct Reads {
    left: usize,
    size: usize,
}

impl Read for Reads {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.left == 0 {
            return Ok(0);
        }
        self.left -= 1;
        let n = self.size.min(buf.len());
        buf[..n].fill(b'x');
        Ok(n)
    }
}

/// Waits until `done` holds, or fails the test after five seconds.
fn eventually(what: &str, done: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_program_writing_a_line_at_a_time_is_not_held_to_a_few_lines_a_drain() {
    // A line is a small read. Counting reads rather than bytes let 32
    // through per drain, about 180 KB/s at the daemon's tick.
    const READS: usize = 1000;
    const LINE: usize = 50;

    let (tx, events) = channel();
    let backlog = Arc::new(Backlog::default());
    let reader = spawn_reader(
        Box::new(Reads {
            left: READS,
            size: LINE,
        }),
        tx,
        Arc::clone(&backlog),
    );

    eventually("the reader stopped before anything drained it", || {
        reader.is_finished()
    });
    let drained = drain_from(&events, DRAIN_BUDGET);
    assert_eq!(
        drained.output.len(),
        READS * LINE,
        "one drain takes every line"
    );
}

#[test]
fn a_reader_waits_once_the_budget_is_waiting_and_goes_on_after_a_drain() {
    const READ: usize = 8192;
    let reads = 4 * OUTPUT_BUDGET / READ;

    let (tx, events) = channel();
    let backlog = Arc::new(Backlog::default());
    let reader = spawn_reader(
        Box::new(Reads {
            left: reads,
            size: READ,
        }),
        tx,
        Arc::clone(&backlog),
    );

    eventually("the reader never filled the budget", || {
        backlog.waiting() > OUTPUT_BUDGET
    });
    std::thread::sleep(Duration::from_millis(100));
    assert!(!reader.is_finished(), "the reader waits for room");
    assert!(
        backlog.waiting() <= OUTPUT_BUDGET + READ + READ_OVERHEAD,
        "it stopped just past the budget, at {}",
        backlog.waiting()
    );

    let mut received = 0;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while received < reads * READ {
        assert!(std::time::Instant::now() < deadline, "only {received} arrived");
        let drained = drain_from(&events, DRAIN_BUDGET);
        backlog.drained(&drained);
        received += drained.output.len();
        std::thread::sleep(Duration::from_millis(1));
    }
    eventually("the reader never finished", || reader.is_finished());
}

#[test]
fn a_flood_of_tiny_reads_is_held_to_the_budget_too() {
    // A spinner writing a byte at a time costs more per read than its one
    // byte: each read is a Vec and a place in the channel. Charged only its
    // bytes, a flood of them would pile up far past what the budget means.
    let (tx, events) = channel();
    let backlog = Arc::new(Backlog::default());
    let reader = spawn_reader(
        Box::new(Reads {
            left: 1_000_000,
            size: 1,
        }),
        tx,
        Arc::clone(&backlog),
    );

    eventually("the reader never filled the budget", || {
        backlog.waiting() > OUTPUT_BUDGET
    });
    std::thread::sleep(Duration::from_millis(100));
    let queued = events.try_iter().count();
    assert!(
        queued <= OUTPUT_BUDGET / (1 + READ_OVERHEAD) + 1,
        "{queued} one-byte reads were queued"
    );
    drop(events);
    backlog.close();
    eventually("the reader never let go", || reader.is_finished());
}

#[test]
fn a_reader_waiting_for_room_lets_go_once_nothing_drains() {
    // A pane dropped while its reader waits for room must not leave the
    // reader waiting for a drain that will never come.
    let (tx, events) = channel();
    let backlog = Arc::new(Backlog::default());
    let reader = spawn_reader(
        Box::new(Reads {
            left: 4 * OUTPUT_BUDGET / 8192,
            size: 8192,
        }),
        tx,
        Arc::clone(&backlog),
    );

    eventually("the reader never filled the budget", || {
        backlog.waiting() > OUTPUT_BUDGET
    });
    drop(events);
    backlog.close();

    eventually("the reader kept waiting after the pane went", || {
        reader.is_finished()
    });
}

#[test]
fn an_exit_sent_after_output_is_taken_with_it() {
    let (tx, events) = channel();
    tx.send(PtyEvent::Output(b"last words".to_vec()))
        .expect("the channel is open");
    tx.send(PtyEvent::Exited(3)).expect("the channel is open");

    let drained = drain_from(&events, DRAIN_BUDGET);
    assert_eq!(drained.output, b"last words");
    assert_eq!(drained.exited, Some(3));
}
```

If `Read`, `Arc` or `channel` is not reachable through `use super::*;`, add `use std::io::Read; use std::sync::Arc; use std::sync::mpsc::channel;` at the top of the tests file.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p dispatch-pty`
Expected: FAIL to compile, because `Backlog`, `OUTPUT_BUDGET` and `READ_OVERHEAD` are not found and `spawn_reader` takes other arguments.

- [ ] **Step 3: Implement**

In `crates/dispatch-pty/src/session.rs`:

(a) Imports: drop `SyncSender` and `sync_channel` from the `std::sync::mpsc` import, and add `Condvar` and `Mutex` to `std::sync`'s.

(b) Replace `OUTPUT_CHUNKS` and its doc comment with:

```rust
/// How much output may wait between the reader thread and `drain`.
///
/// Past it, the reader stops reading, the pseudoterminal's own buffer fills,
/// and the child blocks on its next write: a pane that prints faster than it
/// is drawn is slowed down, not held in memory. Counted in bytes rather than
/// reads, so a program that writes a line at a time is drained as fast as one
/// that writes in blocks.
const OUTPUT_BUDGET: usize = 256 * 1024;

/// What one read costs beyond its bytes: its `Vec` and its place in the
/// channel.
///
/// Charged against [`OUTPUT_BUDGET`] with every read, so a flood of one-byte
/// reads is held to the budget too.
const READ_OVERHEAD: usize = 64;
```

(c) After `enum PtyEvent`, add:

```rust
/// Output that has been read and not yet drained, weighed in bytes plus
/// [`READ_OVERHEAD`] per read.
///
/// The reader counts a read in the same critical section it sends it in,
/// and a drain counts what it took under the same lock. So a drain can never
/// take bytes that were not counted yet, and leave the count too high for
/// good.
#[derive(Default)]
struct Backlog {
    state: Mutex<Waiting>,
    room: Condvar,
}

#[derive(Default)]
struct Waiting {
    weight: usize,
    /// Nothing will drain again: the pane has gone.
    closed: bool,
}

impl Backlog {
    /// Sends `bytes` on and counts them as waiting, then waits while more
    /// than `budget` is. Returns whether anything still drains.
    fn send(&self, tx: &Sender<PtyEvent>, bytes: Vec<u8>, budget: usize) -> bool {
        let mut waiting = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let weight = bytes.len() + READ_OVERHEAD;
        if waiting.closed || tx.send(PtyEvent::Output(bytes)).is_err() {
            return false;
        }
        waiting.weight += weight;
        while waiting.weight > budget && !waiting.closed {
            waiting = self.room.wait(waiting).unwrap_or_else(|e| e.into_inner());
        }
        !waiting.closed
    }

    /// Counts what one drain took as gone, and wakes a reader waiting for
    /// room.
    fn drained(&self, drained: &Drained) {
        if drained.reads == 0 {
            return;
        }
        let mut waiting = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let weight = drained.output.len() + drained.reads * READ_OVERHEAD;
        waiting.weight = waiting.weight.saturating_sub(weight);
        self.room.notify_all();
    }

    /// Counts one read of `len` bytes as gone, for a caller that takes
    /// events one at a time rather than through [`drain_from`].
    fn took(&self, len: usize) {
        let mut waiting = self.state.lock().unwrap_or_else(|e| e.into_inner());
        waiting.weight = waiting.weight.saturating_sub(len + READ_OVERHEAD);
        self.room.notify_all();
    }

    /// Nothing drains any more. A reader waiting for room is woken, and
    /// never waits again.
    fn close(&self) {
        let mut waiting = self.state.lock().unwrap_or_else(|e| e.into_inner());
        waiting.closed = true;
        self.room.notify_all();
    }

    /// How much is waiting now, weighed as the budget weighs it. For tests.
    #[cfg(test)]
    fn waiting(&self) -> usize {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).weight
    }
}
```

(d) `struct Drained`: add a field after `output`:

```rust
    /// How many reads `output` was made of.
    reads: usize,
```

In `drain_from`, initialise `reads: 0`, and in the `Ok(PtyEvent::Output(bytes))` arm add `drained.reads += 1;` beside the `extend_from_slice`.

(e) `struct Pty`: change the `events` field's doc comment to:

```rust
    /// What the reader and the waiter report, bounded by `backlog` so a pane
    /// printing faster than it is drained is slowed rather than stored.
```

and add a field after it:

```rust
    /// How much of `events` is output not yet drained.
    backlog: Arc<Backlog>,
```

(f) `Pty::spawn`: replace

```rust
        let (tx, events) = sync_channel(OUTPUT_CHUNKS);
        spawn_reader(process.reader, tx.clone());
        spawn_waiter(process.child, tx);
```

with

```rust
        let (tx, events) = channel();
        let backlog = Arc::new(Backlog::default());
        spawn_reader(process.reader, tx.clone(), Arc::clone(&backlog));
        spawn_waiter(process.child, tx);
```

and add `backlog,` to the `Ok(Self { … })` after `events,`.

(g) `Pty::drain`: after `let drained = drain_from(&self.events, DRAIN_BUDGET);` add `self.backlog.drained(&drained);`.

(h) `Pty::drain_until_exit` takes events one at a time. Count each read it takes as gone:
- in the `Ok(PtyEvent::Output(bytes)) =>` arm, call `self.backlog.took(bytes.len());` before `output.extend_from_slice(&bytes)`;
- in the inner `while let Ok(PtyEvent::Output(bytes)) = …` loop body, do the same before its `extend_from_slice`.

(i) `impl Drop for Pty`: before `self.terminate();` add

```rust
        // Nothing drains from here on; a reader waiting for room must not
        // wait for good.
        self.backlog.close();
```

(j) Replace `spawn_reader` (keeping its doc comment, with one sentence added) with:

```rust
/// Reads the pseudoterminal until end-of-file, forwarding bytes.
///
/// Once the pane has gone, what happens depends on the platform (see
/// [`dispatch_os::pty::OUTPUT_OUTLIVES_ITS_READER`]). On Windows it reads
/// on, dropping what arrives: the pseudoconsole only finishes closing once
/// its output pipe is drained, so a reader that stopped would leave that
/// close waiting forever. On Unix it stops, letting go of its end of the
/// terminal: reading on would keep that end open for as long as something
/// that outlived the pane still holds the other.
///
/// Waits whenever more than [`OUTPUT_BUDGET`] is waiting to be drained.
fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    tx: Sender<PtyEvent>,
    backlog: Arc<Backlog>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        // Large enough that a burst of output is a few reads rather than
        // hundreds, small enough not to sit idle holding memory per pane.
        let mut buf = [0u8; 8192];
        let mut delivering = true;

        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if delivering && !backlog.send(&tx, buf[..n].to_vec(), OUTPUT_BUDGET) {
                        if !dispatch_os::pty::OUTPUT_OUTLIVES_ITS_READER {
                            break;
                        }
                        delivering = false;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    })
}
```

(k) `spawn_waiter`: change its `tx: SyncSender<PtyEvent>` parameter to `tx: Sender<PtyEvent>`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p dispatch-pty && cargo test -p dispatch-daemon`
Expected: PASS, including the 5 new tests and every existing pty test (the flood test, exit tests, tree tests).

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo clippy -p dispatch-pty --all-targets --target x86_64-pc-windows-gnu -- -D warnings`

```bash
git add crates/dispatch-pty/src/session.rs crates/dispatch-pty/src/session/tests.rs
git commit -m "fix(pty): bound a pane's waiting output by bytes, not by reads"
```

---

### Task 3: An exited pane keeps its process group's number until it is closed

**Files:**
- Modify: `crates/dispatch-os/src/process.rs`: `KILL_TIMEOUT`, `signal_group` and `wait_for_group_to_exit` become `pub(crate)` in the Unix `imp`, and are re-exported.
- Modify: `crates/dispatch-os/src/pty.rs`:
  - public `Child` gets `wait_exit(&self)` and `end_tree(&self, grace)`, replacing `wait(self)`;
  - the Unix `imp::Child` is rewritten;
  - Unix tests are added.
- Modify: `crates/dispatch-os/src/pty/windows.rs`: `Child` carries its `pid`, with `wait_exit(&self)` and `end_tree`.
- Modify: `crates/dispatch-pty/src/session.rs`: `Pty` holds `Arc<Child>`; the waiter calls `wait_exit`; `terminate` calls `end_tree`.
- Modify: `crates/dispatch-pty/src/session/tests.rs`

**Interfaces:**
- Consumes: Task 2's `spawn_waiter(child, tx: Sender<PtyEvent>)`.
- Produces:
  - `dispatch_os::pty::Child::wait_exit(&self) -> i32`;
  - `dispatch_os::pty::Child::end_tree(&self, grace: Duration) -> Result<(), dispatch_os::process::ProcessError>`, where only the first call acts;
  - `Child: Send + Sync`.

- [ ] **Step 1: Write the failing tests**

(a) In `crates/dispatch-os/src/pty.rs`'s `mod tests`, add these Unix tests. Keep the file's existing imports, and add `use std::time::{Duration, Instant};` if it is missing:

```rust
    /// `sh -c script` in a pseudoterminal, nothing set or removed.
    #[cfg(unix)]
    fn sh(script: &str) -> super::PtyProcess {
        let args = vec!["-c".to_string(), script.to_string()];
        spawn(
            &PtyCommand {
                program: "sh",
                args: &args,
                env: &BTreeMap::new(),
                env_remove: &BTreeSet::new(),
                cwd: &std::env::temp_dir(),
            },
            24,
            80,
        )
        .expect("sh starts")
    }

    /// The first line `reader` prints, within five seconds.
    #[cfg(unix)]
    fn first_line(mut reader: Box<dyn std::io::Read + Send>) -> String {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut seen = Vec::new();
            let mut byte = [0u8; 1];
            while reader.read(&mut byte).unwrap_or(0) == 1 {
                if byte[0] == b'\n' {
                    break;
                }
                seen.push(byte[0]);
            }
            let _ = tx.send(String::from_utf8_lossy(&seen).trim().to_string());
        });
        rx.recv_timeout(Duration::from_secs(5))
            .expect("the script printed a line")
    }

    #[test]
    #[cfg(unix)]
    fn an_exited_leader_is_kept_unreaped_until_its_tree_is_ended() {
        // Reaped at once, its pid -- its process group's id -- would be free
        // for another process, and ending the group by it later would reach a
        // stranger's.
        let process = sh("exit 3");
        let pid = libc::pid_t::try_from(process.pid.expect("a pid")).expect("fits");

        assert_eq!(process.child.wait_exit(), 3);
        assert_eq!(
            super::imp::wait_without_reaping(pid, false).expect("still this process's child"),
            Some(3),
            "the leader is kept, unreaped"
        );

        process
            .child
            .end_tree(Duration::from_millis(250))
            .expect("ending succeeds");
        assert!(
            super::imp::wait_without_reaping(pid, false).is_err(),
            "and reaped once its tree is ended"
        );
    }

    #[test]
    #[cfg(unix)]
    fn ending_an_exited_panes_tree_reaches_what_it_left_and_returns_promptly() {
        // The unreaped leader still counts as a member of its group. Were it
        // not reaped before the wait for the group, every close would wait out
        // the grace and the kill timeout too.
        let process = sh("trap '' HUP; sleep 30 & echo $!; exit 0");
        let left: u32 = first_line(process.reader)
            .parse()
            .expect("the script printed the sleep's pid");
        assert_eq!(process.child.wait_exit(), 0);
        assert!(crate::process::is_running(left), "the sleep outlived its shell");

        let started = Instant::now();
        process
            .child
            .end_tree(Duration::from_millis(250))
            .expect("ending succeeds");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "ending took {:?}",
            started.elapsed()
        );

        let deadline = Instant::now() + Duration::from_secs(5);
        while crate::process::is_running(left) {
            assert!(Instant::now() < deadline, "what the pane left outlived it");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    #[cfg(unix)]
    fn a_leader_that_ignores_sigterm_is_killed_and_reaped() {
        let process = sh("trap '' TERM; echo ready; sleep 30");
        let pid = libc::pid_t::try_from(process.pid.expect("a pid")).expect("fits");
        assert_eq!(first_line(process.reader), "ready");

        process
            .child
            .end_tree(Duration::from_millis(250))
            .expect("ending succeeds");

        assert!(
            super::imp::wait_without_reaping(pid, false).is_err(),
            "killed after the grace, and reaped"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_waiter_parked_on_the_exit_hears_it_when_the_tree_is_ended() {
        // The pane's waiter thread is parked on the exit while a close ends
        // the tree and reaps the leader under it. It still hears how the
        // process ended, once.
        let process = sh("sleep 30");
        let child = std::sync::Arc::new(process.child);
        let waiter = {
            let child = std::sync::Arc::clone(&child);
            std::thread::spawn(move || child.wait_exit())
        };
        std::thread::sleep(Duration::from_millis(100));

        child
            .end_tree(Duration::from_millis(250))
            .expect("ending succeeds");

        let deadline = Instant::now() + Duration::from_secs(5);
        while !waiter.is_finished() {
            assert!(Instant::now() < deadline, "the waiter never heard the exit");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_ne!(
            waiter.join().expect("the waiter finishes"),
            0,
            "a killed process never reads as a clean exit"
        );
    }
```

(b) In `crates/dispatch-pty/src/session/tests.rs`, extend `dropping_a_pane_that_has_exited_ends_what_it_left_running`. After its final assert, add:

```rust
    // The shell itself was kept unreaped until the drop, and the drop let it
    // go.
    #[cfg(unix)]
    {
        let deadline = std::time::Instant::now() + TIMEOUT;
        while dispatch_os::process::is_running(pid) {
            assert!(
                std::time::Instant::now() < deadline,
                "the pane's shell was never reaped"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p dispatch-os pty::tests`
Expected: FAIL to compile, because `wait_exit`, `end_tree` and `imp::wait_without_reaping` are not found.

- [ ] **Step 3: Make the group helpers reachable**

In `crates/dispatch-os/src/process.rs`'s Unix `mod imp`, change these to `pub(crate)`: `const KILL_TIMEOUT`, `fn signal_group` and `fn wait_for_group_to_exit`. Then add at the top level, beside `#[cfg(windows)] pub(crate) use imp::contain;`:

```rust
/// Signalling and waiting on a process group by its leader's pid, for a
/// pane's tree: see `pty::Child::end_tree`, which keeps that pid its own
/// until it is done.
#[cfg(unix)]
pub(crate) use imp::{KILL_TIMEOUT, signal_group, wait_for_group_to_exit};
```

- [ ] **Step 4: The public `Child`**

In `crates/dispatch-os/src/pty.rs`, replace the `Child` doc comment and its `impl` with:

```rust
/// A pane's process, shared between whatever waits for it and whatever ends
/// it.
///
/// On Unix the process is not reaped when it exits. It is left a zombie
/// until [`Child::end_tree`], so its pid, which is also its process group's
/// id, cannot be given to another process first. Ending the group by that id
/// long after the exit then reaches only what the pane started. The cost is
/// one zombie per pane that has exited and not yet been closed.
pub struct Child(imp::Child);

impl Child {
    /// Waits for the process to exit, and says how, without letting its pid
    /// go.
    ///
    /// 0 for success, the process's code otherwise, and 1 for a failure
    /// that carried no code: a failed exit must never read as a clean one.
    /// Safe to call from one thread while another ends the tree.
    #[must_use]
    pub fn wait_exit(&self) -> i32 {
        self.0.wait_exit()
    }

    /// Ends the process and everything in its tree, then lets its pid go.
    ///
    /// Only the first call does anything: once the pid has been let go, a
    /// second ending by it could reach another process.
    pub fn end_tree(&self, grace: std::time::Duration) -> Result<(), crate::process::ProcessError> {
        self.0.end_tree(grace)
    }
}
```

Leave the existing `impl std::fmt::Debug for Child` as it is.

- [ ] **Step 5: The Unix `imp::Child`**

In the Unix `mod imp` of `pty.rs`, replace `pub(super) struct Child(…)` and its `impl` with:

```rust
    /// A pane's process, kept unreaped until its tree is ended.
    pub(super) struct Child {
        pid: libc::pid_t,
        /// `portable-pty`'s handle, through which the process is finally
        /// reaped. Taken when it is.
        handle: Mutex<Option<Box<dyn portable_pty::Child + Send + Sync>>>,
        /// How the process exited, once anything has seen it.
        exit: Mutex<Option<i32>>,
        /// Whether the tree has been ended.
        ended: AtomicBool,
    }

    impl Child {
        fn new(handle: Box<dyn portable_pty::Child + Send + Sync>) -> std::io::Result<Self> {
            let pid = handle
                .process_id()
                .and_then(|pid| libc::pid_t::try_from(pid).ok())
                .ok_or_else(|| std::io::Error::other("the pane's process has no pid"))?;
            Ok(Self {
                pid,
                handle: Mutex::new(Some(handle)),
                exit: Mutex::new(None),
                ended: AtomicBool::new(false),
            })
        }

        fn seen(&self) -> Option<i32> {
            *self.exit.lock().unwrap_or_else(|e| e.into_inner())
        }

        fn record(&self, code: i32) {
            *self.exit.lock().unwrap_or_else(|e| e.into_inner()) = Some(code);
        }

        pub(super) fn wait_exit(&self) -> i32 {
            loop {
                if let Some(code) = self.seen() {
                    return code;
                }
                match wait_without_reaping(self.pid, true) {
                    Ok(Some(code)) => {
                        self.record(code);
                        return code;
                    }
                    Ok(None) => {}
                    Err(error) if error.raw_os_error() == Some(libc::EINTR) => {}
                    // Reaped under this wait by `end_tree`, which recorded
                    // the exit before it reaped.
                    Err(_) => return self.seen().unwrap_or(1),
                }
            }
        }

        /// Polls for the exit until `deadline`, without reaping. Returns
        /// whether the process has exited.
        fn exited_by(&self, deadline: Instant) -> bool {
            loop {
                if self.seen().is_some() {
                    return true;
                }
                if let Ok(Some(code)) = wait_without_reaping(self.pid, false) {
                    self.record(code);
                    return true;
                }
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        /// Reaps the process, once it is known to have exited. One stuck in
        /// the kernel is left alone rather than waited on for good.
        fn reap(&self) {
            if self.seen().is_none() {
                return;
            }
            if let Some(mut handle) = self.handle.lock().unwrap_or_else(|e| e.into_inner()).take() {
                let _ = handle.wait();
            }
        }

        pub(super) fn end_tree(&self, grace: Duration) -> Result<(), ProcessError> {
            if self.ended.swap(true, Ordering::AcqRel) {
                return Ok(());
            }
            let pid = self.pid.unsigned_abs();
            let map = |source| ProcessError::Terminate { pid, source };
            let deadline = Instant::now() + grace;

            // The leader is unreaped, so its pid is still its group's id and
            // the group is certainly this pane's.
            signal_group(pid, libc::SIGTERM).map_err(map)?;
            if !self.exited_by(deadline) {
                signal_group(pid, libc::SIGKILL).map_err(map)?;
                self.exited_by(Instant::now() + KILL_TIMEOUT);
            }

            // Reaped before the group is waited on: an unreaped leader counts
            // as a member, and the wait would run out every time. From here
            // the group's id is held by whatever is left in it, and once a
            // probe finds it gone nothing signals it again. The one window
            // left is between a probe that found members and the kill straight
            // after it.
            self.reap();
            let rest = deadline.saturating_duration_since(Instant::now());
            if !wait_for_group_to_exit(pid, rest) && signal_group(pid, libc::SIGKILL).map_err(map)? {
                wait_for_group_to_exit(pid, KILL_TIMEOUT);
            }
            Ok(())
        }
    }

    impl Drop for Child {
        fn drop(&mut self) {
            // A child whose tree was never ended is reaped if it has exited;
            // one still running is left to whoever else holds it.
            if let Some(mut handle) = self.handle.lock().unwrap_or_else(|e| e.into_inner()).take() {
                let _ = handle.try_wait();
            }
        }
    }

    /// Waits for `pid` to exit without reaping it, or with `block` false
    /// looks once. Returns how it exited, once it has.
    pub(super) fn wait_without_reaping(
        pid: libc::pid_t,
        block: bool,
    ) -> std::io::Result<Option<i32>> {
        // SAFETY: an all-zero siginfo_t is a valid place for waitid to write.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let mut options = libc::WEXITED | libc::WNOWAIT;
        if !block {
            options |= libc::WNOHANG;
        }
        // SAFETY: P_PID with this process's own child's pid, and `info` is a
        // valid siginfo_t to write to.
        let result = unsafe { libc::waitid(libc::P_PID, pid.unsigned_abs(), &mut info, options) };
        if result != 0 {
            return Err(std::io::Error::last_os_error());
        }

        // With WNOHANG and nothing to report, waitid leaves the pid zero.
        // SAFETY: waitid filled `info` in for a child event.
        let (who, code, status) = unsafe { (info.si_pid(), info.si_code, info.si_status()) };
        if who == 0 {
            return Ok(None);
        }
        Ok(Some(if code == libc::CLD_EXITED { status } else { 1 }))
    }
```

Add these imports to the Unix `mod imp`:

```rust
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use crate::process::{KILL_TIMEOUT, ProcessError, signal_group, wait_for_group_to_exit};
```

In `imp::spawn`, change `child: super::Child(Child(child)),` to `child: super::Child(Child::new(child)?),`.

About the accessors: `libc::siginfo_t` has `si_pid()` and `si_status()` methods on Linux. If the macOS target's `siginfo_t` lacks them, read its public `si_pid` and `si_status` fields there instead. `cargo clippy --target aarch64-apple-darwin` tells you which. A `#[cfg(target_os = "macos")]` split inside this module is allowed.

- [ ] **Step 6: The Windows `imp::Child`**

In `crates/dispatch-os/src/pty/windows.rs`, replace `pub(super) struct Child(OwnedHandle);` and its `impl` with:

```rust
/// A pane's process. Its job ends its tree; its handle keeps its pid its own
/// until the handle is closed.
pub(super) struct Child {
    handle: OwnedHandle,
    pid: u32,
    ended: std::sync::atomic::AtomicBool,
}

impl Child {
    pub(super) fn wait_exit(&self) -> i32 {
        let handle = self.handle.as_raw_handle() as HANDLE;
        // SAFETY: a live process handle; waiting on one is always defined.
        unsafe { WaitForSingleObject(handle, INFINITE) };

        let mut code = 0u32;
        // SAFETY: a live process handle and a place for its code.
        if unsafe { GetExitCodeProcess(handle, &mut code) } == 0 {
            return 1;
        }
        if code == 0 {
            0
        } else {
            i32::try_from(code).unwrap_or(1)
        }
    }

    pub(super) fn end_tree(
        &self,
        grace: std::time::Duration,
    ) -> Result<(), crate::process::ProcessError> {
        if self.ended.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return Ok(());
        }
        crate::process::terminate_tree(self.pid, grace)
    }
}
```

Where `spawn` builds `child: super::Child(Child(process)),`, build `child: super::Child(Child { handle: process, pid, ended: std::sync::atomic::AtomicBool::new(false) }),` using the `pid` it already has.

- [ ] **Step 7: `dispatch-pty` uses them**

In `crates/dispatch-pty/src/session.rs`:
- `struct Pty`: add a field after `pid`:

  ```rust
      /// The process, shared with the waiter thread, which is what ends the
      /// tree.
      child: Arc<dispatch_os::pty::Child>,
  ```

- `Pty::spawn`: replace `spawn_waiter(process.child, tx);` with

  ```rust
          let child = Arc::new(process.child);
          spawn_waiter(Arc::clone(&child), tx);
  ```

  and add `child,` to `Ok(Self { … })`.
- `Pty::terminate`: replace its body after `let Some(pid) = self.pid.take() else { return; };` with

  ```rust
          if let Err(error) = self.child.end_tree(dispatch_os::process::DEFAULT_GRACE) {
              tracing::warn!(%pid, %error, "failed to terminate the pane's process tree");
          }
  ```

  and change the doc comment's last paragraph to: "Only the first call does anything. On Unix the pane's process is kept unreaped until here, so its pid cannot be given to another process before the tree is ended by it."
- `spawn_waiter`:

  ```rust
  /// Waits for the child and reports its exit status, leaving it unreaped for
  /// the pane's close.
  fn spawn_waiter(child: Arc<dispatch_os::pty::Child>, tx: Sender<PtyEvent>) {
      std::thread::spawn(move || {
          let _ = tx.send(PtyEvent::Exited(child.wait_exit()));
      });
  }
  ```

Then search for other callers of `Child::wait(` or `process.child` across the workspace (`grep -rn "\.child\b\|Child::wait\|\.wait()" crates/dispatch-os/src/pty* crates/dispatch-pty`) and move each to `wait_exit` or `end_tree`.

- [ ] **Step 8: Run the tests**

Run: `cargo test -p dispatch-os && cargo test -p dispatch-pty && cargo test -p dispatch-daemon`
Expected: PASS, including the 4 new `dispatch-os` tests and the extended pty test.

Also run `cargo clippy -p dispatch-os -p dispatch-pty --all-targets --target aarch64-apple-darwin -- -D warnings` and the same for `x86_64-pc-windows-gnu`.

- [ ] **Step 9: Lint and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`

```bash
git add crates/dispatch-os/src/process.rs crates/dispatch-os/src/pty.rs crates/dispatch-os/src/pty/windows.rs crates/dispatch-pty/src/session.rs crates/dispatch-pty/src/session/tests.rs
git commit -m "fix(os): keep an exited pane's process group until the pane is closed"
```

---

### Task 4: A new subagent leaves focus where it is

**Files:**
- Modify: `crates/dispatch-core/src/state.rs`: `adopt_pane`'s focus rule and doc; tests.
- Modify: `dispatch/src/app.rs`: one test in `mod tests`.
- Modify: `README.md`: one sentence in `## Delegation`.

**Interfaces:** none new.

- [ ] **Step 1: Write the failing tests**

(a) In `crates/dispatch-core/src/state.rs`'s `mod tests`, after `a_child_is_listed_under_its_parent`:

```rust
    #[test]
    fn a_new_child_leaves_focus_on_its_parent() {
        // A subagent is not in the grid until it is opened, so focus on it is
        // focus on nothing the user can see.
        let (state, parent, _child) = parent_and_child(false);

        assert_eq!(state.focused_pane(), Some(parent));
    }

    #[test]
    fn a_new_child_leaves_a_zoomed_pane_zoomed() {
        let mut state = AppState::new();
        let project = state.add_project(Project::new("/tmp/one", ProjectSource::LocalDir));
        let parent = state
            .spawn_pane(project, HarnessId::new("claude"))
            .expect("the project exists");
        state.toggle_zoom();
        assert_eq!(state.zoomed_pane(), Some(parent));

        let mut child = Pane::new(project, HarnessId::new("claude"));
        child.parent = Some(parent);
        state.adopt_pane(child).expect("the project exists");

        assert_eq!(state.zoomed_pane(), Some(parent), "the grid did not change");
    }

    #[test]
    fn a_child_arriving_when_nothing_is_focused_is_focused() {
        let mut state = AppState::new();
        let project = state.add_project(Project::new("/tmp/one", ProjectSource::LocalDir));

        let mut child = Pane::new(project, HarnessId::new("claude"));
        child.parent = Some(PaneId::new());
        let id = child.id;
        state.adopt_pane(child).expect("the project exists");

        assert_eq!(state.focused_pane(), Some(id));
    }

    #[test]
    fn a_reattach_leaves_focus_on_the_parent_not_its_last_child() {
        // A reattach replays a parent and then its children. Focus ends on
        // the pane the user can see.
        let mut state = AppState::new();
        let project = state.add_project(Project::new("/tmp/one", ProjectSource::LocalDir));
        let parent = Pane::new(project, HarnessId::new("claude"));
        let parent_id = parent.id;
        state.adopt_pane(parent).expect("the project exists");
        for _ in 0..2 {
            let mut child = Pane::new(project, HarnessId::new("claude"));
            child.parent = Some(parent_id);
            state.adopt_pane(child).expect("the project exists");
        }

        assert_eq!(state.focused_pane(), Some(parent_id));
    }
```

If `PaneId` is not in scope in these tests, add it to the test module's imports from `crate`.

(b) In `dispatch/src/app.rs`'s `mod tests`, after `a_pane_finishing_out_of_focus_is_marked_done_until_focused`:

```rust
    #[test]
    fn a_parent_whose_subagent_arrives_keeps_focus_and_is_not_marked_done() {
        // The user was looking at the parent when the subagent arrived, so its
        // going quiet afterwards is not something finished out of sight.
        let (mut app, project, daemon, _sent) = attached_app();
        let clock = hand_clock(&mut app);
        let parent = spawn_several(&mut app, &daemon, project, 1)[0];

        let child = PaneId::new();
        daemon
            .send(spawned(child, project, "claude", Some(parent), false))
            .expect("the app is listening");
        app.poll_daemon();
        assert_eq!(
            app.state.focused_pane(),
            Some(parent),
            "the new subagent leaves focus where it was"
        );

        advance(&clock, Duration::from_secs(4));
        print(&mut app, &daemon, parent, b"delegated\r\n");
        settle(&mut app, &clock);

        assert_eq!(status_of(&app, parent), PaneStatus::Idle);
        assert!(!app.state.is_unseen(parent), "the user was looking at it");
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p dispatch-core state && cargo test -p dispatch --bin dispatch a_parent_whose_subagent`
Expected: FAIL. The new child takes focus and clears the zoom; the app test's first assert fails.

- [ ] **Step 3: Implement**

In `AppState::adopt_pane`, before `self.panes.push(pane);` add `let delegated = pane.parent.is_some();`. Replace

```rust
        if self.selected_project == Some(project) {
            self.focused_pane = Some(id);
            // A new pane changes the grid, so a zoomed pane would hide it.
            self.zoomed_pane = None;
        }
```

with

```rust
        // A new top-level pane is what the user is about to use, so it takes
        // focus, and a zoomed pane would hide it. A subagent is not in the grid
        // until it is opened, so it leaves both alone -- focus on it would be
        // focus on nothing the user can see -- unless nothing had focus.
        if self.selected_project == Some(project) {
            if !delegated || self.focused_pane.is_none() {
                self.focused_pane = Some(id);
            }
            if !delegated {
                self.zoomed_pane = None;
            }
        }
```

Add a paragraph to `adopt_pane`'s doc comment: "A new top-level pane takes focus. A subagent takes it only when nothing has it."

- [ ] **Step 4: Update the tests that expected the old rule**

Run: `cargo test -p dispatch-core && cargo test -p dispatch --bin dispatch`.

Some existing tests adopt a child and then act on "the focused pane" expecting the child. For each failure, if the test is about the child, make it focus the child explicitly (`state.focus(child)` or `app.focus_pane(child)`) before acting. Don't change what a test asserts about anything else. List every test changed in your report.

- [ ] **Step 5: README**

In `README.md`'s `## Delegation`, extend the sentence "The subagent runs as a pane under the one that asked, and the caller gets its output and exit code when it finishes." with a new sentence after it:

```
Focus stays where it was: open the subagent with `Ctrl p s` (or `^a s`) to
watch it.
```

- [ ] **Step 6: Run the tests**

Run: `cargo test -p dispatch-core && cargo test -p dispatch`
Expected: PASS, including the 5 new tests, with the end-to-end suite green.

- [ ] **Step 7: Lint and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`

```bash
git add crates/dispatch-core/src/state.rs dispatch/src/app.rs README.md
git commit -m "fix(core): leave focus where it is when a subagent arrives"
```

---

### Task 5: A closed listener removes its socket file

**Files:**
- Modify: `crates/dispatch-os/src/ipc.rs`:
  - `Listener` gains `socket: Option<imp::Identity>`, plus a test-only `socket_fd` on Unix;
  - `bind_with` records them;
  - `Drop` removes the file;
  - both `imp` modules gain `Identity`, `identity` and `remove_if_same`;
  - one test.

**Interfaces:**
- Consumes: Task 1's `Listener::bind_with`.

- [ ] **Step 1: Write the failing test**

Add to `ipc.rs`'s `mod tests`, after `dropping_a_listener_frees_its_endpoint`:

```rust
    #[test]
    #[cfg(unix)]
    fn a_copy_of_the_socket_held_elsewhere_does_not_keep_the_endpoint() {
        // A child forked while the listener is open holds a copy of its
        // socket until it execs. The next bind must not take that copy for a
        // running daemon. This is what made the test above fail about half
        // the time beside the tests that spawn processes.
        let _guard = crate::env_lock();
        let _endpoint = Endpoint::new("held");

        let first = Listener::bind().expect("binding succeeds");
        // SAFETY: dup takes a descriptor by value; this one is the listener's
        // and is open until the listener is dropped.
        let copy = unsafe { libc::dup(first.socket_fd) };
        assert!(copy >= 0, "the socket can be copied");
        drop(first);

        let second = Listener::bind();
        // SAFETY: closes the copy made above, and nothing else.
        unsafe { libc::close(copy) };
        second.expect("a copy of the old socket does not keep the endpoint");
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p dispatch-os ipc::tests::a_copy_of_the_socket`
Expected: FAIL to compile, because there is no field `socket_fd`. Once the field exists (Step 3's (a) and (b)), and before Step 3 (c), the test fails with `AlreadyRunning`.

- [ ] **Step 3: Implement**

(a) `struct Listener`: add after `endpoint`:

```rust
    /// Which file the socket is, recorded at bind, so a drop removes the
    /// path only while it still names this listener's socket.
    socket: Option<imp::Identity>,
    /// The listening socket's descriptor, for a test to copy as a forked
    /// child would.
    #[cfg(all(test, unix))]
    socket_fd: std::os::fd::RawFd,
```

(b) In `bind_with`, after `let inner = imp::bind(path)?;` add:

```rust
        let socket = imp::identity(path);
        #[cfg(all(test, unix))]
        let socket_fd = std::os::fd::AsRawFd::as_raw_fd(&inner);
```

and add `socket,` and `#[cfg(all(test, unix))] socket_fd,` to the `Ok(Self { … })`.

(c) `impl Drop for Listener`: after `drop(wake);` add:

```rust
        // The file goes with the listener. Left behind, the next bind finds
        // the path in use and probes it, and a copy of the old socket that a
        // forked child holds for an instant answers as if a daemon were
        // running. Removed only while it is still this listener's socket: a
        // daemon that has since replaced it keeps its own.
        if let Some(socket) = self.socket.take() {
            imp::remove_if_same(&self.endpoint, socket);
        }
```

(d) In the Unix `mod imp`:

```rust
    /// A socket file's device and inode.
    pub(super) type Identity = (u64, u64);

    /// Which file `path` is now, if there is one.
    pub(super) fn identity(path: &Path) -> Option<Identity> {
        use std::os::unix::fs::MetadataExt;

        std::fs::symlink_metadata(path)
            .ok()
            .map(|metadata| (metadata.dev(), metadata.ino()))
    }

    /// Removes `path` if it is still the file `socket` was.
    pub(super) fn remove_if_same(path: &Path, socket: Identity) {
        if identity(path) == Some(socket) {
            let _ = std::fs::remove_file(path);
        }
    }
```

(e) In the Windows `mod imp`:

```rust
    /// A pipe has no file to remove; nothing to identify.
    pub(super) type Identity = ();

    pub(super) fn identity(_path: &Path) -> Option<Identity> {
        None
    }

    pub(super) fn remove_if_same(_path: &Path, _socket: Identity) {}
```

If `Path` is not already imported in the Windows `imp`, add `use std::path::Path;`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p dispatch-os ipc::tests`
Expected: PASS: the new test, `dropping_a_listener_frees_its_endpoint`, `dropping_a_listener_whose_socket_was_removed_returns`, `a_socket_left_by_a_dead_daemon_is_replaced`, and the rest. Then run `for i in $(seq 1 10); do cargo test -q -p dispatch-os --lib 2>&1 | grep -E "test result|FAILED"; done` and report how many of the 10 runs pass. Before this task, about half failed on `dropping_a_listener_frees_its_endpoint`.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo clippy -p dispatch-os --all-targets --target x86_64-pc-windows-gnu -- -D warnings`

```bash
git add crates/dispatch-os/src/ipc.rs
git commit -m "fix(os): remove a listener's socket file when the listener goes"
```

---

### Task 6: Verify the whole branch

**Files:** none, unless a check fails. Then fix it and commit it as `fix(scope): what verifying the reliability pass turned up`.

- [ ] **Step 1: The whole suite.** Run `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace --no-fail-fast`. Report the total. Run the end-to-end suite (`cargo test -p dispatch --test end_to_end`) twice.
- [ ] **Step 2: The other platforms.** Run `cargo clippy --workspace --all-targets --target aarch64-apple-darwin -- -D warnings` and `cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings`. Both must be clean: Windows has been green on `main` since a94e438.
- [ ] **Step 3: Rust 1.89.** List any std API the branch uses that is newer than 1.89. Clippy's `incompatible_msrv` lint catches most.
- [ ] **Step 4: The spec against the code.** Read `docs/superpowers/specs/2026-09-27-reliability-pass-design.md` against the code. Fix whichever is wrong. Change the spec's `Status:` line to `implemented on branch \`fix/reliability\`.`
- [ ] **Step 5: Commit, if anything changed.** Run `git status --short`, then stage by name.
