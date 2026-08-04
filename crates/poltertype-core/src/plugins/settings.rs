//! Reading and writing one setting in a plug-in's own config file.
//!
//! A plug-in's settings pane edits the plug-in's config, not ours. That
//! file belongs to the plug-in and, in the one case this was written
//! for, is mostly prose: comments explaining what each switch means and
//! what turning it on costs. Parsing it into a struct and serialising
//! it back would silently delete all of that the first time a user
//! touched a toggle — so edits go through `toml_edit`, which keeps the
//! document as written and changes only the value asked for.
//!
//! Everything here works on strings rather than paths, so it is pure:
//! the file IO belongs to the caller, and the interesting behaviour is
//! testable without a filesystem.
//!
//! Missing tables are created, because a plug-in's config file is
//! allowed to omit anything it has a default for — the pane should
//! still be able to set it. Nothing else is invented: a key that does
//! not exist reads back as `None`, and the pane shows its own default
//! rather than pretending to know the plug-in's.

use toml_edit::{Document, Item, Value};

use super::enums::PluginError;

/// A value a pane control can hold. Deliberately the three shapes the
/// controls produce, rather than all of TOML: a pane is not a config
/// editor, and a control that cannot render a value has no business
/// writing one.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    Bool(bool),
    Int(i64),
    Text(String),
}

impl SettingValue {
    /// How it should appear in a text field.
    pub fn as_display(&self) -> String {
        match self {
            Self::Bool(b) => b.to_string(),
            Self::Int(n) => n.to_string(),
            Self::Text(s) => s.clone(),
        }
    }
}

/// Read the value at a dotted key, or `None` if the file does not set
/// it — which is normal, not an error.
pub fn read_setting(text: &str, key: &str) -> Option<SettingValue> {
    let doc: Document = text.parse().ok()?;
    let mut item: &Item = doc.as_item();
    for part in key.split('.') {
        item = item.get(part)?;
    }
    match item.as_value()? {
        Value::Boolean(b) => Some(SettingValue::Bool(*b.value())),
        Value::Integer(n) => Some(SettingValue::Int(*n.value())),
        Value::String(s) => Some(SettingValue::Text(s.value().clone())),
        Value::Float(f) => Some(SettingValue::Text(f.value().to_string())),
        _ => None,
    }
}

/// Set the value at a dotted key, returning the whole file as it should
/// now be written.
///
/// Comments, ordering, spacing and every other key survive; only the
/// one value changes.
pub fn write_setting(text: &str, key: &str, value: &SettingValue) -> Result<String, PluginError> {
    let mut doc: Document = text
        .parse()
        .map_err(|e| PluginError::BadManifest(format!("plug-in config is not valid TOML: {e}")))?;

    let parts: Vec<&str> = key.split('.').collect();
    let (last, tables) = parts
        .split_last()
        .ok_or_else(|| PluginError::BadPane(format!("empty config key {key:?}")))?;

    let mut item: &mut Item = doc.as_item_mut();
    for part in tables {
        // A key whose parent is a value, not a table, cannot be
        // reached — and quietly replacing that value with a table
        // would throw away whatever the user had there.
        if !item.is_table_like() && !item.is_none() {
            return Err(PluginError::BadPane(format!(
                "config key {key:?} runs through {part:?}, which is not a table"
            )));
        }
        if item.get(part).is_none() {
            let table = toml_edit::table();
            item[*part] = table;
        }
        item = &mut item[*part];
    }

    if !item.is_table_like() && !item.is_none() {
        return Err(PluginError::BadPane(format!(
            "config key {key:?} does not live in a table"
        )));
    }
    if item
        .get(last)
        .is_some_and(|existing| existing.is_table_like())
    {
        return Err(PluginError::BadPane(format!(
            "config key {key:?} names a table, not a value"
        )));
    }

    let new: Value = match value {
        SettingValue::Bool(b) => (*b).into(),
        SettingValue::Int(n) => (*n).into(),
        SettingValue::Text(s) => s.as_str().into(),
    };

    // Replace the *value*, not the entry. Assigning a fresh `Item`
    // over the key would take its decor with it — and the decor is
    // where a trailing comment lives, so `mode = "learn"  # ...` would
    // quietly lose the explanation next to it the first time anyone
    // moved the switch.
    match item.get_mut(last).and_then(|i| i.as_value_mut()) {
        Some(existing) => {
            let decor = existing.decor().clone();
            *existing = new;
            *existing.decor_mut() = decor;
        }
        None => item[*last] = Item::Value(new),
    }
    Ok(doc.to_string())
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
