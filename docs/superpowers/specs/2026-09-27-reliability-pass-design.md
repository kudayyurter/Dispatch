# Reliability pass: five things a day-long session trips over

**Status:** implemented on branch `fix/reliability`.
**Date:** 2026-09-27.

**Where these came from:**
- the audit remediation's own follow-up list (PR #1): items 1, 2 and 3;
- the merge of that PR with the UI overhaul: items 4 and 5.

Dispatch is meant to run all day, with agents that outlive the interface. None of these five is a new feature. Each one is something a long session will eventually hit.

## Goal

Every item below is pinned by a test that fails before its fix and passes after. All five CI jobs (ubuntu, macos, windows, msrv 1.89, advisories) stay green.

## Constraints

- No new dependencies. `libc` is already a dependency of `dispatch-os`.
- The protocol does not change.
- Platform `#[cfg]` attributes live only in `dispatch-os`. Tests may carry `#[cfg(...)]`.
- `rust-version` stays 1.89.
- Comments say why, in the surrounding voice.
- Commits are `type(scope): lowercase summary`.

## 1. The daemon keeps accepting after a passing error

**Today.** `dispatch-os/src/ipc.rs`, `accept_all`: any error from the platform's accept is sent to the listener's queue, and the accepting thread returns. `Listener::accept` then reports `BrokenPipe` for ever, and the daemon's `serve` loop (`dispatch-daemon/src/session.rs`) logs "failed to accept a connection" and stops accepting. A daemon that ran out of file descriptors for a moment keeps its panes running, but no client can reach it again.

**Change.** The platform layer sorts each accept error as passing or fatal:
- **Unix, passing:** `EMFILE`, `ENFILE`, `ENOBUFS`, `ENOMEM`, `ECONNABORTED`, `EPROTO`, `EINTR`.
- **Windows, passing:** `ERROR_NOT_ENOUGH_MEMORY`, `ERROR_OUTOFMEMORY`, `ERROR_NO_SYSTEM_RESOURCES`, `ERROR_TOO_MANY_OPEN_FILES`. That covers both `ConnectNamedPipe` and creating the next pipe instance.
- Everything else is fatal.

After a passing error:
- **Backoff:** the accepting thread waits and tries again. The wait starts at 10 ms and doubles up to 1 s. Any successful accept resets it.
- **Logging:** the first passing error of a run is logged at `warn`. Further errors in the same run are counted and not logged one by one. When accepting recovers, one line says how many were skipped.
- **Stopping:** the `stopping` flag is still checked after every wait, so a listener being dropped is not held up by a backoff.

A fatal error behaves as today: reported once, then the thread stops.

On Windows, a failure to create the next instance must leave the listener able to try again. The pipe must not be left with no instance at all while the loop keeps running.

**Test.** The loop takes its accept step as a parameter (a closure or a small trait), so a test can script it:
- two `EMFILE` errors, then a real connection: the connection is delivered;
- a fatal error: the loop stops;
- a drop during a backoff: the loop stops promptly.

The error classification gets a table test on each platform.

## 2. Pane output bounded by bytes, not by reads

**Today.** `dispatch-pty/src/session.rs`: the reader thread sends each `read()` result through `sync_channel(OUTPUT_CHUNKS = 32)`, and each `drain` takes up to `DRAIN_BUDGET` (128 KiB). The daemon drains every `TICK` (8 ms). A program that writes one line per `write()` fills the 32 slots with 32 lines, so it moves about 32 lines per tick. That is about 180 KB/s on macOS, while a program writing large blocks is unaffected.

**Change.** The queue between reader and drain is bounded by bytes:
- **Queue:** reads go into an unbounded channel, and a shared count of waiting bytes (a `Mutex<usize>` and a `Condvar`, or equivalent) is kept beside it.
- **Reader:** after each send, while more than `OUTPUT_BUDGET` = 256 KiB is waiting, the reader waits on the condvar. A pane that prints faster than it is drained is still slowed down, not held in memory. 256 KiB is today's worst case: 32 reads of 8 KiB.
- **Drain:** `drain` takes events until `DRAIN_BUDGET` bytes are in hand, as today. It then subtracts what it took and wakes the reader.
- **Exit:** `Exited` still travels through the same channel, after all output read before it.
- **Windows:** the reader's behaviour after its receiver is gone (keep reading and drop, per `OUTPUT_OUTLIVES_ITS_READER`) is unchanged. A reader whose receiver has gone never waits on the budget.

This is the same code in the daemon and in a standalone interface, which both use `Pty`.

**Tests.** They are deterministic, written against the queue as the existing `drain_from` tests are:
- 1,000 reads of 50 bytes come out in one drain, where today they take 32 at a time;
- the reader waits once past the budget, and resumes after a drain;
- an exit arrives after the output sent before it;
- a reader whose receiver has gone does not block.

## 3. An exited pane keeps its process group's number until it is closed (Unix)

**Today.** `dispatch-pty`'s waiter thread calls `dispatch_os::pty::Child::wait`, which reaps the leader the moment it exits. Its pid, which is the pane's process group id, is then free. Closing the pane much later calls `terminate_tree(pid)`, which sends `killpg(pid, …)`. By then an unrelated process may have been given that number and made itself a group leader, so closing an old pane can signal a stranger's process group.

**Change.** The leader is kept unreaped, as a zombie, from its exit until the pane is closed or dropped, so its number cannot be reused. The Linux and BSD kernels keep a number in use for as long as a process, zombie or not, still has it as its pid or process group id.

- **Reporting the exit.** The waiter learns the exit status with `waitid(P_PID, pid, WEXITED | WNOWAIT)`, which does not reap, and reports `Exited(code)` exactly as today. The code is reported exactly once, whichever of the waiter and the closing path sees the exit first. Only `CLD_EXITED`, `CLD_KILLED` and `CLD_DUMPED` count as an exit, because macOS has been seen to answer `WEXITED` for a child that has only stopped (golang/go#19314), and a stopped leader taken for exited would be reaped while it was still going.
- **Ending the tree on close.** Close and drop end the tree in this order:
  1. `SIGTERM` the group. The leader is still pinned, so the group is certainly ours.
  2. Wait up to the grace period for the leader to exit, without reaping.
  3. Reap the leader.
  4. Wait out the rest of the grace for any remaining members, using the existing `killpg(pgid, 0)` probe.
  5. If members remain, `SIGKILL` the group and wait up to `KILL_TIMEOUT`, as today.
- **Why reap in step 3.** Step 4's probe counts a zombie leader as a member. Without step 3, every close would wait out the whole grace period and then force-kill.
- **Never signal an emptied group.** Once any probe reports the group gone (`ESRCH`), nothing signals that number again. After step 3, the number is held by any members still alive. The only window left is between a probe that found members and the `SIGKILL` straight after it, which is microseconds, and is documented where it is.
- **Where the code goes.**
  - **`dispatch-os/src/pty.rs`:** this sequence lives here, since it is platform code. `Child` gains a way to learn the exit without reaping, and a way to end its tree and reap, and it can be shared with the waiter thread.
  - **`dispatch-pty`:** the pane's close and drop use the new tree-ending instead of `process::terminate_tree`.
  - **Windows:** Job Objects already end the tree, and a process handle is held until drop. Only the shape of the API is shared there.
- **Out of scope.** Command transports (`process::spawn_contained`) already reap their leader before waiting for their tree, during an active close. They do not have the long-exited case.
- **The cost.** Each pane that has exited but not been closed holds one zombie. It is gone when the pane is closed, and on shutdown when the daemon ends every pane.

**Tests** (Unix, `dispatch-os` or `dispatch-pty`):
- a pane whose shell has exited still has a zombie leader, and its group id is not free;
- closing it ends a grandchild left running in the group, reaps the leader, and returns promptly, well inside the grace period, when nothing ignores `SIGTERM`;
- a drop without a close also reaps;
- the exit code is reported once.

## 4. A new subagent leaves focus where it is

**Today.** `dispatch-core/src/state.rs`, `AppState::adopt_pane`, focuses every new pane in the selected project and clears the zoom, including a subagent. A subagent is not drawn in the grid until it is opened (`Ctrl p s`, `^a s`), so focus lands on a pane the user cannot see. The parent loses focus, and slice B then marks it "finished while you were looking elsewhere" when it goes quiet after its grace period, though the user was looking at it.

**Change.**
- A new top-level pane is focused, and clears the zoom, as today.
- A new subagent (one with a `parent`) takes focus only when nothing is focused, and leaves the zoom alone.

Both a standalone interface (`add_pane`) and an attached one (`adopt_remote`) go through `adopt_pane`, so this is the one place to change.

**Tests.**
- **Core:** adopting a child keeps focus on its parent and leaves a zoomed pane zoomed; a child adopted when nothing is focused is focused.
- **App:** a parent whose child is adopted, and which goes idle after its grace period, is not marked unseen.
- **Existing tests:** any that expected a child to take focus are updated, and the change is listed.

## 5. A closed listener removes its socket file (Unix)

**Today.** `dispatch-os ipc::tests::dropping_a_listener_frees_its_endpoint` fails about half the time beside the `process::` tests:
- `Listener::drop` stops the accepting thread but leaves the socket file in place;
- a child that another test forked at that moment still holds a copy of the listening socket until its `exec`;
- the next bind finds the path in use, probes it, gets an answer, and reports `AlreadyRunning`.

The same happens to a daemon restarted in the same process while one of its panes is being spawned.

**Change.**
- When the listener binds, it records the socket file's device and inode.
- `Listener::drop`, after the accepting thread has been joined, removes the path, but only if it still names that same socket. A socket another daemon has since put there is left alone.
- A later bind then finds no file and binds cleanly, whatever copies of the old socket linger for an instant.

**Test.** Hold a duplicate of the listener's socket descriptor (`libc::dup`), as a forked child would, then drop the listener. Binding the same path again must succeed. This fails every time before the fix. The existing test stays.

## Failure cases

| Case | Behaviour |
|---|---|
| Accept fails with `EMFILE` | backs off and retries; logged once per run |
| Accept fails with a fatal error | reported once; the accepting thread stops, as today |
| A listener is dropped during a backoff | stops promptly |
| A pane prints faster than it is drained | reader waits at 256 KiB; the child blocks on the pty |
| A pane's leader exits long before the pane is closed | kept as a zombie; closing signals only its own group |
| A pane's tree has a member ignoring `SIGTERM` | `SIGKILL` after the grace period, as today |
| A subagent is approved | focus stays; the child appears in the sidebar |
| Another daemon has replaced the socket file before a drop | the file is left alone |

## Testing

Each item's tests are listed above. The whole suite (`cargo test --workspace`), including the end-to-end tests, runs green on Linux, and CI runs all five jobs. macOS and Windows are checked by CI and by clippy for `aarch64-apple-darwin` and `x86_64-pc-windows-gnu` locally.

## Documentation

- **Item 1 (accepting):** no README change; a doc comment on the accept loop.
- **Item 2 (output):** the constants' doc comments are updated.
- **Item 3 (process groups):** the module docs of `dispatch-os/src/pty.rs` say why the leader is kept.
- **Item 4 (focus):** the README's delegation section says an approved subagent appears in the sidebar and is opened with `Ctrl p s` or `^a s`, if it does not say so already.
- **Item 5 (socket file):** a doc comment on `Listener::drop`.
