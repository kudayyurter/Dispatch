//! A harness's settings: what a value may be, and which of a harness file's
//! settings can be used.
//!
//! A setting says how it becomes a flag -- its own `args` and `env`, with
//! `{value}` standing for the value -- so a harness Dispatch has never seen
//! gets a settings popup from its file alone.

use std::collections::BTreeMap;

use crate::harness::{ChoiceOption, HarnessDef, Launch, SettingDef, SettingKind, TaskRun};

/// Each setting's value, by key, as the popup, the saved file and the
/// protocol carry it: a choice or text as itself, `""` for agent default,
/// and a flag as `"true"` or `"false"`.
pub type Choices = BTreeMap<String, String>;

/// What a value may be made of, for telling the user.
pub const SAFE_CHARACTERS: &str = "letters, digits and . _ : / @ # + -, not starting with -";

/// The longest value a setting takes, in characters.
pub const MAX_VALUE_CHARS: usize = 200;

/// Where a setting's `args` and `env` put its value.
pub(crate) const VALUE: &str = "{value}";

/// Whether `c` may appear in a value.
///
/// On Windows every harness runs through `cmd.exe`, which reads its whole
/// command line as shell syntax: a value holding `&`, `|`, `%`, `^` or a
/// quote could run commands. None of these means anything to it, and none
/// can break out of a JSON string either.
#[must_use]
pub fn is_safe_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '/' | '@' | '#' | '+' | '-')
}

/// Whether `value` may reach a command line.
///
/// Not empty -- empty is agent default, and passes nothing -- not longer
/// than [`MAX_VALUE_CHARS`], made of [`is_safe_char`] only, and not starting
/// with `-`: a "model" of `--dangerously-skip-permissions` would otherwise
/// add a flag of its own.
#[must_use]
pub fn is_safe_value(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= MAX_VALUE_CHARS
        && !value.starts_with('-')
        && value.chars().all(is_safe_char)
}

/// Which values a setting may take, given the other settings' values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Allowed {
    /// Any of its options, a typed value where it takes one, or agent
    /// default.
    Free,
    /// Only these, in the setting's own option order, and never agent
    /// default. Never empty.
    Only(Vec<String>),
    /// None: it is unset, and passes nothing.
    Unavailable,
}

/// Which values the setting `key` of `settings` may take while the settings
/// hold `values`.
///
/// Only a setting `limited_by` another is ever limited, and only while that
/// other holds one of its options: a typed value, or agent default, limits
/// nothing.
#[must_use]
pub fn allowed(settings: &[SettingDef], key: &str, values: &Choices) -> Allowed {
    let find = |key: &str| settings.iter().find(|setting| setting.key == key);
    let Some(setting) = find(key) else {
        return Allowed::Free;
    };
    let Some(limiter) = setting.limited_by.as_deref().and_then(find) else {
        return Allowed::Free;
    };
    let chosen = values.get(&limiter.key).map_or("", String::as_str);
    let Some(option) = limiter.option(chosen) else {
        return Allowed::Free;
    };

    let listed = option.allows.get(key).map_or(&[][..], Vec::as_slice);
    let only: Vec<String> = setting
        .options()
        .iter()
        .filter(|offered| listed.contains(&offered.value))
        .map(|offered| offered.value.clone())
        .collect();
    if only.is_empty() {
        Allowed::Unavailable
    } else {
        Allowed::Only(only)
    }
}

/// Brings each limited setting in `values` within what [`allowed`] lets it
/// take: an unavailable one is unset, and one holding a value its list does
/// not have takes the highest the list does, the last in its own option
/// order.
///
/// A limit is followed, never refused: whatever the saved file or an older
/// client holds, the agent is only ever handed a pair it accepts.
pub fn fit_limits(settings: &[SettingDef], values: &mut Choices) {
    for setting in settings
        .iter()
        .filter(|setting| setting.limited_by.is_some())
    {
        let current = values.get(&setting.key).map_or("", String::as_str);
        let fitted = match allowed(settings, &setting.key, values) {
            Allowed::Free => continue,
            Allowed::Unavailable => String::new(),
            Allowed::Only(only) if only.iter().any(|value| value == current) => continue,
            Allowed::Only(only) => only.last().cloned().unwrap_or_default(),
        };
        values.insert(setting.key.clone(), fitted);
    }
}

impl SettingDef {
    /// Whether this is on or off, rather than a value.
    #[must_use]
    pub fn is_flag(&self) -> bool {
        matches!(self.kind, SettingKind::Bool { .. })
    }

    /// A choice's options, in file order; none for text or a flag.
    #[must_use]
    pub fn options(&self) -> &[ChoiceOption] {
        match &self.kind {
            SettingKind::Choice { options, .. } => options,
            SettingKind::Text { .. } | SettingKind::Bool { .. } => &[],
        }
    }

    /// The option whose value is `value`, if there is one.
    #[must_use]
    pub fn option(&self, value: &str) -> Option<&ChoiceOption> {
        self.options().iter().find(|option| option.value == value)
    }

    /// `value` as it is shown: an option by its label, anything else --
    /// typed, or text -- as itself.
    #[must_use]
    pub fn label_of<'a>(&'a self, value: &'a str) -> &'a str {
        self.option(value).map_or(value, ChoiceOption::label)
    }

    /// The value this setting has when nothing was chosen or saved: the
    /// file's `default`, or agent default. A flag with no default is off.
    #[must_use]
    pub fn file_default(&self) -> String {
        match &self.kind {
            SettingKind::Bool { default } => default.unwrap_or(false).to_string(),
            SettingKind::Choice { default, .. } | SettingKind::Text { default } => {
                default.clone().unwrap_or_default()
            }
        }
    }

    /// Why `value` cannot be this setting's, if it cannot.
    ///
    /// `""` is agent default for a choice or text, and never a flag's: a flag
    /// is on or off.
    pub fn check(&self, value: &str) -> Result<(), String> {
        let key = &self.key;
        let unsafe_value = || format!("{key} takes only {SAFE_CHARACTERS}; {value:?} is refused");

        match &self.kind {
            SettingKind::Bool { .. } => match value {
                "true" | "false" => Ok(()),
                _ => Err(format!(
                    "{key} is on or off, \"true\" or \"false\", not {value:?}"
                )),
            },
            SettingKind::Choice { options, .. } => {
                if value.is_empty() || options.iter().any(|option| option.value == value) {
                    Ok(())
                } else if !self.custom {
                    Err(format!("{value:?} is not one of {key}'s options"))
                } else if is_safe_value(value) {
                    Ok(())
                } else {
                    Err(unsafe_value())
                }
            }
            SettingKind::Text { .. } => {
                if value.is_empty() || is_safe_value(value) {
                    Ok(())
                } else {
                    Err(unsafe_value())
                }
            }
        }
    }
}

/// The settings of harness `id` that can be used, in file order, each with
/// only its usable options and default.
///
/// Whatever cannot be used is logged and left out, and the harness still
/// loads with the rest, as a bad `[status]` rule costs that rule and not the
/// agent. A setting with no `args` and no `env` passes nothing, so it is not
/// offered: every harness file edited before settings had `args` has some.
pub(crate) fn usable(id: &str, settings: Vec<SettingDef>) -> Vec<SettingDef> {
    let mut kept: Vec<SettingDef> = Vec::new();

    for mut setting in settings {
        let key = setting.key.clone();

        if kept.iter().any(|earlier| earlier.key == key) {
            skip(id, &key, "an earlier setting has the same key");
            continue;
        }

        if setting.args.is_empty() && setting.env.is_empty() {
            tracing::info!(
                harness = id,
                setting = %key,
                "a setting with no args or env does nothing; it is not offered"
            );
            continue;
        }

        let names_value = setting
            .args
            .iter()
            .chain(setting.env.values())
            .any(|template| template.contains(VALUE));

        match &mut setting.kind {
            SettingKind::Bool { .. } => {
                if setting.custom {
                    skip(id, &key, "custom is for a choice");
                    continue;
                }
                if names_value {
                    skip(id, &key, "a flag has no value to fill {value} with");
                    continue;
                }
            }
            SettingKind::Text { default } => {
                if setting.custom {
                    skip(id, &key, "custom is for a choice; text is typed already");
                    continue;
                }
                if default
                    .as_deref()
                    .is_some_and(|value| !is_safe_value(value))
                {
                    tracing::warn!(
                        harness = id,
                        setting = %key,
                        "dropping a default a command line cannot safely carry"
                    );
                    *default = None;
                }
            }
            SettingKind::Choice { options, default } => {
                options.retain(|option| {
                    let safe = is_safe_value(&option.value);
                    if !safe {
                        tracing::warn!(
                            harness = id,
                            setting = %key,
                            option = %option.value,
                            "dropping an option a command line cannot safely carry"
                        );
                    }
                    safe
                });
                if options.is_empty() {
                    skip(id, &key, "a choice with no usable option");
                    continue;
                }

                // A label is written to the terminal as it is: an escape
                // sequence in one would redraw the screen.
                for option in options.iter_mut() {
                    let drawable = option.label.as_deref().is_none_or(|label| {
                        !label.trim().is_empty() && !label.chars().any(char::is_control)
                    });
                    if !drawable {
                        tracing::warn!(
                            harness = id,
                            setting = %key,
                            option = %option.value,
                            "dropping a label the popup cannot draw; the value is shown"
                        );
                        option.label = None;
                    }
                }

                let custom = setting.custom;
                let fits = default.as_deref().is_none_or(|value| {
                    options.iter().any(|option| option.value == value)
                        || (custom && is_safe_value(value))
                });
                if !fits {
                    tracing::warn!(
                        harness = id,
                        setting = %key,
                        "dropping a default that is not one of the options"
                    );
                    *default = None;
                }
            }
        }

        kept.push(setting);
    }

    check_limits(id, &mut kept);
    kept
}

/// Drops each `limited_by` of harness `id` that cannot work, and from each
/// option's lists whatever does not name a setting it limits, or a value
/// that setting offers.
///
/// A limit must name another choice of the same harness that is not
/// limited itself: one level only, judged on the limits as written, so the
/// outcome does not depend on the order of the file.
fn check_limits(id: &str, settings: &mut [SettingDef]) {
    let written: Vec<(String, bool, bool)> = settings
        .iter()
        .map(|setting| {
            (
                setting.key.clone(),
                matches!(setting.kind, SettingKind::Choice { .. }),
                setting.limited_by.is_some(),
            )
        })
        .collect();

    for setting in settings.iter_mut() {
        let Some(by) = setting.limited_by.as_deref() else {
            continue;
        };
        let reason = if !matches!(setting.kind, SettingKind::Choice { .. }) {
            Some("only a choice can be limited")
        } else if by == setting.key {
            Some("a setting cannot limit itself")
        } else {
            match written.iter().find(|(key, ..)| key == by) {
                None => Some("limited_by names no setting of this harness"),
                Some((_, false, _)) => Some("limited_by must name a choice"),
                Some((_, _, true)) => Some("limited_by names a setting that is limited itself"),
                Some(_) => None,
            }
        };
        if let Some(reason) = reason {
            tracing::warn!(harness = id, setting = %setting.key, reason, "dropping a limit");
            setting.limited_by = None;
        }
    }

    // (limiter, limited, the values the limited one offers)
    let limits: Vec<(String, String, Vec<String>)> = settings
        .iter()
        .filter_map(|setting| {
            let by = setting.limited_by.clone()?;
            let offered = setting.options().iter().map(|o| o.value.clone()).collect();
            Some((by, setting.key.clone(), offered))
        })
        .collect();

    for setting in settings.iter_mut() {
        let key = setting.key.clone();
        let SettingKind::Choice { options, .. } = &mut setting.kind else {
            continue;
        };
        for option in options.iter_mut() {
            let value = option.value.clone();
            option.allows.retain(|target, listed| {
                let Some((.., offered)) = limits
                    .iter()
                    .find(|(by, limited, _)| *by == key && limited == target)
                else {
                    tracing::warn!(
                        harness = id,
                        setting = %key,
                        option = %value,
                        list = %target,
                        "dropping a list for a setting this one does not limit"
                    );
                    return false;
                };
                listed.retain(|listed_value| {
                    let offers = offered.contains(listed_value);
                    if !offers {
                        tracing::warn!(
                            harness = id,
                            setting = %target,
                            value = %listed_value,
                            "dropping a listed value the setting does not offer"
                        );
                    }
                    offers
                });
                true
            });
        }
    }
}

/// Logs a setting left out of harness `id`, and why.
fn skip(id: &str, key: &str, reason: &str) {
    tracing::warn!(harness = id, setting = key, reason, "skipping a setting");
}

impl HarnessDef {
    /// The values in `saved` this harness can take.
    ///
    /// A key it has no setting for, or a value that setting cannot take, is
    /// logged and left out: the saved file is the user's, and a stale entry
    /// in it must not stop the harness opening.
    #[must_use]
    pub fn usable_saved(&self, saved: &Choices) -> Choices {
        saved
            .iter()
            .filter(|(key, value)| {
                let Some(setting) = self.settings.iter().find(|setting| &setting.key == *key)
                else {
                    tracing::info!(
                        harness = %self.id,
                        setting = %key,
                        "ignoring a saved value for a setting this harness does not have"
                    );
                    return false;
                };
                match setting.check(value) {
                    Ok(()) => true,
                    Err(reason) => {
                        tracing::warn!(harness = %self.id, %reason, "ignoring a saved value");
                        false
                    }
                }
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    /// Each setting's value for one launch: what was `chosen` for it, else
    /// what was `saved`, else the file's default. A setting limited by
    /// another is then fitted to it, as [`fit_limits`] says, rather than
    /// refused.
    ///
    /// A `chosen` value this harness cannot take is refused, naming the
    /// setting. It comes from whoever asked for the pane, and dropping it
    /// quietly could start an agent with its prompts off when the user
    /// turned them on. A `saved` one is only left out, as
    /// [`HarnessDef::usable_saved`] says.
    pub fn resolve(&self, chosen: &Choices, saved: &Choices) -> Result<Choices, String> {
        if let Some(key) = chosen
            .keys()
            .find(|key| !self.settings.iter().any(|setting| &setting.key == *key))
        {
            return Err(format!("{} has no setting {key:?}", self.id));
        }

        let saved = self.usable_saved(saved);
        let mut values = Choices::new();
        for setting in &self.settings {
            let value = match chosen.get(&setting.key) {
                Some(value) => {
                    setting
                        .check(value)
                        .map_err(|reason| format!("{}: {reason}", self.id))?;
                    value.clone()
                }
                None => saved
                    .get(&setting.key)
                    .cloned()
                    .unwrap_or_else(|| setting.file_default()),
            };
            values.insert(setting.key.clone(), value);
        }

        fit_limits(&self.settings, &mut values);
        Ok(values)
    }

    /// The launch for `os`, with the flags and variables `values` turn on
    /// added after its own.
    #[must_use]
    pub fn launch_with(&self, os: &str, values: &Choices) -> Launch {
        let mut launch = self.launch_for(os);
        self.apply_settings(&mut launch, values);
        launch
    }

    /// The one-shot run of `task` on `os`, with the flags and variables
    /// `values` turn on added after its own.
    ///
    /// A subagent gets them as an interactive pane does: without them, a
    /// one-shot agent that must ask before using a tool cannot ask anyone.
    #[must_use]
    pub fn task_launch_with(&self, os: &str, task: &str, values: &Choices) -> Option<TaskRun> {
        let mut run = self.task_launch_for(os, task)?;
        self.apply_settings(&mut run.launch, values);
        Some(run)
    }

    /// Each value in `values` that differs from the file's default, as the
    /// new-pane picker names it beside the harness: an option by its label,
    /// typed text as itself, agent default and flags by the setting's label.
    #[must_use]
    pub fn describe_changes(&self, values: &Choices) -> Vec<String> {
        self.settings
            .iter()
            .filter_map(|setting| {
                let value = values.get(&setting.key)?;
                if *value == setting.file_default() {
                    return None;
                }
                Some(if setting.is_flag() {
                    let state = if value == "true" { "on" } else { "off" };
                    format!("{} {state}", setting.label)
                } else if value.is_empty() {
                    format!("{} agent default", setting.label)
                } else {
                    setting.label_of(value).to_string()
                })
            })
            .collect()
    }

    /// Adds to `launch` what each setting's value in `values` turns on,
    /// once each limited setting is fitted to what limits it.
    fn apply_settings(&self, launch: &mut Launch, values: &Choices) {
        let mut values = values.clone();
        fit_limits(&self.settings, &mut values);

        for setting in &self.settings {
            let value = values.get(&setting.key).map_or("", String::as_str);

            // Checked again, however `values` was come by: what is added
            // lands on a command line cmd.exe parses.
            if let Err(reason) = setting.check(value) {
                tracing::warn!(harness = %self.id, %reason, "leaving out a setting");
                continue;
            }

            let on = if setting.is_flag() {
                value == "true"
            } else {
                !value.is_empty()
            };
            if !on {
                continue;
            }

            launch
                .args
                .extend(setting.args.iter().map(|arg| arg.replace(VALUE, value)));
            for (name, template) in &setting.env {
                launch
                    .env
                    .insert(name.clone(), template.replace(VALUE, value));
            }
        }
    }
}

#[cfg(test)]
mod tests;
