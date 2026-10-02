//! Harness definitions: how to launch each coding agent.
//!
//! Harnesses are data, not code. A user can register a new one by dropping a
//! TOML file into the harnesses directory, or through the harness manager in
//! the TUI, without Dispatch being rebuilt.

use std::collections::{BTreeMap, BTreeSet};

use dispatch_core::HarnessId;
use serde::{Deserialize, Serialize};

/// What a setting accepts, so the harness manager can render a form for a
/// harness Dispatch has never seen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SettingKind {
    /// Free text.
    Text {
        /// Value used when the user does not supply one.
        #[serde(default)]
        default: Option<String>,
    },
    /// One of a fixed set of values, such as a model or effort level.
    Choice {
        /// The values on offer.
        options: Vec<ChoiceOption>,
        /// Value used when the user does not supply one.
        #[serde(default)]
        default: Option<String>,
    },
    /// A flag.
    Bool {
        /// Value used when the user does not supply one.
        #[serde(default)]
        default: Option<bool>,
    },
}

/// One value a choice offers.
///
/// A harness file writes it as the value alone, or as a table naming how it
/// is shown too: `{ value = "gemini-3.1-pro", label = "Gemini 3.1 Pro",
/// effort = ["low", "high"] }`. Any other key in the table names a setting
/// `limited_by` this one, and lists the values that setting may take while
/// this option is chosen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "OptionForm", into = "OptionForm")]
pub struct ChoiceOption {
    /// What is passed to the agent and saved.
    pub value: String,
    /// What the popup and the picker show; the value when absent.
    pub label: Option<String>,
    /// For each setting limited by this one, by key: the values it may take
    /// while this option is chosen. A limited setting this names nothing
    /// for takes none.
    pub allows: BTreeMap<String, Vec<String>>,
}

impl ChoiceOption {
    /// An option with no label, limiting nothing.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: None,
            allows: BTreeMap::new(),
        }
    }

    /// What is shown for this option.
    #[must_use]
    pub fn label(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.value)
    }
}

/// How an option is written in a harness file.
///
/// Read leniently: a mistake in one option costs that option, or that part
/// of it, and is logged, rather than stopping every harness loading.
#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum OptionForm {
    /// The value alone.
    Plain(String),
    /// A table.
    Table(OptionTable),
    /// Anything else, which offers nothing: its value is empty, so it is
    /// dropped when the harness is checked.
    Other(toml::Value),
}

/// An option written as a table, each field as it was written.
#[derive(Clone, Serialize, Deserialize)]
struct OptionTable {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<toml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label: Option<toml::Value>,
    #[serde(flatten)]
    allows: BTreeMap<String, toml::Value>,
}

impl From<OptionForm> for ChoiceOption {
    fn from(form: OptionForm) -> Self {
        let table = match form {
            OptionForm::Plain(value) => return ChoiceOption::new(value),
            OptionForm::Other(written) => {
                tracing::warn!(%written, "dropping an option that is neither a value nor a table");
                return ChoiceOption::new("");
            }
            OptionForm::Table(table) => table,
        };

        // Missing, or not a string, is empty, which no value may be, so the
        // option is dropped when the harness is checked.
        let value = match table.value {
            Some(toml::Value::String(value)) => value,
            _ => String::new(),
        };
        let label = match table.label {
            None => None,
            Some(toml::Value::String(label)) => Some(label),
            Some(written) => {
                tracing::warn!(option = %value, %written, "dropping a label that is not text");
                None
            }
        };
        let allows = table
            .allows
            .into_iter()
            .filter_map(|(key, written)| {
                let listed = match &written {
                    // One value, written without its brackets.
                    toml::Value::String(one) => Some(vec![one.clone()]),
                    toml::Value::Array(items) => items
                        .iter()
                        .map(|item| item.as_str().map(str::to_string))
                        .collect(),
                    _ => None,
                };
                if listed.is_none() {
                    tracing::warn!(
                        option = %value,
                        key = %key,
                        %written,
                        "dropping what is not a list of values: a misspelt key, or a list \
                         with something other than text in it"
                    );
                }
                Some((key, listed?))
            })
            .collect();

        ChoiceOption {
            value,
            label,
            allows,
        }
    }
}

impl From<ChoiceOption> for OptionForm {
    fn from(option: ChoiceOption) -> Self {
        if option.label.is_none() && option.allows.is_empty() {
            OptionForm::Plain(option.value)
        } else {
            OptionForm::Table(OptionTable {
                value: Some(toml::Value::String(option.value)),
                label: option.label.map(toml::Value::String),
                allows: option
                    .allows
                    .into_iter()
                    .map(|(key, listed)| {
                        let items = listed.into_iter().map(toml::Value::String).collect();
                        (key, toml::Value::Array(items))
                    })
                    .collect(),
            })
        }
    }
}

/// One configurable setting a harness exposes.
///
/// It says how it becomes a flag: its own `args` and `env`, with `{value}`
/// standing for the value. A harness Dispatch has never seen gets a settings
/// popup from its file alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingDef {
    /// Key the setting is saved and sent under.
    pub key: String,
    /// Human-readable label shown in the settings popup.
    pub label: String,
    /// What the setting accepts.
    #[serde(flatten)]
    pub kind: SettingKind,
    /// For a choice, whether the popup also offers a value the user types.
    #[serde(default)]
    pub custom: bool,
    /// For a choice, the key of another choice whose chosen option decides
    /// which of this one's values may be used: the values that option lists
    /// under this setting's key, or none when it lists nothing. A value of
    /// the other's that is not one of its options limits nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limited_by: Option<String>,
    /// Arguments added after the launch's own. A choice or text puts its
    /// value where `{value}` is and adds nothing when unset; a flag adds
    /// them as written when it is on.
    #[serde(default)]
    pub args: Vec<String>,
    /// Variables set for the child, filled in as `args` is.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// How to launch a harness on one platform.
///
/// Windows needs its own entry more often than not: `claude` there is
/// typically a `claude.cmd` shim, which cannot be executed directly and has to
/// be run through `cmd.exe /c`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Launch {
    /// Executable to run. Resolved on `PATH` when not absolute.
    pub command: String,
    /// Arguments, which may contain `{placeholder}` templates.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables to set for the child, on top of what it
    /// inherits.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Variables the child must not have at all: not set by `env`, and not
    /// inherited either.
    ///
    /// Never read from a harness file. Whoever starts the launch decides it,
    /// for a variable whose inherited value could only be stale.
    #[serde(skip)]
    pub unset: BTreeSet<String>,
}

/// How a one-shot run is given its task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskInput {
    /// `{task}` in the arguments is replaced by the task, as one argument.
    ///
    /// Safe wherever the program is started directly: an argument is not
    /// parsed by anything on the way.
    #[default]
    Argument,
    /// The task is written to a file named by [`TASK_FILE_ENV`], and the
    /// arguments redirect that file into the program's standard input.
    ///
    /// For a program reached through `cmd.exe`, which reads its whole command
    /// line as shell syntax: a task placed there could run its `&`, `|` and
    /// `%VAR%` as commands, and cannot carry a newline at all. A file on
    /// standard input is parsed by nothing.
    File,
}

/// The variable naming a task's file, for a form whose input is
/// [`TaskInput::File`].
///
/// On Windows its value is the path in double quotes, ready for `cmd.exe`'s
/// `<`: the variable is expanded where it stands, so an unquoted path with a
/// space in it -- `C:\Users\Ada Lovelace\…` -- would redirect from the part
/// before the space. Elsewhere it is the bare path, for a shell to quote as
/// `"$DISPATCH_TASK_FILE"`.
pub const TASK_FILE_ENV: &str = "DISPATCH_TASK_FILE";

/// A one-shot run, ready to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRun {
    /// What to start.
    pub launch: Launch,
    /// How the task reaches it.
    pub input: TaskInput,
}

/// One platform's form for a one-shot run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskArgs {
    /// Arguments, with `{task}` standing for the task.
    #[serde(default)]
    pub args: Vec<String>,
    /// How the task reaches the program: `{task}` in the arguments, or a
    /// file on its standard input.
    #[serde(default)]
    pub input: TaskInput,
}

/// How to run a harness once, on one task, without a person at the keyboard.
///
/// Delegation needs a form that finishes: an interactive agent waits for input
/// forever, so a caller blocking on one would never be answered. A harness
/// without this cannot be delegated to, and Dispatch says so rather than
/// guessing at flags that may mean something else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskLaunch {
    /// Arguments for the one-shot form, with `{task}` standing for the task.
    #[serde(default)]
    pub args: Vec<String>,
    /// How the task reaches the program: `{task}` in the arguments, or a
    /// file on its standard input.
    #[serde(default)]
    pub input: TaskInput,
    /// Per-platform overrides, keyed by `std::env::consts::OS` exactly as
    /// `HarnessDef::platform` is. A platform whose interactive launch needs a
    /// wrapper needs it here too: the wrapper is how the executable is reached,
    /// and a one-shot run reaches it the same way.
    #[serde(default)]
    pub platform: BTreeMap<String, TaskArgs>,
    /// The interactive form, `[task.interactive]`, for `dispatch delegate
    /// --interactive`.
    #[serde(default)]
    pub interactive: Option<InteractiveLaunch>,
}

/// How to start a harness's own interface on a delegated task.
///
/// The handoff becomes the agent's first prompt, and the agent stays open:
/// the user can watch it and type into it. It never exits by itself, so the
/// daemon knows it is finished when it runs `dispatch report`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct InteractiveLaunch {
    /// Arguments, with `{task}` standing for the brief.
    #[serde(default)]
    pub args: Vec<String>,
    /// Per-platform overrides, keyed by `std::env::consts::OS`.
    #[serde(default)]
    pub platform: BTreeMap<String, InteractiveArgs>,
}

/// One platform's interactive form.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct InteractiveArgs {
    /// Arguments, with `{task}` standing for the brief.
    #[serde(default)]
    pub args: Vec<String>,
}

/// The mark drawn beside a harness that names none of its own.
///
/// A terminal, because that is what every harness is until it says otherwise.
/// Nerd Font, like the rest of Dispatch's glyphs.
pub const DEFAULT_ICON: &str = "\u{f120}";

/// The mark a harness Dispatch ships is drawn with, by id.
fn built_in_icon(id: &str) -> Option<&'static str> {
    match id {
        "claude" => Some("\u{ec82}"),
        "codex" => Some("\u{ec81}"),
        "agy" => Some("\u{e7f0}"),
        "opencode" => Some("\u{f121}"),
        _ => None,
    }
}

/// A registered coding agent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessDef {
    /// Stable identifier, matching the file stem by convention.
    pub id: String,
    /// Name shown in pickers.
    pub display_name: String,

    /// A single character drawn beside this harness's panes in the sidebar.
    ///
    /// Optional: a harness that names none is drawn with [`DEFAULT_ICON`]. One
    /// column is reserved for it, so a glyph wider than a cell pushes the title
    /// of that row alone out of line.
    #[serde(default)]
    pub icon: Option<String>,

    /// Default launch configuration, used when no platform override applies.
    #[serde(flatten)]
    pub launch: Launch,

    /// Launch overrides per platform, keyed by `std::env::consts::OS`
    /// (`"windows"`, `"macos"`, `"linux"`).
    #[serde(default)]
    pub platform: BTreeMap<String, Launch>,

    /// Environment variables to set for the child process.
    #[serde(default)]
    pub env: BTreeMap<String, String>,

    /// The non-interactive form used when another agent delegates to this one.
    #[serde(default)]
    pub task: Option<TaskLaunch>,

    /// Settings the settings popup offers for this harness.
    #[serde(default)]
    pub settings: Vec<SettingDef>,

    /// Rules that read this agent's state off its screen. Absent means the
    /// built-in rules for its id, if Dispatch has some.
    #[serde(default)]
    pub status: Option<crate::status::StatusDef>,
}

impl HarnessDef {
    /// The mark drawn beside this harness's panes.
    ///
    /// A file with no `icon` key falls back on its id before the generic
    /// glyph: an installation made before icons existed keeps its files
    /// without one wherever they are not upgraded -- `agy` and `opencode`,
    /// and any the user edited.
    #[must_use]
    pub fn icon(&self) -> &str {
        self.icon
            .as_deref()
            .or_else(|| built_in_icon(&self.id))
            .unwrap_or(DEFAULT_ICON)
    }

    /// The identifier as a [`HarnessId`].
    #[must_use]
    pub fn harness_id(&self) -> HarnessId {
        HarnessId::new(&self.id)
    }

    /// The launch configuration for the platform Dispatch is running on.
    #[must_use]
    pub fn launch_for_current_platform(&self) -> Launch {
        self.launch_for(std::env::consts::OS)
    }

    /// The launch configuration for a named platform.
    ///
    /// Falls back to the default when the platform has no override, so a
    /// harness that behaves the same everywhere needs only one entry.
    ///
    /// The harness-level environment is merged in, with any platform-specific
    /// entry winning, so `env` can be declared once and still be overridden
    /// where a platform needs something different.
    #[must_use]
    pub fn launch_for(&self, os: &str) -> Launch {
        let base = self.platform.get(os).unwrap_or(&self.launch);

        let mut launch = base.clone();
        for (key, value) in &self.env {
            launch
                .env
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        launch
    }

    /// The run for doing `task` once on the current platform.
    #[must_use]
    pub fn task_launch(&self, task: &str) -> Option<TaskRun> {
        self.task_launch_for(std::env::consts::OS, task)
    }

    /// The run for doing `task` once on a named platform.
    ///
    /// Returns `None` when the harness has no one-shot form for that platform,
    /// including when its argument list is empty: without arguments there is no
    /// way to tell the agent what the task is, so there is nothing to run.
    #[must_use]
    pub fn task_launch_for(&self, os: &str, task: &str) -> Option<TaskRun> {
        let (args, input) = self.task_form_for(os)?;
        if args.is_empty() {
            return None;
        }

        let mut launch = self.launch_for(os);
        launch.args = match input {
            TaskInput::Argument => args.iter().map(|arg| arg.replace("{task}", task)).collect(),
            // Nothing is substituted: the task goes to a file, and the
            // arguments only ever name the file.
            TaskInput::File => args.to_vec(),
        };

        Some(TaskRun { launch, input })
    }

    /// Why this harness's one-shot form must not run on `os`, if it must not,
    /// judged as [`HarnessDef::task_refusal_as`] judges it, on the harness's
    /// own launch with this process's environment beneath it.
    #[must_use]
    pub fn task_refusal_for(&self, os: &str) -> Option<String> {
        self.task_refusal_as(os, &self.launch_for(os))
    }

    /// Why this harness's one-shot form must not start as `launch` on `os`,
    /// if it must not.
    ///
    /// A form that puts the task on `cmd.exe`'s command line lets the task's
    /// `&`, `|` and `%VAR%` run as commands. The harness file is the user's
    /// and may predate Dispatch knowing that, so such a form is refused with
    /// a way out rather than run. What decides is the file Windows would
    /// start: `cmd.exe` named outright, or a command that `launch`'s `PATH`
    /// and `PATHEXT` turn into a batch file -- `claude` found as
    /// `claude.cmd` -- which Windows runs through `cmd.exe` all the same. So
    /// `launch` is the run as it will start, environment and all; variables
    /// it does not set are this process's, as the child's will be.
    ///
    /// A form that reads its task from a file and names `{task}` too is
    /// refused everywhere: nothing fills that in, so the agent would be
    /// handed the placeholder itself.
    #[must_use]
    pub fn task_refusal_as(&self, os: &str, launch: &Launch) -> Option<String> {
        let (args, input) = self.task_form_for(os)?;
        let names_the_task = args.iter().any(|arg| arg.contains("{task}"));

        if input == TaskInput::File && names_the_task {
            return Some(format!(
                "harness {id:?} reads its task from a file (input = \"file\") but its args \
                 also name \"{{task}}\", which a file form never fills in. A file form reads \
                 the task from %{TASK_FILE_ENV}% on Windows or \"${TASK_FILE_ENV}\" elsewhere \
                 and must not name {{task}}: remove it from the args in {id}.toml and restart \
                 the daemon",
                id = self.id
            ));
        }

        if os != "windows" || input != TaskInput::Argument || !names_the_task {
            return None;
        }
        let through = if runs_through_cmd(&launch.command) {
            String::new()
        } else {
            let found = found_as(launch);
            if !runs_through_cmd(&found.to_string_lossy()) {
                return None;
            }
            format!(
                " ({} is {}, a batch file, which Windows runs through cmd.exe)",
                launch.command,
                found.display()
            )
        };

        Some(format!(
            "harness {id:?} would put the task on cmd.exe's command line{through}, where \
             characters like & and % run as commands. In {id}.toml, under \
             [task.platform.windows] (add that table if there is none), set input = \"file\" \
             and put \"<%{TASK_FILE_ENV}%\" where \"{{task}}\" was: args = [..., \
             \"<%{TASK_FILE_ENV}%\"]. For a harness Dispatch ships, deleting {id}.toml \
             brings back the current one instead. Then restart the daemon",
            id = self.id
        ))
    }

    /// The launch for running this harness's own interface on `task`, on a
    /// named platform.
    ///
    /// `None` when it has no interactive form there, or one that never names
    /// `{task}`: the brief has no other way in, since the agent's standard
    /// input is its terminal.
    #[must_use]
    pub fn interactive_launch_for(&self, os: &str, task: &str) -> Option<Launch> {
        let form = self.task.as_ref()?.interactive.as_ref()?;
        let args = form.platform.get(os).map_or(&form.args, |over| &over.args);
        if !args.iter().any(|arg| arg.contains("{task}")) {
            return None;
        }

        let mut launch = self.launch_for(os);
        launch.args = args.iter().map(|arg| arg.replace("{task}", task)).collect();
        Some(launch)
    }

    /// Why this harness's interactive form must not start as `launch` on
    /// `os`, if it must not.
    ///
    /// The one-shot form escapes `cmd.exe` by reading its task from a file
    /// on standard input. An interactive agent cannot: its standard input is
    /// the terminal. So on Windows any interactive form reached through
    /// `cmd.exe` is refused outright, rather than letting a handoff's `&`
    /// and `%VAR%` run as commands.
    #[must_use]
    pub fn interactive_refusal_as(&self, os: &str, launch: &Launch) -> Option<String> {
        if os != "windows" {
            return None;
        }
        if !runs_through_cmd(&launch.command)
            && !runs_through_cmd(&found_as(launch).to_string_lossy())
        {
            return None;
        }

        Some(format!(
            "harness {id:?} would put the handoff on cmd.exe's command line, where characters \
             like & and % run as commands, and an interactive agent cannot read it from a file \
             instead. Interactive delegation is not available for {id} on Windows; delegate \
             without --interactive",
            id = self.id
        ))
    }

    /// The one-shot form for `os`: its arguments, and how the task reaches
    /// them. A platform's own form wins whole, input included, so a Windows
    /// form reading a file never inherits the default's `{task}`.
    fn task_form_for(&self, os: &str) -> Option<(&[String], TaskInput)> {
        let form = self.task.as_ref()?;
        Some(match form.platform.get(os) {
            Some(override_) => (&override_.args, override_.input),
            None => (&form.args, form.input),
        })
    }
}

/// The file Windows would start for `launch`: its command, looked for on its
/// `PATH` and completed with its `PATHEXT` exactly as the spawn does.
///
/// Both read through the environment the spawn would build --
/// [`dispatch_os::pty::WindowsEnvironment`], this process's variables with
/// `launch`'s on top and its removals taken out, names folded as Windows
/// folds them -- so of `PATH` and `Path` this sees the one the child gets.
fn found_as(launch: &Launch) -> std::path::PathBuf {
    let environment =
        dispatch_os::pty::WindowsEnvironment::new(std::env::vars_os(), &launch.env, &launch.unset);
    dispatch_os::pty::resolve_program(
        &launch.command,
        environment.get("PATH"),
        environment.get("PATHEXT"),
    )
}

/// Whether `command` runs through `cmd.exe`: the program itself, or a batch
/// file, which Windows runs with it.
///
/// Judged on the name Windows opens, which has lost any trailing dots and
/// spaces: `agent.cmd.` is `agent.cmd`.
fn runs_through_cmd(command: &str) -> bool {
    let name = std::path::Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command)
        .trim_end_matches(['.', ' '])
        .to_ascii_lowercase();

    name == "cmd" || name == "cmd.exe" || name.ends_with(".cmd") || name.ends_with(".bat")
}
