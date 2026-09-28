# Shared Pane Size: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When several Dispatch windows show the same pane, the window in use decides its size, and every window draws it at that real size instead of garbling it.

**Architecture:**
- The daemon ranks its interface clients by use (activity messages, plus a new throttled `Active`). It remembers each client's asked size per pane in a small pure `Sizes` table. It gives each pane the size of its highest-ranked asker, and broadcasts a new `PaneResized` whenever the pty's size changes (also in `Subscribe`'s catch-up and after each `PaneSpawned`).
- A client resizes its emulator for a remote pane only on `PaneResized`. The ordered connection then puts every byte in an emulator of the size it was drawn for. The client tracks what it last asked separately, sends `HidePane` for panes that leave its screen, and falls back to today's behaviour for a daemon that never sends `PaneResized`.

**Tech Stack:** Rust 2024 (rust-version 1.89), serde, ratatui 0.30, crossterm 0.29 (through `dispatch_tui::input`). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-09-28-shared-pane-size-design.md`

## Global Constraints

- No new dependencies. Protocol `VERSION` stays `1.1`. New variants land in `#[serde(other)] Unknown` on older peers.
- New messages, exactly:
  - `ServerMessage::PaneResized { pane: PaneId, size: (u16, u16) }` (tag `pane_resized`)
  - `ClientMessage::Active` (tag `active`)
  - `ClientMessage::HidePane { pane: PaneId }` (tag `hide_pane`)
  - `size` is `(cols, rows)`, as in `ResizePane`.
- **Activity** (ranks the sending client, interface role only):
  - `Subscribe`, `WritePane`;
  - `OpenProject`, `CloseProject`, `SpawnPane`, `ClosePane`;
  - `MovePane`, `RenameTab`, `CloseTab`, `MoveTab`;
  - `DelegateDecision`, `Active`.
- **Not activity:** `Hello`, `Ping`, `ResizePane`, `HidePane`, `DelegateRequest`, `Unknown`, and anything from a `Role::Delegate` client.
- A pane's size is the size asked by its highest-ranked asker, ties going to the higher client id. A pane with no asker keeps its size.
- The daemon resizes the pty **before** broadcasting `PaneResized`. In the catch-up, `PaneResized` follows the pane's `PaneSpawned` and precedes its replayed `PaneOutput`.
- The client throttles `Active` to at most one per 500 ms (`ACTIVE_EVERY`). It sends `Active` and `HidePane` only to a daemon it knows is size-aware.
- Standalone (local `PtySession`) behaviour is unchanged.
- Platform `#[cfg]` only in `dispatch-os`; tests may use `#[cfg(unix)]`.
- CI must stay green:
  - `cargo fmt --all --check`;
  - `cargo clippy --workspace --all-targets -- -D warnings`, and again with `--target x86_64-pc-windows-gnu`;
  - `cargo test --workspace --no-fail-fast`.
- Comments explain why, in the surrounding voice. Commits are `type(scope): lowercase summary`, ending with exactly:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2
  ```

## Review Focus

1. **The window in use changes while both windows keep resizing.** Pinned in Task 3 by:
   - `a_pane_takes_the_size_of_the_window_last_used`
   - `typing_counts_as_use_and_resizing_does_not`
2. **The active window closes.** The next most recent window must take over, not leave the pane stuck. Pinned in Task 3 by `the_next_window_takes_over_when_the_one_in_use_leaves`.
3. **A newly attached window's replay.** It must be drawn at the pane's current size, which needs `PaneResized` before the replayed output. Pinned in Task 3 by `a_window_attaching_is_told_each_panes_size_before_its_output`.
4. **An old daemon.** A new window must keep resizing its own emulators, or its panes would never resize at all. Pinned in Task 4 by `with_an_old_daemon_a_pane_is_resized_here_as_before`.
5. **A pane that leaves the screen and comes back.** It must withdraw its size and ask again. Pinned in Task 4 by `a_pane_leaving_the_screen_withdraws_its_size_and_asks_again`.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/dispatch-proto/src/message.rs` + `message/tests.rs` | Task 1: the three messages. |
| `crates/dispatch-daemon/src/sizing.rs` + `sizing/tests.rs` (new) | Task 2: `Sizes`, the pure rank-and-ask table. |
| `crates/dispatch-daemon/src/lib.rs` | Task 2: `mod sizing;`. |
| `crates/dispatch-daemon/src/session.rs` + `session/tests.rs` | Task 3: ranking, asks, `fit`, `PaneResized` broadcasts, a test hook. |
| `dispatch/src/backend.rs` | Task 4: `RemotePane` `asked`/`sized_by_daemon`, `Backend::fit`, `Backend::hide`, `RemotePane::resized`. |
| `dispatch/src/app.rs` | Task 4: `resize_panes` via `fit` and `hide`; `PaneResized` handling. Task 5: `Active`, the attachment's `size_aware`, mouse bounds. |
| `README.md`, the spec | Task 6. |

---

### Task 1: The three messages

**Files:**
- Modify: `crates/dispatch-proto/src/message.rs`
- Modify: `crates/dispatch-proto/src/message/tests.rs`

**Interfaces:**
- Produces:
  - `ServerMessage::PaneResized { pane: PaneId, size: (u16, u16) }`
  - `ClientMessage::Active`
  - `ClientMessage::HidePane { pane: PaneId }`

- [ ] **Step 1: Write the failing tests**

Append to `crates/dispatch-proto/src/message/tests.rs`:

```rust
#[test]
fn the_shared_size_messages_survive_the_wire() {
    let pane = PaneId::new();

    let server = ServerMessage::PaneResized {
        pane,
        size: (120, 40),
    };
    let mut buf = Vec::new();
    Frame::write(&mut buf, &server).expect("writing succeeds");
    let read: ServerMessage = Frame::read(&mut buf.as_slice()).expect("reading succeeds");
    assert_eq!(read, server);

    for client in [ClientMessage::Active, ClientMessage::HidePane { pane }] {
        let mut buf = Vec::new();
        Frame::write(&mut buf, &client).expect("writing succeeds");
        let read: ClientMessage = Frame::read(&mut buf.as_slice()).expect("reading succeeds");
        assert_eq!(read, client);
    }
}

#[test]
fn an_older_build_reads_the_shared_size_messages_as_unknown() {
    // What a build from before these messages sees: its own variants, and
    // `Unknown` for the rest. Reading one must not fail the frame, which
    // would take the connection with it.
    #[derive(Debug, serde::Deserialize, PartialEq)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum Older {
        Ping {
            token: u64,
        },
        #[serde(other)]
        Unknown,
    }

    let pane = PaneId::new();
    let mut frames = Vec::new();
    Frame::write(&mut frames, &ClientMessage::Active).expect("writing succeeds");
    Frame::write(&mut frames, &ClientMessage::HidePane { pane }).expect("writing succeeds");
    Frame::write(
        &mut frames,
        &ServerMessage::PaneResized {
            pane,
            size: (80, 24),
        },
    )
    .expect("writing succeeds");

    let mut reader = frames.as_slice();
    for _ in 0..3 {
        let read: Older = Frame::read(&mut reader).expect("an older build reads it");
        assert_eq!(read, Older::Unknown);
    }
}
```

If `serde` is not a direct dependency the tests module can name, use whatever path the existing `Newer` / `FromNewer` test enums in this file use for `Serialize`/`Deserialize`.

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p dispatch-proto shared_size`
Expected: FAIL to compile: `PaneResized`, `Active` and `HidePane` do not exist.

- [ ] **Step 3: Add the variants**

In `crates/dispatch-proto/src/message.rs`:

In `ClientMessage`, after `ResizePane { .. }`, add:

```rust
    /// The user is using this window: it should decide the size of the panes
    /// it shows.
    ///
    /// Sent on input the daemon would not otherwise see, such as moving focus
    /// or resizing the window, at most twice a second.
    Active,

    /// This window no longer shows `pane`, so its size for it stops counting.
    HidePane {
        /// The pane it has stopped showing.
        pane: PaneId,
    },
```

In `ServerMessage`, after `PaneOutput { .. }`, add:

```rust
    /// A pane's pty is now `size`, as `(cols, rows)`.
    ///
    /// Sent after the pty was resized, so output drawn at the new size always
    /// follows it on the connection, and output drawn at the old size comes
    /// before it.
    PaneResized {
        /// Which pane.
        pane: PaneId,
        /// Its size now, in cells, as `(cols, rows)`.
        size: (u16, u16),
    },
```

- [ ] **Step 4: Handle the new variants where matches are exhaustive**

Run `cargo build --workspace --all-targets`. Where the compiler reports a non-exhaustive match on the new variants:
- the daemon's `handle_request` in `crates/dispatch-daemon/src/session.rs`: add `ClientMessage::Active | ClientMessage::HidePane { .. } => {}` for now (Task 3 gives them meaning);
- the client's `apply_from` in `dispatch/src/app.rs`: add `ServerMessage::PaneResized { .. } => false` for now (Task 4 gives it meaning);
- any other match: the arm that does nothing.

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test -p dispatch-proto && cargo build --workspace --all-targets`
Expected: PASS; the workspace builds.

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`

```bash
git add crates/dispatch-proto dispatch/src/app.rs crates/dispatch-daemon/src/session.rs
git commit -F - <<'EOF'
feat(proto): add the messages that share a pane's size between windows

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2
EOF
```

---

### Task 2: The daemon's rank-and-ask table

**Files:**
- Create: `crates/dispatch-daemon/src/sizing.rs`
- Create: `crates/dispatch-daemon/src/sizing/tests.rs`
- Modify: `crates/dispatch-daemon/src/lib.rs` (`mod sizing;`)

**Interfaces:**
- Produces `pub(crate) struct Sizes`, with `Default` and these methods:
  - `touch(&mut self, client: u64) -> Vec<PaneId>`
  - `ask(&mut self, client: u64, pane: PaneId, size: Size)`
  - `hide(&mut self, client: u64, pane: PaneId)`
  - `forget_client(&mut self, client: u64) -> Vec<PaneId>`
  - `forget_pane(&mut self, pane: PaneId)`
  - `wanted(&self, pane: PaneId) -> Option<Size>`

`Size` is `dispatch_pty::Size`; `PaneId` is `dispatch_core::PaneId`.

- [ ] **Step 1: Write the failing tests**

Create `crates/dispatch-daemon/src/sizing/tests.rs`:

```rust
//! Tests for which window's size a pane takes.

use super::*;

fn size(cols: u16, rows: u16) -> Size {
    Size::new(cols, rows)
}

#[test]
fn a_pane_nobody_asked_about_has_no_wanted_size() {
    assert_eq!(Sizes::default().wanted(PaneId::new()), None);
}

#[test]
fn the_most_recently_used_window_decides() {
    let pane = PaneId::new();
    let mut sizes = Sizes::default();
    sizes.touch(1);
    sizes.ask(1, pane, size(100, 30));
    sizes.touch(2);
    sizes.ask(2, pane, size(60, 20));

    assert_eq!(sizes.wanted(pane), Some(size(60, 20)));

    sizes.touch(1);
    assert_eq!(sizes.wanted(pane), Some(size(100, 30)));
}

#[test]
fn touching_a_window_names_the_panes_it_sizes_unless_it_was_already_in_use() {
    let (first, second) = (PaneId::new(), PaneId::new());
    let mut sizes = Sizes::default();
    sizes.touch(1);
    sizes.ask(1, first, size(100, 30));
    sizes.ask(1, second, size(100, 30));
    sizes.touch(2);

    let mut moved = sizes.touch(1);
    moved.sort_by_key(|pane| pane.to_string());
    let mut expected = vec![first, second];
    expected.sort_by_key(|pane| pane.to_string());
    assert_eq!(moved, expected);

    assert!(
        sizes.touch(1).is_empty(),
        "already the window in use: nothing moves"
    );
}

#[test]
fn a_window_that_hides_a_pane_stops_sizing_it() {
    let pane = PaneId::new();
    let mut sizes = Sizes::default();
    sizes.touch(1);
    sizes.ask(1, pane, size(100, 30));
    sizes.touch(2);
    sizes.ask(2, pane, size(60, 20));

    sizes.hide(2, pane);

    assert_eq!(sizes.wanted(pane), Some(size(100, 30)));
}

#[test]
fn a_window_that_leaves_hands_its_panes_on() {
    let pane = PaneId::new();
    let mut sizes = Sizes::default();
    sizes.touch(1);
    sizes.ask(1, pane, size(100, 30));
    sizes.touch(2);
    sizes.ask(2, pane, size(60, 20));

    assert_eq!(sizes.forget_client(2), vec![pane]);
    assert_eq!(sizes.wanted(pane), Some(size(100, 30)));
    assert!(sizes.forget_client(2).is_empty(), "forgotten once");
}

#[test]
fn a_closed_pane_is_forgotten() {
    let pane = PaneId::new();
    let mut sizes = Sizes::default();
    sizes.ask(1, pane, size(100, 30));

    sizes.forget_pane(pane);

    assert_eq!(sizes.wanted(pane), None);
}

#[test]
fn a_window_never_used_loses_to_one_that_was() {
    // A delegate connection is never ranked; its ask, if it sent one, must
    // not beat a window in use.
    let pane = PaneId::new();
    let mut sizes = Sizes::default();
    sizes.touch(1);
    sizes.ask(1, pane, size(100, 30));
    sizes.ask(9, pane, size(40, 10));

    assert_eq!(sizes.wanted(pane), Some(size(100, 30)));
}

#[test]
fn between_windows_never_used_the_later_one_decides() {
    let pane = PaneId::new();
    let mut sizes = Sizes::default();
    sizes.ask(1, pane, size(100, 30));
    sizes.ask(2, pane, size(60, 20));

    assert_eq!(sizes.wanted(pane), Some(size(60, 20)));
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p dispatch-daemon sizing`
Expected: FAIL to compile: there is no module `sizing`.

- [ ] **Step 3: Implement**

Create `crates/dispatch-daemon/src/sizing.rs`:

```rust
//! Which window's size each pane takes, when several windows show it.
//!
//! Every window asks for the size of its own tile, and a pty has one size.
//! The window the user is using decides: each client is ranked by when it
//! was last used, and a pane takes the size its highest-ranked asker wants.
//! Kept apart from the daemon loop so the rule can be tested without one.

use std::collections::HashMap;

use dispatch_core::PaneId;
use dispatch_pty::Size;

/// Each window's asked size for each pane, and how recently each was used.
#[derive(Debug, Default)]
pub(crate) struct Sizes {
    asked: HashMap<(u64, PaneId), Size>,
    ranks: HashMap<u64, u64>,
    /// The last rank handed out; the next use gets one more.
    counter: u64,
}

impl Sizes {
    /// Marks `client` as the window in use now.
    ///
    /// Returns the panes whose size may have changed: those `client` asked a
    /// size for, or none when it was the window in use already.
    pub(crate) fn touch(&mut self, client: u64) -> Vec<PaneId> {
        let top = self.ranks.values().max().copied();
        let was = self.ranks.get(&client).copied();

        self.counter += 1;
        self.ranks.insert(client, self.counter);

        if was.is_some() && was == top {
            return Vec::new();
        }
        self.panes_asked_by(client)
    }

    /// Records that `client` shows `pane` in a tile of `size`.
    pub(crate) fn ask(&mut self, client: u64, pane: PaneId, size: Size) {
        self.asked.insert((client, pane), size);
    }

    /// Records that `client` no longer shows `pane`.
    pub(crate) fn hide(&mut self, client: u64, pane: PaneId) {
        self.asked.remove(&(client, pane));
    }

    /// Forgets a window that has gone, returning the panes it asked a size
    /// for, whose size may now change.
    pub(crate) fn forget_client(&mut self, client: u64) -> Vec<PaneId> {
        let panes = self.panes_asked_by(client);
        self.asked.retain(|(asker, _), _| *asker != client);
        self.ranks.remove(&client);
        panes
    }

    /// Forgets a pane that has closed.
    pub(crate) fn forget_pane(&mut self, pane: PaneId) {
        self.asked.retain(|(_, asked), _| *asked != pane);
    }

    /// The size `pane` should have: the one its most recently used asker
    /// wants, or `None` when no window has asked.
    ///
    /// A window never used ranks below every window that was. Between
    /// equals, the later client, the higher id, decides.
    pub(crate) fn wanted(&self, pane: PaneId) -> Option<Size> {
        self.asked
            .iter()
            .filter(|((_, asked), _)| *asked == pane)
            .max_by_key(|((client, _), _)| (self.ranks.get(client).copied().unwrap_or(0), *client))
            .map(|(_, size)| *size)
    }

    fn panes_asked_by(&self, client: u64) -> Vec<PaneId> {
        self.asked
            .keys()
            .filter(|(asker, _)| *asker == client)
            .map(|(_, pane)| *pane)
            .collect()
    }
}

#[cfg(test)]
mod tests;
```

In `crates/dispatch-daemon/src/lib.rs`, add `mod sizing;` in alphabetical place (after `mod session;`).

Until Task 3 uses it, the compiler will warn that `Sizes` is never used. Put `#[allow(dead_code)] // used by the daemon loop from the next commit` on the `mod sizing;` line so clippy's `-D warnings` passes. Task 3 removes it.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p dispatch-daemon sizing`
Expected: PASS (8 tests).

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all && cargo clippy -p dispatch-daemon --all-targets -- -D warnings`

```bash
git add crates/dispatch-daemon/src/sizing.rs crates/dispatch-daemon/src/sizing/tests.rs crates/dispatch-daemon/src/lib.rs
git commit -F - <<'EOF'
feat(daemon): rank windows by use and pick each pane's size from them

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2
EOF
```

---

### Task 3: The daemon decides and tells every window

**Files:**
- Modify: `crates/dispatch-daemon/src/session.rs`
- Modify: `crates/dispatch-daemon/src/session/tests.rs`
- Modify: `crates/dispatch-daemon/src/lib.rs` (drop Task 2's `#[allow(dead_code)]`)

**Interfaces:**
- Consumes: Task 1's messages; Task 2's `Sizes`.
- Produces:
  - the daemon behaviour the spec describes;
  - a test hook `Daemon::pane_size_for_test(&self, pane: PaneId) -> Option<Size>` (`#[doc(hidden)]`, beside the other `_for_test` hooks).

- [ ] **Step 1: Write the failing tests**

Append to `crates/dispatch-daemon/src/session/tests.rs`:

```rust
/// Attaches interface client `id`, says hello and subscribes.
fn attach_window(daemon: &mut Daemon, id: u64) -> Inbox {
    let inbox = daemon.attach_for_test(id);
    daemon.request_for_test(id, hello());
    daemon.request_for_test(id, ClientMessage::Subscribe);
    inbox
}

/// The last size `messages` said `pane` has.
fn resized(messages: &[ServerMessage], pane: PaneId) -> Option<(u16, u16)> {
    messages.iter().rev().find_map(|m| match m {
        ServerMessage::PaneResized { pane: p, size } if *p == pane => Some(*size),
        _ => None,
    })
}

fn ask_size(daemon: &mut Daemon, id: u64, pane: PaneId, cols: u16, rows: u16) {
    daemon.request_for_test(
        id,
        ClientMessage::ResizePane {
            pane,
            size: (cols, rows),
        },
    );
}

/// Two windows on one pane: window 1 asked 100×30 and window 2, used
/// last, asked 60×20.
fn two_windows(label: &str) -> (Daemon, TempDir, PaneId, Inbox, Inbox) {
    let (mut daemon, project, dir) = daemon(label);
    let first = attach_window(&mut daemon, 1);
    let pane = spawn_pane_for_test(&mut daemon, &first, project);
    ask_size(&mut daemon, 1, pane, 100, 30);
    let second = attach_window(&mut daemon, 2);
    ask_size(&mut daemon, 2, pane, 60, 20);
    let _ = drain(&first);
    let _ = drain(&second);
    (daemon, dir, pane, first, second)
}

#[test]
fn a_pane_takes_the_size_of_the_window_last_used() {
    let (mut daemon, _dir, pane, first, second) = two_windows("size-last-used");
    assert_eq!(daemon.pane_size_for_test(pane), Some(Size::new(60, 20)));

    daemon.request_for_test(1, ClientMessage::Active);

    assert_eq!(daemon.pane_size_for_test(pane), Some(Size::new(100, 30)));
    assert_eq!(resized(&drain(&first), pane), Some((100, 30)));
    assert_eq!(
        resized(&drain(&second), pane),
        Some((100, 30)),
        "the other window is told the real size too"
    );
}

#[test]
fn typing_counts_as_use_and_resizing_does_not() {
    let (mut daemon, _dir, pane, _first, _second) = two_windows("size-typing");

    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: b"x".to_vec(),
        },
    );
    assert_eq!(daemon.pane_size_for_test(pane), Some(Size::new(100, 30)));

    ask_size(&mut daemon, 2, pane, 70, 25);
    assert_eq!(
        daemon.pane_size_for_test(pane),
        Some(Size::new(100, 30)),
        "a resize is not use: window 1 still decides"
    );
}

#[test]
fn the_next_window_takes_over_when_the_one_in_use_leaves() {
    let (mut daemon, _dir, pane, first, _second) = two_windows("size-leaves");

    daemon.detach_for_test(2);

    assert_eq!(daemon.pane_size_for_test(pane), Some(Size::new(100, 30)));
    assert_eq!(resized(&drain(&first), pane), Some((100, 30)));
}

#[test]
fn a_window_that_stops_showing_a_pane_stops_sizing_it() {
    let (mut daemon, _dir, pane, _first, _second) = two_windows("size-hides");

    daemon.request_for_test(2, ClientMessage::HidePane { pane });

    assert_eq!(daemon.pane_size_for_test(pane), Some(Size::new(100, 30)));
}

#[test]
fn a_pane_nobody_shows_keeps_its_size() {
    let (mut daemon, project, _dir) = daemon("size-nobody");
    let first = attach_window(&mut daemon, 1);
    let pane = spawn_pane_for_test(&mut daemon, &first, project);
    ask_size(&mut daemon, 1, pane, 100, 30);
    let _ = drain(&first);

    daemon.request_for_test(1, ClientMessage::HidePane { pane });

    assert_eq!(daemon.pane_size_for_test(pane), Some(Size::new(100, 30)));
    assert_eq!(resized(&drain(&first), pane), None, "nothing changed");
}

#[test]
fn a_new_pane_is_announced_with_its_size() {
    let (mut daemon, project, _dir) = daemon("size-new-pane");
    let first = attach_window(&mut daemon, 1);
    let _ = drain(&first);

    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (90, 33),
            place: Placement::Auto,
        },
    );
    let seen = drain(&first);
    let spawned = seen
        .iter()
        .position(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
        .expect("announced");
    let ServerMessage::PaneSpawned { pane: new, .. } = &seen[spawned] else {
        unreachable!()
    };
    let sized = seen
        .iter()
        .position(|m| matches!(m, ServerMessage::PaneResized { pane: p, .. } if p == new))
        .expect("its size is said");
    assert!(spawned < sized, "after the announcement");
    assert_eq!(resized(&seen, *new), Some((90, 33)));
}

#[test]
fn a_subagent_is_announced_with_its_size() {
    let (mut daemon, project, _dir) = daemon("size-subagent");
    let ui = attach_window(&mut daemon, 1);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    // The delegation helper, which asks at 80×24.
    let _caller = ask(&mut daemon, parent, "echo sized");
    let request = pending(&drain(&ui)).expect("the interface is asked");

    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );

    let seen = drain(&ui);
    let child = seen
        .iter()
        .find_map(|m| match m {
            ServerMessage::PaneSpawned {
                pane,
                parent: Some(p),
                ..
            } if *p == parent => Some(*pane),
            _ => None,
        })
        .expect("the subagent is announced");
    assert_eq!(resized(&seen, child), Some((80, 24)));
}

#[test]
fn a_window_attaching_is_told_each_panes_size_before_its_output() {
    let (mut daemon, project, _dir) = daemon("size-catch-up");
    let first = attach_window(&mut daemon, 1);
    let pane = spawn_pane_for_test(&mut daemon, &first, project);
    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: b"echo size-catch-up-marker\r".to_vec(),
        },
    );
    wait_for(&mut daemon, &first, |m| {
        output_of(m, pane).contains("size-catch-up-marker")
    });

    let second = attach_window(&mut daemon, 2);
    let seen = drain(&second);

    let spawned = seen
        .iter()
        .position(|m| matches!(m, ServerMessage::PaneSpawned { pane: p, .. } if *p == pane))
        .expect("the pane is described");
    let sized = seen
        .iter()
        .position(|m| matches!(m, ServerMessage::PaneResized { pane: p, .. } if *p == pane))
        .expect("its size is said");
    let replayed = seen
        .iter()
        .position(|m| matches!(m, ServerMessage::PaneOutput { pane: p, .. } if *p == pane))
        .expect("its output is replayed");
    assert!(spawned < sized && sized < replayed, "{seen:#?}");
}

#[test]
fn a_delegate_connection_never_decides_a_size() {
    let (mut daemon, _dir, pane, _first, _second) = two_windows("size-delegate");
    let _caller = daemon.attach_for_test(9);
    daemon.request_for_test(
        9,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "delegate".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );

    ask_size(&mut daemon, 9, pane, 40, 10);
    daemon.request_for_test(9, ClientMessage::Active);

    assert_eq!(daemon.pane_size_for_test(pane), Some(Size::new(60, 20)));
}
```

The fixture's `spawn_pane_for_test` sends `SpawnPane` from client 1 at `(80, 24)`. `ask` (already in this file) is the delegation helper; the new size helper is `ask_size`. If `Size` is not in scope through `use super::*`, import `dispatch_pty::Size`.

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p dispatch-daemon size`
Expected: FAIL to compile: `pane_size_for_test` does not exist. Once it does, the tests fail on the missing behaviour.

- [ ] **Step 3: Implement**

In `crates/dispatch-daemon/src/session.rs`:

1. `use crate::sizing::Sizes;`, and a field on `Daemon`:

   ```rust
       /// Each window's asked size for each pane, and which window was used
       /// last: what decides a pane's size when several windows show it.
       sizes: Sizes,
   ```

   with `sizes: Sizes::default(),` in `with_limits`. Remove the `#[allow(dead_code)]` Task 2 put on `mod sizing;` in `lib.rs`.

2. A free function near the top of the file (after `type ClientId = u64;`):

   ```rust
   /// Whether `message` is the user using the window that sent it.
   ///
   /// A resize is not: a window resizes its panes when another window changes
   /// the layout, which says nothing about where the user is.
   fn counts_as_use(message: &ClientMessage) -> bool {
       matches!(
           message,
           ClientMessage::Subscribe
               | ClientMessage::WritePane { .. }
               | ClientMessage::OpenProject { .. }
               | ClientMessage::CloseProject { .. }
               | ClientMessage::SpawnPane { .. }
               | ClientMessage::ClosePane { .. }
               | ClientMessage::MovePane { .. }
               | ClientMessage::RenameTab { .. }
               | ClientMessage::CloseTab { .. }
               | ClientMessage::MoveTab { .. }
               | ClientMessage::DelegateDecision { .. }
               | ClientMessage::Active
       )
   }
   ```

3. A method on `Daemon`:

   ```rust
       /// Gives `pane` the size its most recently used window asked for, when
       /// that differs from the size it has, and tells every window.
       ///
       /// The pty first, then the message: output the program draws at the new
       /// size then always follows the message on every connection, and output
       /// drawn at the old size always precedes it.
       fn fit(&mut self, pane: PaneId) {
           let Some(size) = self.sizes.wanted(pane) else {
               return;
           };
           let Some(target) = self.panes.get_mut(&pane) else {
               return;
           };
           if target.session.size() == size {
               return;
           }
           if let Err(error) = target.session.resize(size) {
               tracing::warn!(%error, "failed to resize a pane");
               return;
           }
           self.broadcast(ServerMessage::PaneResized {
               pane,
               size: (size.cols, size.rows),
           });
       }
   ```

4. In `handle_request`, after the `!client.ready` check and before `match message`:

   ```rust
           // The window in use decides the size of what it shows.
           if counts_as_use(&message)
               && self
                   .clients
                   .get(&id)
                   .is_some_and(|client| client.role == Role::Interface)
           {
               for pane in self.sizes.touch(id) {
                   self.fit(pane);
               }
           }
   ```

5. The `ResizePane` arm becomes:

   ```rust
               ClientMessage::ResizePane { pane, size } => {
                   if !self.panes.contains_key(&pane) {
                       self.send(
                           id,
                           ServerMessage::Error {
                               error: ProtocolError::NoSuchPane(pane),
                           },
                       );
                       return;
                   }
                   self.sizes.ask(id, pane, Size::new(size.0, size.1));
                   self.fit(pane);
               }
   ```

   and Task 1's placeholder arm becomes:

   ```rust
               // Use is counted above, before the match.
               ClientMessage::Active => {}

               ClientMessage::HidePane { pane } => {
                   self.sizes.hide(id, pane);
                   self.fit(pane);
               }
   ```

6. In `spawn_pane`, after `self.panes.insert(id, pane);`, record the asker's size: `self.sizes.ask(client, id, size);`. `size` is the `Size` parameter. If it was moved into the spawn, take a copy first: `Size` is `Copy`. After the `PaneSpawned` broadcast, add:

   ```rust
           // Its size, said as for any other change of it, so every window's
           // copy starts at the pty's size rather than a guess.
           self.broadcast(ServerMessage::PaneResized {
               pane: id,
               size: (size.cols, size.rows),
           });
   ```

   Use the pane's actual size, `self.panes[&id].session.size()`, if `size` could differ from it (for example after clamping).

7. In the subagent spawn (the `self.broadcast(ServerMessage::PaneSpawned { ..., parent: Some(parent), durable })` after `resolve`), add the same `PaneResized` broadcast after it, using that pane's `session.size()`.

8. In `Subscribe`'s catch-up loop, right after `existing.push(ServerMessage::PaneSpawned { .. })` and before the branch and history pushes:

   ```rust
                       // Before anything replayed: the replay is drawn into a
                       // copy of the size the pane has now.
                       let size = pane.session.size();
                       existing.push(ServerMessage::PaneResized {
                           pane: pane.id,
                           size: (size.cols, size.rows),
                       });
   ```

9. Forget closed panes: in `close_pane` after `self.panes.remove`, in `terminate_panes` for each removed pane, and in `close_all_panes`'s drain loop, call `self.sizes.forget_pane(id)` with that pane's id.

10. Forget departed windows: at the top of `abandon(&mut self, caller: ClientId)` (every departure path reaches it):

    ```rust
            // Its tiles stop counting; each pane it sized follows the next
            // window that shows it.
            for pane in self.sizes.forget_client(caller) {
                self.fit(pane);
            }
    ```

11. The test hook, beside `pane_pids_for_test`:

    ```rust
        /// A pane's pty size, for tests of which window decides it.
        #[doc(hidden)]
        #[must_use]
        pub fn pane_size_for_test(&self, pane: PaneId) -> Option<Size> {
            self.panes.get(&pane).map(|target| target.session.size())
        }
    ```

    If `Size` is not re-exported where the test can name it, make the hook return `Option<(u16, u16)>` and adjust the tests to compare tuples.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p dispatch-daemon && cargo test -p dispatchd`
Expected: PASS, including 9 new tests.

An existing test that asserts an exact list or count of messages may now also see `PaneResized`. Update such a test to allow it, keeping its point, and name every test you changed in your report.

- [ ] **Step 5: Lint, including for Windows, and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo clippy -p dispatch-daemon --all-targets --target x86_64-pc-windows-gnu -- -D warnings`

```bash
git add crates/dispatch-daemon
git commit -F - <<'EOF'
feat(daemon): let the window in use decide a pane's size, and tell every window

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2
EOF
```

---

### Task 4: The client draws the real size

**Files:**
- Modify: `dispatch/src/backend.rs`
- Modify: `dispatch/src/app.rs` (`resize_panes`, `apply_from`, a new field, tests)

**Interfaces:**
- Consumes: Task 1's messages.
- Produces:
  - `Backend::fit(&mut self, size: Size) -> Result<bool>`, where `true` means the pane's screen changed size now, and replaces `Backend::resize`;
  - `Backend::hide(&mut self)`;
  - `RemotePane::resized(&mut self, size: Size) -> Result<()>`;
  - the `App` field `fitted: HashSet<PaneId>`.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `dispatch/src/app.rs`, near `the_repaint_after_a_resize_is_not_work`:

```rust
    /// Says `pane` is `cols`×`rows`, as a daemon that decides sizes does.
    fn say_size(
        app: &mut App,
        daemon: &Sender<ServerMessage>,
        pane: PaneId,
        cols: u16,
        rows: u16,
    ) {
        daemon
            .send(ServerMessage::PaneResized {
                pane,
                size: (cols, rows),
            })
            .expect("the app is listening");
        app.poll_daemon();
    }

    /// The `ResizePane` sizes this app sent for `pane`, in order.
    fn asked_sizes(sent: &Receiver<ClientMessage>, pane: PaneId) -> Vec<(u16, u16)> {
        sent.try_iter()
            .filter_map(|message| match message {
                ClientMessage::ResizePane { pane: p, size } if p == pane => Some(size),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_pane_is_drawn_at_the_size_its_daemon_says() {
        let (mut app, project, daemon, sent) = attached_app();
        let pane = spawn_several(&mut app, &daemon, project, 1)[0];
        say_size(&mut app, &daemon, pane, 50, 20);
        assert_eq!(app.panes[&pane].backend.size(), Size::new(50, 20));

        let mut terminal = a_terminal();
        drawn(&mut app, &mut terminal);
        app.resize_panes();

        assert_eq!(
            app.panes[&pane].backend.size(),
            Size::new(50, 20),
            "its tile is asked for, not taken"
        );
        let asked = asked_sizes(&sent, pane);
        assert_eq!(asked.len(), 1, "{asked:?}");
        assert_ne!(asked[0], (50, 20), "the tile's own size was asked");

        app.resize_panes();
        assert!(asked_sizes(&sent, pane).is_empty(), "asked once, not every frame");
    }

    #[test]
    fn with_an_old_daemon_a_pane_is_resized_here_as_before() {
        let (mut app, project, daemon, sent) = attached_app();
        let pane = spawn_several(&mut app, &daemon, project, 1)[0];

        let mut terminal = a_terminal();
        drawn(&mut app, &mut terminal);
        app.resize_panes();

        let asked = asked_sizes(&sent, pane);
        assert_eq!(asked.len(), 1);
        assert_eq!(
            app.panes[&pane].backend.size(),
            Size::new(asked[0].0, asked[0].1),
            "a daemon that never says a size leaves it to this client"
        );
    }

    #[test]
    fn a_pane_leaving_the_screen_withdraws_its_size_and_asks_again() {
        let (mut app, project, daemon, sent) = attached_app();
        let panes = spawn_several(&mut app, &daemon, project, 2);
        for pane in &panes {
            say_size(&mut app, &daemon, *pane, 40, 12);
        }
        send_tabs(&mut app, &daemon, project, &[&panes[..1], &panes[1..]]);
        app.focus_pane(panes[0]);
        let mut terminal = a_terminal();
        drawn(&mut app, &mut terminal);
        app.resize_panes();
        let _ = sent.try_iter().count();

        app.focus_pane(panes[1]);
        drawn(&mut app, &mut terminal);
        app.resize_panes();

        let messages: Vec<ClientMessage> = sent.try_iter().collect();
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, ClientMessage::HidePane { pane } if *pane == panes[0])),
            "the pane now off screen withdraws its size: {messages:?}"
        );

        app.focus_pane(panes[0]);
        drawn(&mut app, &mut terminal);
        app.resize_panes();
        assert_eq!(asked_sizes(&sent, panes[0]).len(), 1, "and asks again when shown");
    }

    #[test]
    fn the_repaint_after_the_daemon_resizes_a_pane_is_not_work() {
        let (mut app, project, daemon, _sent) = attached_app();
        let clock = hand_clock(&mut app);
        let panes = spawn_several(&mut app, &daemon, project, 2);
        let background = panes[0];
        say_size(&mut app, &daemon, background, 40, 12);
        advance(&clock, Duration::from_secs(4));
        app.poll_panes();
        assert_eq!(status_of(&app, background), PaneStatus::Idle);

        say_size(&mut app, &daemon, background, 60, 20);
        advance(&clock, Duration::from_millis(50));
        print(&mut app, &daemon, background, b"\x1b[H\x1b[2Jrepainted\r\n");
        for _ in 0..20 {
            advance(&clock, Duration::from_millis(100));
            app.poll_panes();
            assert_eq!(status_of(&app, background), PaneStatus::Idle);
        }
    }
```

`attached_app`, `spawn_several`, `send_tabs`, `a_terminal`, `drawn`, `hand_clock`, `advance`, `print` and `status_of` already exist in this module. If `focus_pane` does not switch the tab on screen in this setup, use whatever the existing tab tests use (`a_click_ends_tab_mode`, `the_daemons_snapshot_decides_which_tab_each_pane_is_on`) to show the other tab.

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p dispatch -- a_pane_is_drawn_at_the_size a_pane_leaving_the_screen the_repaint_after_the_daemon with_an_old_daemon`
Expected: the size-aware tests FAIL (`PaneResized` is ignored, and the emulator follows the tile). `with_an_old_daemon…` passes already and must keep passing.

- [ ] **Step 3: Implement the backend**

In `dispatch/src/backend.rs`:

1. `RemotePane` gains two fields:

   ```rust
       /// The size this client last asked the daemon for, while the pane is
       /// on this client's screen.
       asked: Option<Size>,
       /// Whether the daemon has said what size the pane is. One that does
       /// decides it; one too old to never will, and this client sizes its
       /// own copy as it always did.
       sized_by_daemon: bool,
   ```

   with `asked: None, sized_by_daemon: false` in `RemotePane::new`, and:

   ```rust
       /// Takes the size the daemon says the pane now is.
       pub fn resized(&mut self, size: Size) -> Result<()> {
           self.terminal
               .resize(size)
               .context("failed to resize a pane")?;
           self.size = size;
           self.sized_by_daemon = true;
           Ok(())
       }
   ```

2. Replace `Backend::resize` with:

   ```rust
       /// Fits the pane to a tile of `size`.
       ///
       /// Returns whether its screen changed size now. A pane whose daemon
       /// decides sizes only asks here, and changes size when the daemon says
       /// so.
       pub fn fit(&mut self, size: Size) -> Result<bool> {
           match self {
               Self::Local(session) => {
                   if session.size() == size {
                       return Ok(false);
                   }
                   session.resize(size).context("failed to resize a pane")?;
                   Ok(true)
               }
               Self::Remote(remote) => {
                   if remote.asked == Some(size) {
                       return Ok(false);
                   }
                   // A daemon too old to say what size the pane is: this
                   // client's copy is resized here, this frame, as it always
                   // was.
                   let now = !remote.sized_by_daemon;
                   if now {
                       remote
                           .terminal
                           .resize(size)
                           .context("failed to resize a pane")?;
                       remote.size = size;
                   }
                   remote.asked = Some(size);
                   remote.daemon.send(ClientMessage::ResizePane {
                       pane: remote.id,
                       size: (size.cols, size.rows),
                   });
                   Ok(now)
               }
           }
       }

       /// Stops this client's tile counting towards the pane's size, now the
       /// pane is off this client's screen.
       pub fn hide(&mut self) {
           if let Self::Remote(remote) = self
               && remote.asked.take().is_some()
               && remote.sized_by_daemon
           {
               remote
                   .daemon
                   .send(ClientMessage::HidePane { pane: remote.id });
           }
       }
   ```

- [ ] **Step 4: Implement the app**

In `dispatch/src/app.rs`:

1. A field on `App`, with `fitted: HashSet::new(),` in `App::new`:

   ```rust
       /// The panes this client last fitted to its screen, so one that leaves
       /// the screen can withdraw its size.
       fitted: HashSet<PaneId>,
   ```

2. `resize_panes` becomes:

   ```rust
       /// Fits every visible pane to the rectangle it now occupies, and
       /// withdraws this client's size for any pane no longer on screen.
       ///
       /// A child that is not told its new size redraws to the old one, which
       /// is the most visible bug this layer can have.
       pub fn resize_panes(&mut self) {
           let now = self.now();
           let layout = self.layout.clone();

           let showing: HashSet<PaneId> = layout.iter().map(|(id, _)| *id).collect();
           let gone: Vec<PaneId> = self.fitted.difference(&showing).copied().collect();
           for id in gone {
               if let Some(pane) = self.panes.get_mut(&id) {
                   pane.backend.hide();
               }
           }
           self.fitted = showing;

           for (id, rect) in layout {
               // (keep the existing comment about an unreachable machine)
               if !self.can_reach_pane(id) {
                   continue;
               }

               let Some(pane) = self.panes.get_mut(&id) else {
                   continue;
               };

               match pane.backend.fit(Size::new(rect.width, rect.height)) {
                   Ok(false) => {}
                   Ok(true) => {
                       // The program repaints to fit, and that repaint is this
                       // resize's doing rather than the program at work.
                       pane.activity.resized(now);
                       if let Ok(screen) = pane.reader.read(pane.backend.terminal()) {
                           pane.screen = screen;
                       }
                   }
                   Err(error) => tracing::warn!(%error, "failed to resize a pane"),
               }
           }
       }
   ```

   Keep the existing comment explaining why an unreachable machine's pane is skipped silently.

3. In `apply_from`, replace Task 1's placeholder arm with:

   ```rust
               ServerMessage::PaneResized { pane, size } => {
                   let now = self.now();
                   let Some(target) = self.panes.get_mut(&pane) else {
                       return false;
                   };
                   let Backend::Remote(remote) = &mut target.backend else {
                       return false;
                   };
                   // The pty is this size now, and every byte that follows was
                   // drawn for it: the copy takes it here, in order with the
                   // output, never ahead of it.
                   if let Err(error) = remote.resized(Size::new(size.0, size.1)) {
                       tracing::warn!(%error, "failed to resize a pane");
                       return false;
                   }
                   // The repaint that follows is the resize's doing.
                   target.activity.resized(now);
                   if let Ok(screen) = target.reader.read(target.backend.terminal()) {
                       target.screen = screen;
                   }
                   true
               }
   ```

   Task 5 adds the attachment's `size_aware` flag here too. Leave room for it, but don't add it now.

4. Every other caller of `Backend::resize` (the compiler names them) moves to `fit`, ignoring its `bool` where it has no use.

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test -p dispatch`
Expected: PASS, including the 4 new tests and the existing `the_repaint_after_a_resize_is_not_work`.

- [ ] **Step 6: Lint, including for Windows, and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo clippy -p dispatch --all-targets --target x86_64-pc-windows-gnu -- -D warnings`

```bash
git add dispatch/src/backend.rs dispatch/src/app.rs
git commit -F - <<'EOF'
fix(dispatch): draw a daemon's pane at the size the daemon says it is

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2
EOF
```

---

### Task 5: The window says when it is in use, and ignores clicks past a pane's edge

**Files:**
- Modify: `dispatch/src/app.rs`

**Interfaces:**
- Consumes: Task 1's `ClientMessage::Active`; Task 4's `PaneResized` arm.
- Produces:
  - `Attachment::size_aware: bool`;
  - `App::active_sent: Option<Instant>`;
  - `const ACTIVE_EVERY: Duration = Duration::from_millis(500)`;
  - `App::note_active(&mut self)`.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `dispatch/src/app.rs` (Task 4's `say_size` helper is there):

```rust
    /// How many `Active` messages this app sent.
    fn actives(sent: &Receiver<ClientMessage>) -> usize {
        sent.try_iter()
            .filter(|message| matches!(message, ClientMessage::Active))
            .count()
    }

    #[test]
    fn input_tells_a_size_aware_daemon_this_window_is_in_use() {
        let (mut app, project, daemon, sent) = attached_app();
        let clock = hand_clock(&mut app);
        let pane = spawn_several(&mut app, &daemon, project, 1)[0];
        say_size(&mut app, &daemon, pane, 40, 12);
        let _ = sent.try_iter().count();

        press(&mut app, KeyCode::Char('x'));
        assert_eq!(actives(&sent), 1);

        advance(&clock, Duration::from_millis(100));
        press(&mut app, KeyCode::Char('y'));
        assert_eq!(actives(&sent), 0, "at most once per half second");

        advance(&clock, Duration::from_millis(500));
        press(&mut app, KeyCode::Char('z'));
        assert_eq!(actives(&sent), 1);
    }

    #[test]
    fn input_says_nothing_to_a_daemon_too_old_to_size_panes() {
        let (mut app, project, daemon, sent) = attached_app();
        spawn_several(&mut app, &daemon, project, 1);
        let _ = sent.try_iter().count();

        press(&mut app, KeyCode::Char('x'));

        assert_eq!(actives(&sent), 0);
    }

    #[test]
    fn a_click_past_the_edge_of_a_smaller_pane_goes_nowhere() {
        let (mut app, project, daemon, sent) = attached_app();
        let pane = spawn_several(&mut app, &daemon, project, 1)[0];
        say_size(&mut app, &daemon, pane, 10, 5);
        // The program asks for mouse reports, as an agent's interface does.
        print(&mut app, &daemon, pane, b"\x1b[?1000h");
        let _ = sent.try_iter().count();

        let click = |col, row| MouseInput {
            action: dispatch_pty::MouseAction::Press,
            button: dispatch_pty::MouseButton::Left,
            col,
            row,
            modifiers: dispatch_pty::Modifiers::default(),
        };

        app.send_mouse(pane, click(20, 2));
        assert!(
            !sent
                .try_iter()
                .any(|m| matches!(m, ClientMessage::WritePane { .. })),
            "past the pane's right edge: nothing is sent"
        );

        app.send_mouse(pane, click(3, 2));
        assert!(
            sent.try_iter()
                .any(|m| matches!(m, ClientMessage::WritePane { .. })),
            "inside it: the click is the program's"
        );
    }
```

If `Modifiers::default()` does not exist, use how the app's own mouse code builds `MouseInput`. If `MouseInput` is not in scope in the tests module, import it from `dispatch_pty`.

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p dispatch -- input_tells input_says a_click_past`
Expected: `input_tells…` fails (no `Active` is sent), and `a_click_past…` fails (the click is sent). `input_says…` passes already.

- [ ] **Step 3: Implement**

In `dispatch/src/app.rs`:

1. `Attachment` gains:

   ```rust
       /// Whether this daemon decides pane sizes, known once it has said one.
       /// A daemon from before that never will, and is told nothing it would
       /// not understand.
       size_aware: bool,
   ```

   Set it to `false` in every `Attachment { .. }` construction (the compiler names them). In `sync_attachment`, where a new generation is taken (`attachment.generation = generation;`), add `attachment.size_aware = false;`. A new connection may be to a different daemon.

2. In `apply_from`'s `PaneResized` arm (from Task 4), first mark the attachment it came from:

   ```rust
                   if let Mode::Attached(attachments) = &mut self.mode
                       && let Some(attachment) =
                           attachments.iter_mut().find(|a| a.device == device)
                   {
                       attachment.size_aware = true;
                   }
   ```

3. A constant near the other timing constants:

   ```rust
   /// How often a window tells its daemons it is the one in use, at most.
   ///
   /// Often enough that the size follows the user within a moment of turning
   /// to a window; rare enough that typing does not send one per keystroke.
   const ACTIVE_EVERY: Duration = Duration::from_millis(500);
   ```

   Then an `App` field `active_sent: Option<Instant>` (`None` in `App::new`), and:

   ```rust
       /// Tells every daemon that decides sizes that this window is the one in
       /// use, at most once per [`ACTIVE_EVERY`].
       fn note_active(&mut self) {
           let now = self.now();
           if self
               .active_sent
               .is_some_and(|at| now.duration_since(at) < ACTIVE_EVERY)
           {
               return;
           }
           let Mode::Attached(attachments) = &self.mode else {
               return;
           };
           let mut sent = false;
           for attachment in attachments.iter().filter(|a| a.size_aware) {
               attachment.client.send(ClientMessage::Active);
               sent = true;
           }
           if sent {
               self.active_sent = Some(now);
           }
       }
   ```

4. In `App::handle`, before `let handled = self.act_on(event, area);`:

   ```rust
           // Anything the user does in this window makes it the one in use,
           // including what never reaches a daemon: moving focus, switching
           // tabs, resizing the window.
           if matches!(
               event,
               Event::Key(_) | Event::Mouse(_) | Event::Paste(_) | Event::Resize(..) | Event::FocusGained
           ) {
               self.note_active();
           }
   ```

5. In `send_mouse`, right after `let size = pane.backend.size();`:

   ```rust
           // Another window can have made the pane smaller than its tile here:
           // a pointer past its edge is over nothing the program drew. A wheel
           // there still scrolls this client's own copy.
           if input.col >= size.cols || input.row >= size.rows {
               if let Some(rows) = wheel {
                   self.scroll_pane(id, rows);
               }
               return;
           }
   ```

   If the borrow checker objects because `pane` is still borrowed, copy `size` out and end the borrow before the check. For example, read `size` in a small block and look `pane` up again afterwards.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p dispatch`
Expected: PASS, including the 3 new tests.

- [ ] **Step 5: Run the whole workspace, lint, and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings && cargo test --workspace --no-fail-fast`

```bash
git add dispatch/src/app.rs
git commit -F - <<'EOF'
feat(dispatch): tell the daemon which window is in use, and ignore clicks past a pane's edge

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2
EOF
```

---

### Task 6: Documentation

**Files:**
- Modify: `README.md`
- Modify: `docs/superpowers/specs/2026-09-28-shared-pane-size-design.md` (status line)

- [ ] **Step 1: README**

In `README.md`, `## Running the daemon`, the paragraph beginning "Several clients can attach at once and see the same panes." Keep it, and add after its first sentence:

```markdown
Each pane takes the size of the window you used last: the one you typed or
clicked in, resized, or opened most recently. The other windows show it at
that size, with blank space around it or its edges cut off to fit their tile.
```

Reflow the paragraph to the file's width (about 80 columns).

- [ ] **Step 2: Mark the spec implemented**

Change `Status: designed, not yet planned.` to ``Status: implemented on branch `fix/shared-pane-size`.``

- [ ] **Step 3: Commit**

```bash
git add README.md docs/superpowers/specs/2026-09-28-shared-pane-size-design.md
git commit -F - <<'EOF'
docs: say that the window in use decides a shared pane's size

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2
EOF
```
