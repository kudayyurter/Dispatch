# Harness settings: auto-approve by default, and a model/effort popup

Status: implemented on branch `feat/harness-settings`.
Date: 2026-09-28.
Branch: `feat/harness-settings`, cut from `main` at 4045531.

## The problem

The user asked to "change the default settings in each harness so we can
maximize their features". Asked what that meant, they chose two things:

1. **Fewer permission prompts.** Every agent should start in its
   auto-approve mode by default, for everyone who installs Dispatch.
2. **A popup to choose model and effort.** From the new-pane picker, a key on
   a harness opens a popup where the user can change that harness's default
   model and effort, or pick a model and effort for one pane only.

Today neither is possible. Every shipped harness file launches its agent with
`args = []`, so each agent starts on its own defaults. `claude.toml` and
`codex.toml` declare `[[settings]]` (model, permission mode, effort), but
nothing reads them: no code substitutes a `{model}` into the arguments (only
`{task}` is filled in), and the harness manager never renders them. The
settings are decoration.

### Not in this piece of work

- **Delegation everywhere.** `agy` and `opencode` keep having no `[task]`
  form, and no `gemini` harness is added. Offered, and set aside.
- **Model or effort flags on `dispatch delegate`.** A subagent gets the
  daemon's saved defaults.
- **Showing a pane's model** in its sidebar row or title.
- **Live model lists.** The popup's choices come from the harness file, not
  from asking the agent.
- **Rebinding the popup's keys** through `[keys]`. The picker's keys are
  fixed today, and `e` joins them.

## Decisions taken with the user

- **Auto-approve is on by default for everyone.** The shipped harness files
  change, and installations that never edited them upgrade automatically. The
  popup has a permissions row, so anyone can turn prompts back on for one pane
  or save that as the harness's default.
- **The model list is shipped in each harness file, plus a "Custom…" row**
  for typing any model name. Instant and offline; it goes stale when a vendor
  ships a model, until Dispatch updates the file or the user edits it.
- **Approach A: each setting says how it becomes a flag.** A setting in the
  harness file carries its own `args` (and `env`) template. Chosen over
  placeholders in the launch line (a hidden "drop the flag before an empty
  placeholder" rule, and no clean fit for on/off flags) and over flag
  knowledge in Rust (breaks "harnesses are data, not code", and a harness the
  user adds could never have a popup).
- **The popup key is `e`** in the new-pane picker. `Enter` still opens a pane
  with the saved defaults.
- **`s` saves and keeps the popup open**, so a default can be changed without
  starting a pane. `Enter` in the popup opens a pane with what is shown,
  without saving.
- **Model and effort ship unset.** Each agent keeps its own default,
  including whatever its own config file says, until the user saves one. The
  only default that changes for everyone is auto-approve.

## The harness file: settings that become flags

`SettingDef` gains three fields, and its kinds keep their names:

| Field | Applies to | Meaning |
|---|---|---|
| `kind` | all | `choice`, `text` or `bool`, as today |
| `options` | `choice` | the values the popup steps through, as today |
| `default` | all | the value used when the user has chosen nothing, as today. With no `default`, nothing is passed and the agent uses its own default |
| `custom` | `choice` | **new.** `true` adds a "Custom…" value the user types. A `text` setting is custom by nature |
| `args` | all | **new.** Arguments to add. For `choice` and `text`, `{value}` is replaced by the value, and nothing is added when the value is unset. For `bool`, added as written when the value is `true`, and nothing when `false` |
| `env` | all | **new.** Variables to set, with `{value}` replaced, under the same rules as `args` |

A setting's `args` go **after** the launch's own arguments: after
`["/c", "claude"]` on Windows, and after `-p {task}` or `exec {task}` in a
one-shot run. On Windows a file-input form's redirect,
`<%DISPATCH_TASK_FILE%`, is read by `cmd.exe` wherever it stands, so a flag
after it still reaches the agent. A setting's `env` is added to the launch's
environment, and wins over the harness file's own `env` for the same name.

Settings are applied in the order the file lists them.

A setting with neither `args` nor `env` does nothing. It is not shown in the
popup, and a value saved for it is ignored. This is how a file edited before
this change behaves: its `[[settings]]` keep doing nothing, as they do today.

### What `default` and "agent default" mean

Each setting resolves to a value, or to **unset**:

1. the value chosen in the popup, when the pane was opened with `Enter` there
2. the user's saved default (see "Saved defaults" below)
3. the harness file's `default`
4. unset: nothing is passed, and the agent decides

A saved empty string means the user deliberately chose **agent default**, and
stops at step 2 as unset, even when the file has a `default`. That is how
someone turns Claude's permission mode back to Claude's own behaviour.

A `bool` is never unset once it has a `default`. One with no `default` and
nothing saved is `false`.

### The shipped files

`claude.toml`:

```toml
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = ["opus", "sonnet", "fable", "haiku"]
custom = true
args = ["--model", "{value}"]

[[settings]]
key = "effort"
label = "Effort"
kind = "choice"
options = ["low", "medium", "high", "xhigh", "max"]
args = ["--effort", "{value}"]

[[settings]]
key = "permissions"
label = "Permissions"
kind = "choice"
options = ["bypassPermissions", "auto", "acceptEdits", "plan", "manual"]
default = "bypassPermissions"
args = ["--permission-mode", "{value}"]
```

`codex.toml`:

```toml
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = ["gpt-6-astra", "gpt-6-sol", "gpt-6-luna", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna", "gpt-5.5"]
custom = true
args = ["-m", "{value}"]

[[settings]]
key = "effort"
label = "Effort"
kind = "choice"
options = ["low", "medium", "high", "xhigh", "max", "ultra"]
args = ["-c", "model_reasoning_effort={value}"]

# Codex's only way to skip every prompt also drops its sandbox.
[[settings]]
key = "bypass"
label = "Skip prompts"
kind = "bool"
default = true
args = ["--dangerously-bypass-approvals-and-sandbox"]
```

`-c` parses its value as TOML and falls back to the raw string, so
`model_reasoning_effort=high` needs no quotes. Quotes would sit on `cmd.exe`'s
command line on Windows.

`agy.toml`:

```toml
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = [
  "gemini-3.8-flash-high", "gemini-3.8-flash-medium", "gemini-3.8-flash-low",
  "gemini-3.7-flash-high", "gemini-3.7-flash-medium", "gemini-3.7-flash-low",
  "gemini-3.6-flash-high", "gemini-3.6-flash-medium", "gemini-3.6-flash-low",
  "gemini-3.1-pro-high", "gemini-3.1-pro-low",
  "claude-sonnet-4-6", "claude-opus-4-6-thinking", "gpt-oss-120b-medium",
]
custom = true
args = ["--model", "{value}"]

[[settings]]
key = "effort"
label = "Effort"
kind = "choice"
options = ["low", "medium", "high"]
args = ["--effort", "{value}"]

[[settings]]
key = "bypass"
label = "Skip prompts"
kind = "bool"
default = true
args = ["--dangerously-skip-permissions"]
```

`opencode.toml`:

```toml
# opencode's interactive mode has no model flag; its inline config variable
# carries one instead. It knows about 200 models, too many to ship, so the
# model is typed, as provider/model with an optional #variant.
[[settings]]
key = "model"
label = "Model"
kind = "text"
env = { OPENCODE_CONFIG_CONTENT = '{"model":"{value}"}' }

[[settings]]
key = "bypass"
label = "Auto-approve"
kind = "bool"
default = true
args = ["--auto"]
```

opencode's model row must be **verified by hand before it ships**. Its
interactive mode connects to a background service, and a variable set on the
terminal client may never reach that service. If it does not, the fallback is
to add `"--standalone"` to the setting's `args`, which gives the pane a
private server that does read the variable. If neither works, the row is
removed and opencode's popup shows only Auto-approve.

The model and effort lists above are what each CLI reported on 2026-09-28:
`claude --help`, `codex debug models`, and `agy models`.

### Upgrading installed files

The files shipped today become superseded bodies: `claude-6.toml` and
`codex-6.toml` join the existing lists, and `agy` and `opencode` get their
first, `agy-1.toml` and `opencode-1.toml`. An installation whose files are
byte-for-byte one of those is upgraded to the new file on the next start,
through the existing `write_missing_built_ins`. The user's own installation
is in that state today. A file someone edited is left alone.

## Saved defaults

A new file, `harness-settings.toml`, in the config directory
(`dispatch_os::paths::config_dir()`, beside `projects.toml`), holds one table
per harness id:

```toml
[claude]
model = "opus"
effort = "high"

[codex]
bypass = false        # this harness's prompts are back on
```

- `choice` and `text` values are TOML strings; `""` means agent default. `bool`
  values are TOML booleans.
- It is kept apart from `config.toml`, which is written by hand: rewriting
  that file would lose the user's comments. It is kept apart from the harness
  files, because editing one of those would stop upgrades from reaching it.
- It is written only through `store::update`, the locked, replace-not-rewrite
  update `projects.toml` and `machines.toml` use. Two Dispatch windows saving
  at once each keep their change, and a crash mid-write leaves the old file.
- A saved key the harness does not have, or has without `args` or `env`, is
  ignored. So is a saved value the harness would refuse (see "Safety"), with
  a log line naming the harness and key: the saved file is the user's, and a
  stale entry in it must not stop the harness opening.
- It is read at every spawn and every delegation, not cached. It is small, and
  reading it fresh means a save takes effect at once, in every client and in
  the daemon, with no reload message.

## How a choice reaches the pane

**The client sends what the user chose, the daemon checks and fills in the
rest.** `Enter` in the picker sends the user's saved defaults, the ones this
harness can take. `Enter` in the popup sends every value the popup shows. A
setting the message leaves out is the daemon's to decide.

Sending every setting on a plain `Enter` was the first design. It would have
refused every spawn on a machine whose harness file lacks one of this
machine's settings, even when the user had chosen nothing. Sending only what
the user chose means a machine with a different harness file still starts
the pane, unless the user asked it for something it cannot do.

The message:

```rust
ClientMessage::SpawnPane {
    project: ProjectId,
    harness: String,
    size: (u16, u16),
    #[serde(default)]
    place: Placement,
    /// Each setting's value the user chose or saved: a choice or text as
    /// itself ("" for agent default), a bool as "true" or "false". An older
    /// client sends none.
    #[serde(default)]
    settings: BTreeMap<String, String>,
}
```

The daemon validates every entry (see "Safety"), fills in any setting the
message leaves out from its own saved defaults and then the harness file, and
builds the launch. So:

- **A local pane** gets exactly what the picker detail or the popup showed:
  the daemon's saved file is the same file.
- **A pane on another machine** gets the user's saved defaults, or everything
  the popup showed: those travel with the request. A setting the user never
  chose is decided by that machine, from its own saved defaults and then its
  harness file. A setting that machine's harness file does not have is
  refused, naming it.
- **An older client** sends no settings, and the daemon applies its own
  saved defaults and the harness file's defaults. Auto-approve applies there
  too.
- **A delegated subagent** is started by the daemon with no client choice: it
  gets the daemon's saved defaults, then the file's. The user's `s`-saved
  Claude settings apply to `dispatch delegate` on the same machine, and with
  auto-approve on, a delegated `claude -p` can use tools it could not before.
  A subagent does not inherit a one-off choice from the pane that asked for
  it.

**Standalone**, with no daemon, the app resolves and builds the launch itself,
through the same functions.

### Where the code goes

- **`dispatch-config`**: the new `SettingDef` fields; validation of settings
  at load; a `harness_settings` module that reads and saves
  `harness-settings.toml` through `store`; and resolution plus launch
  building, as `HarnessDef` methods that take the chosen values and return
  the launch or a refusal. `launch_for` and `task_launch_for` stay as they
  are for callers that pass no settings. The one-shot path takes settings
  too, so `task_refusal_as` judges the run that will actually start.
- **`dispatch-proto`**: the `settings` field on `SpawnPane`.
- **`dispatch-daemon`**: `spawn_pane` and `task_run` build their launches with
  settings. The daemon is given the directory `harness-settings.toml` lives
  in, so tests can point it at a temporary one.
- **`dispatch-tui`**: the popup as its own widget, a form of rows, holding
  its own selection, stepping and custom-text state, so `app.rs` (already
  about 11,000 lines) gains only the glue.
- **`dispatch`**: the `e` key, the new overlay, sending `settings`, saving on
  `s`, and the picker's detail and hint.

## The popup

### Opening it

In the **New pane** picker, `Enter` opens the highlighted harness with its
resolved settings, as today. **`e`** opens the settings popup for the
highlighted harness instead. On the shell row, or on a harness with no
setting that has `args` or `env`, `e` says so on the status row:
`Shell has no settings`.

Each row's dimmed detail is the command, as today, followed by every setting
whose resolved value differs from the harness file's default, joined with
` · `. A harness still on its file's defaults shows only its command, and a
setting changed back shows nothing:

```
┌ New pane ─────────────────────────────────┐
│ Shell · fish                              │
│ Claude Code      claude · opus · high     │
│ Codex            codex · Skip prompts off │
│ agy              agy                      │
│ opencode         opencode                 │
└──── Enter open · e settings · Esc close ──┘
```

A `bool` that differs is written as its label and `on` or `off`. The hint on
the bottom border is only on the new-pane picker.

### Inside it

```
┌ New Claude Code pane ───────────────────────┐
│ Model        ◂ opus ▸                       │
│ Effort       ◂ agent default ▸              │
│ Permissions  ◂ bypassPermissions ▸          │
└─ Enter open · s save as default · Esc back ─┘
```

It opens showing each setting's value as the picker's `Enter` would resolve
it: the saved default, else the file's `default`, else **agent default**.

| Key | Does |
|---|---|
| `↑` `↓`, `k` `j` | move between rows |
| `←` `→`, `h` `l` | step through the row's values. A choice steps through **agent default**, its options in file order, then **Custom…** when `custom` is set, and wraps. A bool steps between `on` and `off` |
| `Space` | toggle a bool row |
| `Enter` | open a pane with the values shown; nothing is saved; the popup and picker close |
| `s` | save the values shown as this harness's defaults; the popup stays open and the status row says `saved as Claude Code's default` |
| `Esc` | back to the picker, with nothing saved |

A `text` row steps between **agent default** and its text.

**Custom values.** Stepping onto **Custom…** (or onto a `text` row's text)
starts editing it in place: `Custom: opus-5-5▏`. While editing:

- every printable key is text, so `j`, `s` and the rest type themselves; a
  character outside the safe set (see "Safety") is not accepted, and the
  status row says which characters are;
- `Backspace` deletes;
- `Enter` confirms the text and stops editing, without opening a pane;
- `↑` and `↓` confirm the text as `Enter` would, then move to the next row;
- `←` and `→` drop the text and step to the neighbouring value, so the arrows
  never trap the user on Custom;
- `Esc` stops editing and restores the value from before.

An empty confirmed value is **agent default**. A saved value that is not one
of the options opens as Custom with that text.

**What `s` writes.** Only the values that differ from the harness file's
`default` are written; a value equal to it removes that key from the saved
file. A default Dispatch later ships then reaches every setting the user never
changed. Choosing **agent default** where the file has a `default` (Claude's
permissions) differs, and is saved as `""`.

The popup opens a pane in the same place the picker would have: it keeps the
placement the picker was opened with.

## Safety

**The character rule.** On Windows every harness runs as
`cmd.exe /c claude …`, so a setting's value sits on `cmd.exe`'s command line,
where `&`, `|`, `%` and `^` are syntax: the same danger the one-shot form
already guards against. Every value that reaches a launch (a choice option,
a custom or text value, a saved value, a value in a `SpawnPane`) must:

- be 1 to 200 characters of letters, digits and `. _ : / @ # + -`, and
- not begin with `-`, so a "model" of `--dangerously-skip-permissions` cannot
  add a flag of its own.

Every real model name above passes, including
`amazon-bedrock/au.anthropic.claude-haiku-4-5-20251001-v1:0` and opencode's
`provider/model#variant`. With no quote or backslash allowed, a value cannot
break out of opencode's JSON either. The empty string is not a value: it is
"agent default", and passes nothing.

**Checked at every door:**

- **Load.** A harness file's options and `default` must pass the rule. An
  option that fails is dropped, with a log line naming the file and the
  option. A `default` that fails, or names no option of a non-custom choice,
  is dropped the same way.
- **The popup** does not accept a character outside the rule.
- **The daemon** checks every `SpawnPane` again and **refuses the spawn**,
  naming the harness and setting on the status row, when a setting:
  - is not one of the harness's (`claude has no setting "colour"`);
  - is a choice value that is not one of its options and the setting is not
    `custom`;
  - is a bool value other than `true` or `false`;
  - breaks the character rule.

  It refuses rather than drops, because a dropped `bypass = false` would start
  an agent with its prompts off, the opposite of what the user asked for. A
  client is not trusted to have checked: the daemon serves every client on its
  socket.
- **The saved file** is the user's own, but its values pass the same checks;
  one that fails is ignored with a log line, as above.

**Malformed settings.** A setting that parses but makes no sense is dropped
with a log line, and the harness still loads, as a bad `[status]` rule is
today:

- a `choice` with no options;
- `custom` on a `bool` or `text`;
- `{value}` in a `bool`'s `args` or `env`, which has nothing to fill it with;
- a second setting with a key already used.

A file that does not parse at all (`args = "x"`, say) is still an error for
the whole harness, as today.

**An unreadable saved file.** If `harness-settings.toml` does not parse, it
is treated as empty for launching, with a log line, and `s` refuses with
`couldn't save: harness-settings.toml line 4: …` rather than overwrite what
the user may want back.

## Documentation

- **`docs/security-model.md`** gains a section: agents now start without
  permission prompts by default, so an agent can act as the user without
  asking; Codex's flag also drops its sandbox. It says how to turn prompts back
  on (the popup's permissions row, then `s`) and repeats the existing advice
  to run untrusted agents under an OS sandbox.
- **README**: a "Harness settings" section: the `e` key and popup, the saved
  file, the setting fields with an example, and the auto-approve default with
  a pointer to the security model. The delegation section says that a
  subagent gets the saved defaults.

## Testing

**`dispatch-config`**
- Resolution order: popup value, then saved, then file default, then unset.
  A saved `""` beats a file default.
- `{value}` substitution in `args` and in `env`; a bool adds its `args` only
  when `true`; unset adds nothing.
- Settings' arguments come after the launch's own, on every platform form,
  and in one-shot runs, the Windows file form included.
- Setting `env` wins over the harness file's `env`.
- The character rule: every shipped option passes; each forbidden character,
  a leading `-`, an empty value and 201 characters are refused.
- Lenient loading: each malformed-setting case is dropped and the harness
  still loads; a setting with no `args` or `env` is ignored.
- Saved file: round trip; unknown keys and bad values ignored; an unparseable
  file reads as empty and refuses to save.
- Upgrade: each superseded body, `claude-1` to `claude-6`, `codex-1` to
  `codex-6`, `agy-1` and `opencode-1`, is replaced by the new file; an edited
  file is not.

**`dispatch-proto`**
- `SpawnPane` round trips with `settings`, and a message without the field
  (an older client) decodes to an empty map.

**`dispatch-daemon`**
- A spawn with settings starts the launch with the right arguments and
  environment.
- Each refusal case answers with an error naming the setting, and starts
  nothing.
- A spawn with no settings uses the saved file, then the file defaults.
- A delegation's one-shot run carries the saved defaults and the auto-approve
  flag.

**`dispatch` (app)**
- `e` opens the popup on the highlighted harness; on the shell it says there
  are no settings.
- The popup opens showing resolved values; `←` and `→` step and wrap; `Space`
  toggles a bool.
- Custom editing: letters type rather than navigate, a forbidden character is
  refused, `Enter` confirms without spawning, `↑`/`↓` confirm and move,
  `←`/`→` drop the text and step, `Esc` restores.
- `Enter` spawns with the shown values and saves nothing; `s` writes the file
  and keeps the popup open; `Esc` returns to the picker.
- `Enter` in the picker sends the saved values the harness can take, and
  nothing when nothing is saved.
- `s` writes only values that differ from the file's defaults, removes keys
  set back to them, and writes `""` for agent default over a file default.
- The picker's detail shows values that differ from the file's defaults, and
  only those.
- Standalone, a pane opened from the popup starts with the chosen flags.

**By hand, before merging**
- Each real CLI starts with its flags from a Dispatch pane: `claude`, `codex`,
  `agy` and `opencode`. Claude may ask once, the first time, to confirm
  bypass mode; that is Claude's own and is expected.
- opencode's model row, as described under "The shipped files".
- A delegated `claude -p` edits a file without asking.
