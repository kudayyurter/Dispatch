# Option labels, and one setting limited by another

Status: designed, not yet planned.
Date: 2026-09-28.
Branch: `feat/option-labels-and-limits`, cut from `main` at 2ea6a1f.
Builds on: `docs/superpowers/specs/2026-09-28-harness-settings-design.md`.

## The problem

PR #6 removed agy's Effort row. Passing a full model name such as
`gemini-3.8-flash-high` with a different `--effort` made agy quietly fall back
to another model. The user wants the row back, because effort is something
agy lets you change, and wants the popup to show clean names rather than
`gemini-3.8-flash-high`.

A check of agy on 2026-09-28 showed its native form is a **base** model plus
`--effort`, and that each model allows its own efforts:

| Model | `--effort` agy accepts |
|---|---|
| `gemini-3.8-flash`, `gemini-3.7-flash`, `gemini-3.6-flash` | `low`, `medium`, `high`; one is required |
| `gemini-3.1-pro` | `low`, `high`; one is required |
| `gpt-oss-120b` | `medium` (the model alone also works) |
| `claude-sonnet-4-6`, `claude-opus-4-6-thinking` | none; `--effort` is an error |

agy's own errors say so, for example "gemini-3.1-pro has no "medium" effort
(available: low, high)" and "--effort is not supported for model
"claude-sonnet-4-6"".

### Not in this piece of work

- Fetching model or effort lists from the agents at run time.
- Limits between settings other than one setting limited by one other.
- Labels for a setting's own name: settings already have `label`.

## Decisions taken with the user

- **The Effort row comes back for agy**, offering only what the chosen model
  allows, and faded out for a model that allows none.
- **Clean names everywhere.** Every harness's options get labels: Claude,
  Codex and agy.
- **When the current effort is not offered** by a newly chosen model, the
  row takes the **highest** effort that model offers.
- **Harnesses stay data.** The rules live in the harness file, not in Rust,
  so a harness the user adds can use them too.

## The harness file

### An option can be a table

An entry in a choice's `options` is either a plain string, as today, or a
table:

| Field | Meaning |
|---|---|
| `value` | what is passed to the agent and saved; the same character rule as any value |
| `label` | what the popup and the picker show; optional, defaulting to `value` |
| any other key | a list of values allowed for the setting of that key, when that setting is `limited_by` this one; see below |

Both forms can be mixed in one list. A file written before this change parses
exactly as before.

### `limited_by`

A choice setting may say `limited_by = "<key of another choice setting>"`.
Then the other setting's chosen option decides which of this setting's values
are allowed:

- **The chosen option lists values for this setting's key** (agy's Gemini
  models list `effort`). This setting is **required**: it may only be one of
  those values, and "agent default" is not offered. If the current value is
  not among them, it becomes the **highest** of them, meaning the one that
  comes last in this setting's own `options` order.
- **The chosen option lists nothing for this key** (agy's Claude models).
  This setting is **unavailable**: its value is unset, it passes nothing, and
  the popup fades it.
- **The limiting setting holds a value that is not one of its options** (a
  typed Custom… model), or it is unset. This setting is **free**: any of its
  options, or agent default, as today.

### Checked at load

These are dropped with a log line, the harness still loading, as other
malformed settings are:

- a `limited_by` naming a key that is not a choice setting in the same file,
  names the setting itself, or forms a chain (a setting that is `limited_by`
  one that is itself `limited_by` another);
- in an option's list, a value that is not one of the limited setting's
  options;
- a table option with no `value`, or one whose value fails the character
  rule. Only that option is dropped.

## Resolving and launching

`HarnessDef::resolve` resolves every value as today: chosen, then saved, then
the file's default. It then **fits** each limited setting to its limiter's
resolved value, by the rule above. So:

- a chosen or saved effort that the chosen model does not allow becomes the
  model's highest, rather than being refused;
- a model that allows none sets the effort to unset;
- a chosen effort that is not one of the Effort setting's own options is
  still refused, as today.

The client resolves the same way before it sends, and the daemon fits again
when it builds the launch, so a stale saved pair can never reach agy.

## What the user sees

**The popup:**
- A choice row shows the chosen option's label: `Model ◂ Gemini 3.8 Flash ▸`.
- A typed value shows as `Custom: <text>`, as today.
- A limited row steps only through its allowed values. It offers "agent
  default" only while it is free.
- When it is unavailable, it is drawn faded as `not available`, and `↑` and
  `↓` skip it.
- Changing the limiting row re-fits the limited one at once.

**The picker's detail** uses labels, for example `agy · Gemini 3.1 Pro · High`
and `claude · Opus · High`. A flag stays `<label> on` or `<label> off`.

## The shipped files

`agy.toml` gets back an Effort row, as in the table below. Its Model row
becomes base models with labels and effort lists:

| value | label | effort |
|---|---|---|
| `gemini-3.8-flash` | Gemini 3.8 Flash | low, medium, high |
| `gemini-3.7-flash` | Gemini 3.7 Flash | low, medium, high |
| `gemini-3.6-flash` | Gemini 3.6 Flash | low, medium, high |
| `gemini-3.1-pro` | Gemini 3.1 Pro | low, high |
| `gpt-oss-120b` | GPT-OSS 120B | medium |
| `claude-sonnet-4-6` | Claude Sonnet 4.6 | — |
| `claude-opus-4-6-thinking` | Claude Opus 4.6 (Thinking) | — |

agy's Effort row is `limited_by = "model"`, with options `low` / `medium` /
`high`, labelled Low / Medium / High, and args `["--effort", "{value}"]`.

`claude.toml` and `codex.toml` keep their values and gain labels:

| Harness | Setting | value → label |
|---|---|---|
| Claude | Model | `opus` Opus · `sonnet` Sonnet · `fable` Fable · `haiku` Haiku |
| Claude | Effort | `low` Low · `medium` Medium · `high` High · `xhigh` Extra high · `max` Max |
| Claude | Permissions | `bypassPermissions` Bypass permissions · `auto` Auto · `acceptEdits` Accept edits · `plan` Plan · `manual` Manual |
| Codex | Model | `gpt-6-astra` GPT-6 Astra · `gpt-6-sol` GPT-6 Sol · `gpt-6-luna` GPT-6 Luna · `gpt-5.6-sol` GPT-5.6 Sol · `gpt-5.6-terra` GPT-5.6 Terra · `gpt-5.6-luna` GPT-5.6 Luna · `gpt-5.5` GPT-5.5 |
| Codex | Effort | `low` Low · `medium` Medium · `high` High · `xhigh` Extra high · `max` Max · `ultra` Ultra |

`opencode.toml` is unchanged: its model is typed, and its flag already has a
label.

**Upgrades:** today's `claude.toml`, `codex.toml` and `agy.toml` become
superseded bodies (`claude-7`, `codex-7`, `agy-3`), so unedited files are
upgraded.

A value saved before this change keeps working:
- A saved agy model such as `gemini-3.8-flash-high` is no longer an option,
  so it is a typed custom model. agy accepts that alone, and the Effort row
  is then free.
- A saved agy `effort` is fitted to the model.

## Testing

**`dispatch-config`**
- Options parse in both forms, mixed in one list, and a label defaults to
  the value.
- Load-time checks: each malformed case is dropped with the harness still
  loading.
- Fitting: an allowed value is kept; a value not allowed becomes the highest
  allowed; a model allowing none makes it unset; a custom or unset limiter
  leaves it free; a chosen effort that is no option at all is still refused.
- The shipped agy file launches:
  - `--model gemini-3.1-pro --effort high` for model `gemini-3.1-pro` with
    effort `medium` (fitted up);
  - `--model claude-sonnet-4-6` and no effort for a Claude model;
  - `--model gpt-oss-120b --effort medium`.
- `describe_changes` uses labels.
- Every earlier body is upgraded.

**`dispatch-tui` (popup)**
- It shows labels.
- The Effort row steps only allowed values, and offers no agent default while
  limited.
- Changing the model re-fits the effort to the highest.
- For a Claude model the row is faded, reads `not available`, and `↑` and `↓`
  skip it.
- A custom model frees the row.

**`dispatch` (app)**
- The picker detail shows labels.
- `Enter` in the popup sends the fitted pair.

**By hand**
- Open agy through `e`: Gemini 3.1 Pro starts on High, and a Claude model
  starts with no effort.
