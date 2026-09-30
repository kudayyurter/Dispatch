# Mouse interaction and unified Settings: design handoff

Date: 2026-09-30
Status: proposed implementation design; product code is not changed by this handoff.
Audience: the next implementer working on Dispatch's Rust terminal interface.

## 1. User intent and scope

Make everyday operations usable with a mouse, keep keyboard operation complete,
and move shortcut discovery and editing into a searchable Settings workspace.
The bottom bar must be a quiet status surface, not a list of keybindings.
Settings must include keyboard, mouse, model, agent, and appearance preferences.
Preserve the existing terminal-derived palette and compact workspace identity.

The user has already assigned all seven improvements in
[the TUI polish design](2026-09-30-tui-polish-design.md). They are dependencies,
not tasks to repeat. Inspect the receiving branch before implementation: the
polish document says unimplemented, but the inspected source already contains
some of its changes, including shared `Chrome` and attention overlay support.

| Existing assignment | Integration required here; do not rebuild |
|---|---|
| Focus-glide continuity | Reuse the corrected focus animation for mouse focus. |
| Attention navigation | Make its existing picker/command clickable from status. |
| Responsive sidebar and dialogs | Reuse sidebar dragging, remembered width, drawer, and safe sizing. |
| Consistent dialog styling | Use the existing shared `Chrome` roles for new controls. |
| Discoverability and command help | Reuse the action catalog/help; add editing and Settings entry points. |
| Visible-animation scheduling | Register new transitions with the existing scheduler. |
| Readable states and contrast | Reuse status labels and palette validation. |

**Explicit replacement:** the polish spec's footer shortcut reminders, including
the attention shortcut suffix, are superseded by this design. Keep those actions
and bindings; expose shortcuts in menus, command help, and Settings instead.
Sidebar-divider dragging is already assigned. Only its integration with common
pointer capture belongs here. Pane-divider resizing and tab dragging are new.

## 2. Workspace composition

Retain the project tree, tab strip, and tiled terminals. Put a visible Settings
button at the right end of the bottom bar, with a text fallback rather than a
Nerd Font glyph alone. Add Settings to the existing command catalog. Do not
choose a new default chord until checking the merged keymap for conflicts.

Normal footer:

```text
Connected · 4 working · 2 need attention                 [Activity] [Settings]
```

Only count statuses Dispatch actually knows. In standalone mode, omit the
connection claim; with unreachable machines, show their connection state.
The attention count opens the existing attention picker. Activity opens a
compact current-status list of panes; it is not a new persistent event-log system.
At narrow widths, drop routine counts first, retaining urgent state and Settings.

Scroll mode shows `Scrollback · 120 lines above live` and a clickable
`Return to live`. Other keyboard modes show a compact mode label and an exit
control; they do not restore the shortcut wall. Lock must remain visibly marked
and escapable through the existing keyboard mechanism. Mouse behavior in Lock
must match its documented contract; do not silently redefine it.
Transient success messages expire; errors and pending attention remain reachable.

## 3. Mouse behavior contract

| Target | Left click | Secondary interaction |
|---|---|---|
| Project label | Select project without folding it | Chevron alone expands/collapses; menu opens project actions. |
| Pane row | Focus pane using existing reveal/tab-switch behavior | Right-click opens the same actions as its visible menu. |
| Pane header | Focus pane | Double-click toggles zoom; visible `…` opens actions. |
| Tab | Switch tab | Drag reorders within the project; menu offers rename, reorder, close. |
| New-tab/new-pane control | Open the existing picker | No alternative launch behavior hidden in mouse handling. |
| Sidebar divider | Use the polish work's resize behavior | Double-click restores its default width. |
| Pane divider | Begin split resize | Menu/keyboard commands offer resize and reset alternatives. |
| Setting row/control | Focus or activate its explicit control | Wheel scrolls the list, not the value under the pointer. |
| Picker row | Select | Explicit Open/Choose activates; double-click may also activate. |
| Menu item/button | Activate on release inside the pressed target | Leaving the target cancels activation. |

Click-to-focus is the default. Hover highlights controls but never redirects
typing. Offer an optional Focus follows pointer setting for users who want the
old behavior. Keep this separate from the existing `hover_claims_panes` option,
which controls shared-pane size ownership across client windows.

Pane menus contain relevant actions: zoom/restore, rename where supported, move
to tab, select text, and close. Close is labeled `Close pane and stop agent` when
that is the actual effect. Reuse existing destructive-action policy; do not add
a generic confirmation to every click. Right-click is never treated as left-click.

### Input ownership

Resolve pointer events in this order: captured gesture, topmost modal/menu,
Dispatch controls, then terminal content. Hit regions come from the same layout
used to render the visible frame, including clipping and scrolling.

- Dispatch owns borders, headers, menus, settings, and dividers.
- Mouse-aware child applications own events inside their content region.
- Without child mouse tracking, dragging selects local text and the wheel
  scrolls Dispatch's copy of the pane history.
- Provide an explicit Select text action to override child mouse tracking.
  Do not depend on Shift+drag reaching Dispatch; terminal emulators may consume it.
- Left-click inside an unfocused pane focuses it and forwards the event if the
  child requests it. Clicking its header focuses without child input.
- Capture a drag to its original owner until release, even across panes.
  Route the matching release to that owner; never leave a child with a stuck press.
- Cancel local drag safely if its pane disappears, focus is lost, or a terminal
  resize invalidates the layout. Release child capture safely when still reachable.
- An open modal consumes all input. An outside click may dismiss a menu, but
  must not activate the control behind it. Settings never discards edits outside-click.
- Pointer hover alone must not claim remote/shared pane sizes.

Text selection is anchored to content coordinates, not moving screen rows.
New output must not silently change what will be copied. Provide Copy and Cancel
controls. Clipboard transport belongs behind a platform adapter; show a useful
failure if unavailable, and never claim a copy succeeded without confirmation
from that adapter. Avoid automatic remote clipboard writes.

### Dragging and resize limits

Begin a drag after movement of at least one terminal cell. Show a tab insertion
marker; commit reorder only on release. Use existing tab-order commands/protocol,
not a separate client-only order that disagrees with attached clients.

Pane resizing preserves the existing balanced-grid topology. Store row weights
and per-row column weights keyed by project/tab and pane membership, locally in
UI state. A horizontal divider changes adjacent row weights; a vertical divider
changes adjacent columns in its row. Reset weights when membership changes.
Clamp toward a minimum content area of 20 columns by 5 rows; if the terminal
cannot provide that, use existing compact layout behavior and disable invalid
resize handles. This is not an arbitrary split-tree rewrite.

Draw resize feedback immediately, coalesce PTY resize requests, and send the final
size on release through the existing active-client size-ownership mechanism.
Do not animate geometry behind the pointer or queue every mouse movement.

## 4. Settings workspace

Use a centered overlay on terminals at least 100 columns by 28 rows, capped at
110 columns by 34 rows with a one-cell outer margin. Below that use the full
available body. Below 72 columns replace category navigation with a category
picker/back control; fields remain one column and scroll vertically. At unusably
small sizes show a compact resize message and a working close control.

```text
┌ Settings ───────────────────────────────────────────────── [×] ┐
│ Search settings…                                              │
├───────────────────┬───────────────────────────────────────────┤
│ General           │ Keyboard shortcuts                        │
│ Mouse & layout    │ Search actions…       Context: [All     ▾]│
│ Keyboard          │                                           │
│ Appearance        │ Action            Shortcut       Context  │
│ Agents & models   │ New pane          Alt N          Normal   │
│ Notifications     │ Next attention    Alt A          Normal   │
│ Machines          │ Open settings     Unassigned     Normal   │
│ Advanced          │                                           │
│                   │ [Record shortcut] [Remove] [Reset]        │
├───────────────────┴───────────────────────────────────────────┤
│ 2 changes in Keyboard                    [Discard] [Apply]     │
└───────────────────────────────────────────────────────────────┘
```

The shortcut examples are illustrative; render the effective merged keymap.
Search matches setting names, descriptions, and action names across categories.
Each result includes its category and jumps to the matching control. Empty
results retain the query and offer Clear search. Remember category and scroll
position in the current client session, not as shared global navigation state.

Tab/Shift-Tab moves focus, arrows operate lists, Enter activates, and Esc closes
the innermost popover before the workspace. Every mouse operation has a keyboard
path. Focus and hover have distinct styles. Labels and padded control regions
are clickable; do not require clicking a single glyph.

### Categories and scope

| Category | First delivery |
|---|---|
| General | Existing supported shell/startup options, with application timing. |
| Mouse & layout | Click/hover focus, shared-window hover ownership, existing sidebar preferences, resize reset. |
| Keyboard | Effective shortcuts, contexts, recording, conflict resolution, reset. |
| Appearance | Follow terminal, bundled dark/light presets, accent, existing motion switch, icon fallback. |
| Agents & models | Per-agent defaults, supported model/effort/permission controls, named launch profiles. |
| Notifications | Attention pulse and bell preferences; reuse attention state detection. |
| Machines | Existing connection actions and status, with explicit machine identity. |
| Advanced | Configuration paths, diagnostics, existing reset capabilities. |

Do not render fake editable settings for unsupported capabilities. Import/export,
remote configuration mutation, automatic model discovery, and a notification
history database are future work. Machines initially exposes existing management
actions and read-only configuration scope, not a new remote settings protocol.

## 5. Keyboard editor

Rows show human-readable action, one or more effective bindings, context, and
source (built-in, config file, UI override). Reuse the existing command metadata
and help catalog; there must not be a second manually maintained action list.

Record opens a modal that consumes input without executing any command. Show the
canonical chord the terminal delivered, then explicit Use shortcut and Cancel
buttons. Esc cancels recording and is reserved for UI escape; use the existing
lock escape protections. Initial delivery edits chords within existing contexts,
not arbitrary new multi-stroke sequences. Display existing mode-entry paths.

Check conflicts only where contexts overlap. Offer Replace existing binding,
Choose another, or Cancel; never silently remove an unrelated binding. Warn when
a Normal binding intercepts a key otherwise sent to the child. Preserve multiple
bindings per action. Validate escape/lock recovery with the existing keymap rules.

Remove disables that effective binding. Reset removes the UI override and reveals
the underlying file/default value; label this distinction. A separate Restore
built-in action writes an explicit override if the config file differs. Applying
key changes rebuilds the router, clears any pending prefix/mode, and refreshes
all displayed shortcuts. Unrelated child keystrokes continue to pass through.

## 6. Agent, model, and appearance settings

Agent pages are driven by existing harness definitions and validation. Reuse
`SettingsForm` behavior and dependency limits; do not hardcode current model names
or create a separate provider registry. Support custom identifiers where the
harness permits them. Disabled effort controls explain the model dependency.

Distinguish Agent default, Saved default, and an explicit chosen value. Display
permission settings using each harness's actual semantics; unsupported controls
are absent rather than pretending every agent has the same permission model.

The page has a Defaults editor and a Profiles list. A named profile stores a
harness ID and validated setting overrides, not arbitrary shell command strings.
New pane can choose a profile and make one-launch overrides. Resolve values in
this order: harness defaults, saved defaults, selected profile, one-launch edits.
Profiles inherit unspecified values and never mutate the saved defaults.
Show the resolved launch settings before starting. Editing a model or permissions
is labeled `Applies to new panes`; never imply running agents are reconfigured.

Appearance previews are client-local and reversible. Discard restores the last
committed theme. Reuse the terminal palette and contrast work. Follow terminal
remains the default. Theme changes affect Dispatch chrome, not terminal font size
or a child application's own styling. Motion uses the existing on/off mechanism.

## 7. Persistence and truthful Apply behavior

Protect hand-edited `config.toml` and harness TOML files: do not serialize them
back and discard comments. Add a UI-managed `preferences.toml` for interface and
key overrides and named profiles. Keep saved agent defaults in the existing
`harness-settings.toml`; reuse `ui.toml` from the polish work for layout state.
Do not store the same preference in two files.

Interface/key precedence is built-in defaults < config.toml < preferences.toml.
Settings shows the effective source. Reset removes only the relevant UI override.
Profiles use the launch precedence above. Remote launch support must respect the
connected daemon's actual capabilities and validation.

Apply commits one editing section at a time to one owning file: Keyboard,
Appearance, Mouse preferences, one agent's Defaults, or one Profile. Changing
sections with unsaved edits offers Apply, Discard, or Keep editing. Layout drag
persistence remains the existing ui.toml operation; it is not part of a global
multi-file Settings transaction. This avoids a misleading all-or-nothing Apply.

Keep a base snapshot plus draft. Under the existing locked atomic store update,
merge unrelated external changes, detect changes to the same edited fields, and
show a conflict instead of overwriting them. Unparseable files are never replaced.
Validate before writing; on failure retain the draft and identify the file/field.
After successful persistence apply live preferences and show any deferred effects.
Other already-open clients need not hot-reload in the first delivery; reopening
Settings reloads the persisted state and labels client-local preview scope.

Application timing is explicit per setting: immediate in this client, new panes,
next client start, or daemon restart. No silent restarts or promises of remote
application. Read-only remote settings identify their machine and limitation.

## 8. Implementation boundaries

- `crates/dispatch-tui`: reusable focusable controls, hit regions, menus, Settings
  view state, shortcut recorder, and pointer gesture state. Reuse `Chrome`.
- `crates/dispatch-config`: typed preference overrides, profiles, provenance,
  merge/validation, and locked persistence. Reuse harness validation and store.
- `dispatch/src/app.rs`: wire commands and backend operations. Extract the new
  settings and pointer orchestration into focused modules rather than extending
  the large app match statements with unrelated widget logic.
- `crates/dispatch-layout`: optional weighted balanced-grid calculation, with
  the existing tile function as default/reset behavior.
- `dispatch/src/tabs.rs` and backend/protocol paths: reuse existing reorder and
  resize authority. Add protocol work only if inspection proves it necessary.

Mouse, keyboard, menus, and command help invoke the same typed actions. Widgets
emit actions rather than writing files, spawning agents, or sending PTY bytes.
Rendering supplies clipped hit rectangles for the frame; a stale rectangle must
not activate a control after a resize or overlay change.

## 9. Delivery sequence and verification

1. Reconcile the seven-change branch. Record which dependency APIs exist and
   integrate them; do not reimplement the polish work from its stale status line.
2. Introduce common pointer ownership and clickable existing overlays. Implement
   click-to-focus and separate project selection from folding.
3. Build Settings navigation/search, preferences persistence, Appearance and
   Keyboard editors. Replace the footer hints with status and entry controls.
4. Integrate agent Defaults, launch Profiles, and existing machine/status actions.
5. Add tab dragging, weighted pane resizing, and local text selection/clipboard.

Acceptance checks:

- Mouse-only: open Settings, change a theme, record a shortcut, Apply, close;
  launch an agent with a profile; switch tabs; reach an agent needing attention.
- Keyboard-only: complete the same supported operations and escape every modal.
- Modal clicks/pastes/recorded shortcuts never reach a child PTY; right-click
  never performs a left-click action; a dismissed menu never clicks through.
- Drag starts in pane A and ends over B: ownership and release remain correct.
  Repeat with a modal opening, a closed pane, lost focus, and terminal resize.
- Mouse-aware child applications retain their own clicking and scrolling;
  explicit selection mode can copy text without executing child actions.
- Footer has no persistent shortcut list in normal, pane, tab, scroll, prefix,
  or session modes; urgent state and mode identity remain visible.
- Settings renders at 120×40, 100×28, 80×24, 60×18, and very small dimensions
  without panics, unreachable close controls, or clipped active fields.
- Test binding conflicts, overrides, reset semantics, duplicate shortcuts in
  separate contexts, lock recovery, and terminal-normalized key combinations.
- Test save failure, corrupt TOML, unrelated concurrent changes, same-field
  conflicts, restart/reload, profile inheritance, and unsupported model options.
- Multiple attached clients: dragging in the active client respects existing
  pane-size ownership; hover and local theme preview cannot resize remote panes.
- Reduced motion leaves every operation usable. Dragging remains immediate.

Use unit/event-sequence tests for routing and persistence, Ratatui buffer tests
for layout, and real-terminal smoke tests for mouse capture, clipboard, SSH, and
terminal key encoding. Report untested environments explicitly. This handoff does
not claim runtime verification or implementation completion.
