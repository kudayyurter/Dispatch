# Shared pane size: the window you are using decides

Status: implemented on branch `fix/shared-pane-size`.
Date: 2026-09-28.
Branch: `fix/shared-pane-size`, cut from `main` at 4045531. Independent of
`feat/harness-settings` (PR #4).

## The problem

The README promises that "several clients can attach at once and see the same
panes". They can, but they garble each other's panes.

The user sent a screenshot of two panes (agy and opencode) taken just after
closing a third. opencode, a full-screen program, was drawn as if for a much
narrower pane, its lines wrapped mid-word ("What is t / he tech stack"), and
agy's old footer was left stuck in the middle of its pane.

The daemon's log shows two interface clients attached at once: one left open
from the night before (attached 01:45 UTC) and the one the user was testing in
(16:31 UTC). The screenshot was taken at 16:35 UTC. The first client detached
at 16:38:58 UTC.

What goes wrong:

1. Every attached client lays out the project's panes in its own window and
   sends `ResizePane` for each pane whose tile changed (`App::resize_panes`,
   `dispatch/src/app.rs`).
2. The daemon applies each `ResizePane` straight to the pty
   (`crates/dispatch-daemon/src/session.rs`, the `ResizePane` arm). It keeps no
   record of who asked and tells no other client. Whichever window spoke last
   decides the size the program sees.
3. Each client resizes its own emulator to its own tile as it sends the
   request (`Backend::resize`, `dispatch/src/backend.rs`). The program then
   draws for the other window's size into this window's emulator.
4. The losing client never corrects it: `resize_panes` skips a pane whose
   emulator already has the tile's size, so it believes it already asked.

Opening or closing a pane changes the layout in every attached window at
once, so every window resizes every pane. That is why closing the bottom pane
broke the other two.

### Not in this piece of work

- **Marking a pane on screen** when it is padded or cut off because another
  window sized it.
- **A command to detach the other windows.**
- **A glitch with one window alone.** If the user's single-window retest shows
  one, it is a separate bug.

## Decisions taken with the user

- **The window you are using decides a pane's size,** tmux's default, chosen
  over "smallest window wins" (a forgotten small window would shrink every
  agent) and over "last writer wins, but tell everyone" (windows could flip the
  size back and forth).
- **Every window draws the pane at its real size,** padded or cut off in its
  tile, instead of pretending it has its own size.

## The rule

### The active window

The daemon ranks its interface clients by when each was last used. A client's
rank is a number from a counter the daemon bumps on each activity, so the most
recently used client has the highest.

These `ClientMessage`s from an interface client count as activity:

- `Subscribe` (a window that has just attached is where the user is);
- `WritePane` (typing, pasting, mouse reports);
- `OpenProject`, `CloseProject`, `SpawnPane`, `ClosePane`;
- `MovePane`, `RenameTab`, `CloseTab`, `MoveTab`;
- `DelegateDecision`;
- `Active` (new, below).

These do not: `Hello`, `Ping`, `ResizePane`, `HidePane` (new), and anything
from a client whose role is `Delegate`. A `ResizePane` does not count because
a window resizes its panes when *another* window changes the layout, and that
is not the user using it.

A client sends **`Active`** when the user gives it input the daemon would
otherwise not see: a key or mouse event handled by the client itself (moving
focus, switching tabs, opening a picker), a paste, a resize of its own
terminal, and the terminal regaining focus if focus events are reported. It is
throttled: at most one `Active` per 500 ms. Input that already sends one of
the messages above needs no `Active` as well, but sending one anyway is
harmless.

### Which size a pane gets

- The daemon remembers, for every interface client and every pane, the size
  that client last asked for with `ResizePane` (or with `SpawnPane`, for the
  pane it asked to start).
- A pane's size is the size asked for by the highest-ranked client that has
  one for it.
- It is worked out again when a client asks for a size, when a client's rank
  changes, when a client withdraws its size with `HidePane`, and when a client
  detaches (all its sizes are dropped).
- A pane no client has a size for keeps the size it has.
- When the worked-out size differs from the pty's, the daemon resizes the pty
  and then sends `PaneResized` to every subscribed interface client.

A subagent starts at its `DelegateRequest` size, as today, and follows the rule
once a window shows it.

## The protocol

| Message | Direction | Fields | Meaning |
|---|---|---|---|
| `PaneResized` | daemon → interface clients | `pane: PaneId`, `size: (u16, u16)` | the pane's pty is now this size |
| `Active` | client → daemon | none | the user is using this window |
| `HidePane` | client → daemon | `pane: PaneId` | this window no longer shows the pane; stop counting its size |

`ResizePane` keeps its fields and now means "the size of my tile for this
pane", which the daemon may or may not apply.

`PaneResized` is sent:

- to every subscribed interface client whenever the pty's size changes;
- in `Subscribe`'s catch-up, for every pane, straight after its `PaneSpawned`
  and **before** its replayed output, so the replay is drawn at the size the
  pane has now;
- straight after the `PaneSpawned` broadcast for a new pane, including a
  subagent.

`VERSION` stays 1.1: new variants land in each side's `#[serde(other)]
Unknown`, which the daemon logs at debug level and the client ignores.

## The client

### Two sizes per pane

A client keeps, for each remote pane:

- **asked**: the size it last sent in `ResizePane` or `SpawnPane`, or none;
- **real**: the emulator's size, which is the pty's size as last told by
  `PaneResized`.

`resize_panes` compares each visible pane's tile with *asked*, not with the
emulator: when they differ it sends `ResizePane` and records *asked*. It no
longer resizes the emulator itself once the daemon is size-aware (below).

A pane that was in the layout last frame and is not now (another tab, folded
under its parent, left the grid) gets a `HidePane`, and its *asked* is cleared.
When it is shown again, its tile differs from the cleared *asked*, and the
client asks again.

### Resizing the emulator only on `PaneResized`

On `PaneResized` the client resizes that pane's emulator to the size given,
and tells the pane's activity tracker it was resized (`activity.resized`,
which `resize_panes` calls today). The program's repaint follows the pty's
resize, not the request, and must not be taken for the agent at work.

The daemon resizes the pty before sending `PaneResized`, and everything travels
on one ordered connection. So output the program drew at the old size reaches
the client before the `PaneResized`, and output drawn at the new size after it.
Every byte lands in an emulator of the size it was drawn for. That ordering is
the fix.

The cost: when a window's own tile changes, the pane's contents reflow one
round trip later. That is under a millisecond for a local daemon, and one
network hop for a pane on another machine.

### Old daemons

An older daemon never sends `PaneResized`. A client treats a daemon as
**size-aware** once it has received a `PaneResized` from it. A new daemon sends
one for every pane in `Subscribe`'s catch-up, so this is known at attach. Until
then, the client resizes the emulator itself when it sends `ResizePane`,
exactly as today, and sends no `Active` or `HidePane`.

Size-awareness is per daemon connection: a client attached to several machines
tracks it for each.

### Drawing a pane that does not fit its tile

`PaneWidget` already draws `min(tile, screen)` in each direction, leaving the
rest of the tile blank and cutting off what does not fit. In addition:

- The terminal cursor is not placed when the pane's cursor falls outside the
  tile.
- A mouse event is forwarded to the pane only when it falls inside both the
  tile and the pane's real size.

### Standalone

A standalone Dispatch (no daemon) is unchanged: one window, and `PtySession`
resizes pty and emulator together.

## The daemon

- `Client` gains a `rank: u64`, set from a daemon-wide counter on each
  activity.
- The daemon keeps `asked: HashMap<(ClientId, PaneId), Size>`.
- `ResizePane` records the asking client's size and re-works the pane's size.
- `SpawnPane` records the asking client's size for the new pane.
- `HidePane` removes the client's entry for the pane and re-works its size.
- An activity message from an interface client sets its rank and re-works the
  size of every pane that client has an entry for.
- A detach removes the client's entries and re-works each pane they covered.
- Closing a pane removes every entry for it.
- Re-working a pane: find the highest-ranked client with an entry. If its size
  differs from the pty's, resize the pty and broadcast `PaneResized`.
- The daemon's own emulator for the pane (inside `Pty`) follows the pty, as
  today.

## Edge cases

- **`dispatch delegate` connections** (`Role::Delegate`) never rank and never
  size a pane: they send no `ResizePane`, and their messages are not activity.
- **A window attached to several machines** is ranked by each daemon on its
  own.
- **Two windows on the same tile size** agree, and nothing changes.
- **The active window detaches:** its entries go, and each pane it sized
  follows the next most recently used window that shows it.
- **An old window with a new daemon:** it never sends `Active` or `HidePane`,
  and ignores `PaneResized`. Its `Subscribe`, typing and other actions still
  rank it. While it is not the active window, its own emulator may disagree
  with the pty, as today. Upgrading the window fixes that.

## Testing

**`dispatch-proto`**
- `PaneResized`, `Active` and `HidePane` round-trip.
- A peer that does not know them reads each as `Unknown`.

**`dispatch-daemon`**
- Two interface clients ask different sizes for one pane: the pty takes the
  more recently active one's, and both clients are sent `PaneResized` with it.
- `Active` from the other client moves the size to its request.
- `WritePane` counts as activity; `ResizePane` alone does not.
- Detaching the active client hands the pane to the next most recent one.
- `HidePane` withdraws a request; a pane with no requests keeps its size.
- `Subscribe`'s catch-up sends `PaneResized` for every pane, after
  `PaneSpawned` and before the replayed output.
- A new pane's `PaneSpawned` is followed by `PaneResized`.
- A delegate client's messages never rank it.

**`dispatch` (app)**
- `PaneResized` resizes the emulator to the size given, not to the tile, and
  marks the pane resized for its activity tracker.
- Once size-aware, a tile change sends `ResizePane` and leaves the emulator's
  size alone; before, it resizes the emulator as today.
- A pane leaving the layout sends `HidePane`; showing it again sends
  `ResizePane`.
- Input sends `Active`, at most once per 500 ms, and only to a size-aware
  daemon.
- The cursor is not placed outside the tile; a click outside the pane's real
  area is not forwarded.

**By hand, before merging**
- Two windows of different sizes on one daemon: open and close panes in one,
  then type in the other. Each time, the window being used shows every agent
  drawn correctly, and the other shows it padded or cut off, never garbled.
