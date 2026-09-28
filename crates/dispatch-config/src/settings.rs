//! A harness's settings: what a value may be, and which of a harness file's
//! settings can be used.
//!
//! A setting says how it becomes a flag -- its own `args` and `env`, with
//! `{value}` standing for the value -- so a harness Dispatch has never seen
//! gets a settings popup from its file alone.

use crate::harness::{SettingDef, SettingKind};

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

impl SettingDef {
    /// Whether this is on or off, rather than a value.
    #[must_use]
    pub fn is_flag(&self) -> bool {
        matches!(self.kind, SettingKind::Bool { .. })
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
                if value.is_empty() || options.iter().any(|option| option == value) {
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
                    let safe = is_safe_value(option);
                    if !safe {
                        tracing::warn!(
                            harness = id,
                            setting = %key,
                            %option,
                            "dropping an option a command line cannot safely carry"
                        );
                    }
                    safe
                });
                if options.is_empty() {
                    skip(id, &key, "a choice with no usable option");
                    continue;
                }

                let custom = setting.custom;
                let fits = default.as_deref().is_none_or(|value| {
                    options.iter().any(|option| option == value) || (custom && is_safe_value(value))
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

    kept
}

/// Logs a setting left out of harness `id`, and why.
fn skip(id: &str, key: &str, reason: &str) {
    tracing::warn!(harness = id, setting = key, reason, "skipping a setting");
}

#[cfg(test)]
mod tests;
