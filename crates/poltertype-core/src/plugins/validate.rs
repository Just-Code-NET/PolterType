//! Checking an extension's manifest before anything is run.
//!
//! Everything here happens on static text read from disk, before the
//! plug-in's program is launched — which is the whole reason the
//! contribution model is declarative.
//!
//! The checks are about what PolterType would otherwise be tricked into
//! doing on the plug-in's behalf, not about the plug-in being
//! well-written: a program only **out of its own `bin/`**, by plain file
//! name, so a manifest cannot point at `/usr/bin/env` or a sibling pack;
//! every button and tray entry naming a command that exists; every
//! stored control naming its config key.

use super::enums::{ControlKind, PluginError};
use super::types::ExtensionManifest;

/// Refuse a manifest that would leave PolterType doing something
/// unclear on the plug-in's behalf.
pub fn check_extension(m: &ExtensionManifest) -> Result<(), PluginError> {
    let exe = m.exe.trim();
    if exe.is_empty() {
        return Err(PluginError::NoExecutable(
            "manifest has no [extension] exe".to_owned(),
        ));
    }
    if !is_plain_file_name(exe) {
        return Err(PluginError::BadExecutablePath(exe.to_owned()));
    }

    for c in &m.commands {
        if c.id.trim().is_empty() {
            return Err(PluginError::BadPane("a command has no id".to_owned()));
        }
    }

    for item in &m.tray_items {
        // A status entry reports rather than acts, so it has no command;
        // requiring one would force authors to invent a do-nothing one.
        if item.is_status() {
            if m.state_args.is_empty() {
                return Err(PluginError::BadPane(format!(
                    "tray item {:?} shows state, but the manifest declares no state_args to \
                     read it with",
                    item.label
                )));
            }
            continue;
        }
        if !m.commands.iter().any(|c| c.id == item.command) {
            return Err(PluginError::BadPane(format!(
                "tray item {:?} refers to unknown command {:?}",
                item.label, item.command
            )));
        }
        // A tick that nothing ever sets would sit permanently unticked
        // and quietly claim the mode is never active.
        if item.is_check() && m.state_args.is_empty() {
            return Err(PluginError::BadPane(format!(
                "tray item {:?} declares state_value, but the manifest declares no \
                 state_args to read state with",
                item.label
            )));
        }
    }

    for control in &m.pane {
        match control.kind {
            ControlKind::Button | ControlKind::Report => {
                // Both name a command rather than a key; pointed at one
                // nobody declared, a report renders an empty box for
                // ever, which reads as "nothing to say".
                if !m.commands.iter().any(|c| c.id == control.command) {
                    let what = if control.kind == ControlKind::Button {
                        "button"
                    } else {
                        "report"
                    };
                    return Err(PluginError::BadPane(format!(
                        "{what} {:?} refers to unknown command {:?}",
                        control.label, control.command
                    )));
                }
            }
            ControlKind::Choice => {
                if control.options.is_empty() {
                    return Err(PluginError::BadPane(format!(
                        "choice {:?} offers no options",
                        control.label
                    )));
                }
                for option in &control.options {
                    if option.value().trim().is_empty() {
                        return Err(PluginError::BadPane(format!(
                            "choice {:?} has an option with no value",
                            control.label
                        )));
                    }
                    // A link in a manifest is a third party naming a
                    // place PolterType will send somebody: `https` only,
                    // and the pane shows the address as the link text,
                    // so a plug-in cannot mislabel a destination.
                    let link = option.link().trim();
                    if !link.is_empty() && !link.starts_with("https://") {
                        return Err(PluginError::BadPane(format!(
                            "choice {:?} links to {link:?} — only https:// links are shown",
                            control.label
                        )));
                    }
                }
                check_key(control.key.trim(), &control.label)?;
            }
            ControlKind::Suggest => {
                check_key(control.key.trim(), &control.label)?;
                check_suggestions(m, control, "suggest")?;
            }
            ControlKind::Toggle
            | ControlKind::Text
            | ControlKind::Number
            | ControlKind::Decimal
            | ControlKind::Strings => {
                check_key(control.key.trim(), &control.label)?;
            }
            // A heading stores nothing and runs nothing; unlabelled, it
            // draws a fold arrow with no way to tell what is behind it.
            ControlKind::Section => {
                if control.label.trim().is_empty() {
                    return Err(PluginError::BadPane(
                        "a section heading has no label".to_owned(),
                    ));
                }
            }
            ControlKind::List => {
                // Without the command there are no rows to tick; without
                // the key a tick has nowhere to go.
                check_key(control.key.trim(), &control.label)?;
                if !m.commands.iter().any(|c| c.id == control.command) {
                    return Err(PluginError::BadPane(format!(
                        "list {:?} refers to unknown command {:?}",
                        control.label, control.command
                    )));
                }
            }
            ControlKind::Records => {
                check_key(control.key.trim(), &control.label)?;
                if control.fields.is_empty() {
                    return Err(PluginError::BadPane(format!(
                        "records {:?} declares no fields — a row would be an empty card",
                        control.label
                    )));
                }
                for field in &control.fields {
                    // A row is one table; a dotted key inside it would
                    // nest, and nesting makes this a config editor.
                    let key = field.key.trim();
                    if key.contains('.') {
                        return Err(PluginError::BadPane(format!(
                            "records {:?} has a field keyed {key:?} — a row's field is one \
                             name, not a path",
                            control.label
                        )));
                    }
                    check_key(key, &field.label)?;
                    match field.kind {
                        ControlKind::Toggle
                        | ControlKind::Text
                        | ControlKind::Number
                        | ControlKind::Decimal
                        | ControlKind::Choice
                        | ControlKind::Suggest
                        | ControlKind::Strings => {}
                        other => {
                            return Err(PluginError::BadPane(format!(
                                "records {:?} has a {other:?} field — a row holds values, not \
                                 sections, buttons, reports or more rows",
                                control.label
                            )));
                        }
                    }
                    if field.kind == ControlKind::Choice && field.options.is_empty() {
                        return Err(PluginError::BadPane(format!(
                            "records {:?} has a choice field {:?} with no options",
                            control.label, field.label
                        )));
                    }
                    if field.kind == ControlKind::Suggest {
                        check_suggestions(m, field, "records field")?;
                    }
                }
                check_row_actions(m, control)?;
            }
            // Nothing to check: refusing a manifest for containing an
            // unknown control would defeat the point of tolerating one.
            ControlKind::Unknown => {}
        }
    }

    if !m.config_file.is_empty() && !is_plain_file_name(&m.config_file) {
        return Err(PluginError::BadPane(format!(
            "config_file {:?} must be a plain file name",
            m.config_file
        )));
    }

    Ok(())
}

/// A box that suggests has to have something to suggest — from the
/// manifest, from a command, or from both. With neither it is a plain
/// text box wearing a drop-down arrow that never opens.
fn check_suggestions(
    m: &ExtensionManifest,
    control: &super::types::PaneControl,
    what: &str,
) -> Result<(), PluginError> {
    let command = control.command.trim();
    if !command.is_empty() && !m.commands.iter().any(|c| c.id == command) {
        return Err(PluginError::BadPane(format!(
            "{what} {:?} refers to unknown command {command:?}",
            control.label
        )));
    }
    if command.is_empty() && control.options.is_empty() {
        return Err(PluginError::BadPane(format!(
            "{what} {:?} suggests nothing — give it options, a command, or both",
            control.label
        )));
    }
    for option in &control.options {
        if option.value().trim().is_empty() {
            return Err(PluginError::BadPane(format!(
                "{what} {:?} has an option with no value",
                control.label
            )));
        }
    }
    Ok(())
}

/// Buttons on a row must run something that exists and must know which
/// field names the row: a `{id}` nothing fills in would run the command
/// against the literal string — refused at load, not at the click.
fn check_row_actions(
    m: &ExtensionManifest,
    control: &super::types::PaneControl,
) -> Result<(), PluginError> {
    if control.actions.is_empty() {
        return Ok(());
    }
    let id_field = control.id_field.trim();
    if id_field.is_empty() {
        return Err(PluginError::BadPane(format!(
            "records {:?} has row actions but no id_field saying which field names a row",
            control.label
        )));
    }
    if !control.fields.iter().any(|f| f.key.trim() == id_field) {
        return Err(PluginError::BadPane(format!(
            "records {:?} names {id_field:?} as its id_field, which is not one of its fields",
            control.label
        )));
    }
    for action in &control.actions {
        if !m.commands.iter().any(|c| c.id == action.command) {
            return Err(PluginError::BadPane(format!(
                "records {:?} action {:?} refers to unknown command {:?}",
                control.label, action.label, action.command
            )));
        }
    }
    Ok(())
}

fn check_key(key: &str, label: &str) -> Result<(), PluginError> {
    if key.is_empty() {
        return Err(PluginError::BadPane(format!(
            "control {label:?} stores a value but names no config key"
        )));
    }
    // A key becomes a path into the plug-in's own TOML. Restricting it
    // to identifier characters keeps a manifest from reaching sideways
    // into a table it did not mean to name.
    let ok = key
        .split('.')
        .all(|part| !part.is_empty() && part.chars().all(|c| c.is_alphanumeric() || c == '_'));
    if !ok {
        return Err(PluginError::BadPane(format!(
            "control {label:?} has an unusable config key {key:?}"
        )));
    }
    Ok(())
}

/// Is this a bare file name — no directories, no traversal, no root?
fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && name != "."
        && name != ".."
        && !name.starts_with('.')
}

#[cfg(test)]
mod tests;
