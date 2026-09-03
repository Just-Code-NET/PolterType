//! Reading and writing one setting in a plug-in's own config file.
//!
//! That file belongs to the plug-in and is mostly prose — comments
//! explaining what each switch costs. Parsing it into a struct and
//! serialising it back would delete all of that the first time a user
//! touched a toggle, so edits go through `toml_edit`, which changes only
//! the value asked for.
//!
//! Strings in, strings out: file IO belongs to the caller. Missing
//! tables are created; nothing else is invented, so an absent key reads
//! back as `None` rather than as a guess at the plug-in's default.

use toml_edit::{Document, Item, Value};

use super::enums::{PluginError, SettingValue};

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
        Value::Float(f) => Some(SettingValue::Float(*f.value())),
        _ => None,
    }
}

/// Read an array of strings at a dotted key. A missing key is an empty
/// list, not an error: a plug-in whose allow-list is absent allows
/// nothing, which is what the unticked boxes say.
pub fn read_string_array(text: &str, key: &str) -> Vec<String> {
    let Ok(doc) = text.parse::<Document>() else {
        return Vec::new();
    };
    let mut item: &Item = doc.as_item();
    for part in key.split('.') {
        match item.get(part) {
            Some(next) => item = next,
            None => return Vec::new(),
        }
    }
    item.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Add `member` to the array at `key`, or take it out. The array is
/// created if missing and otherwise left exactly as written, comments
/// included. Adding something already there, or removing something that
/// is not, is a no-op rather than an error.
pub fn set_array_member(
    text: &str,
    key: &str,
    member: &str,
    present: bool,
) -> Result<String, PluginError> {
    set_array_members(text, key, std::slice::from_ref(&member), present)
}

/// The same for a whole set at once: one parse and one write, not one
/// per member, so a "select all" cannot be caught half-finished by
/// another program reading the same file.
pub fn set_array_members(
    text: &str,
    key: &str,
    members: &[&str],
    present: bool,
) -> Result<String, PluginError> {
    let mut doc: Document = text
        .parse()
        .map_err(|e| PluginError::BadManifest(format!("plug-in config is not valid TOML: {e}")))?;
    let (item, last) = reach_mut(&mut doc, key)?;

    if item.get(last).is_none() {
        item[last] = Item::Value(Value::Array(toml_edit::Array::new()));
    }
    let array = item
        .get_mut(last)
        .and_then(|i| i.as_array_mut())
        .ok_or_else(|| PluginError::BadPane(format!("config key {key:?} is not an array")))?;

    for member in members {
        let at = array
            .iter()
            .position(|v| v.as_str().is_some_and(|s| s == *member));
        match (present, at) {
            (true, None) => array.push(*member),
            (false, Some(i)) => {
                array.remove(i);
            }
            _ => {}
        }
    }
    Ok(doc.to_string())
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
    let (item, last) = reach_mut(&mut doc, key)?;

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
        SettingValue::Float(f) => (*f).into(),
        SettingValue::Text(s) => s.as_str().into(),
    };

    // Replace the *value*, not the entry: assigning a fresh `Item` over
    // the key takes its decor with it, and the decor is where a trailing
    // comment lives (`mode = "learn"  # ...`).
    match item.get_mut(last).and_then(|i| i.as_value_mut()) {
        Some(existing) => {
            let decor = existing.decor().clone();
            *existing = new;
            *existing.decor_mut() = decor;
        }
        None => item[last] = Item::Value(new),
    }
    Ok(doc.to_string())
}

/// Replace the whole array at `key` with `values`, for a set the user
/// types rather than ticks. The entry's decor survives, so a trailing
/// comment is kept; the previous members do not.
pub fn write_string_array(text: &str, key: &str, values: &[String]) -> Result<String, PluginError> {
    let mut doc: Document = text
        .parse()
        .map_err(|e| PluginError::BadManifest(format!("plug-in config is not valid TOML: {e}")))?;
    let (item, last) = reach_mut(&mut doc, key)?;

    if item
        .get(last)
        .is_some_and(|existing| existing.is_table_like())
    {
        return Err(PluginError::BadPane(format!(
            "config key {key:?} names a table, not a list"
        )));
    }

    let mut array = toml_edit::Array::new();
    for v in values {
        array.push(v.as_str());
    }
    match item.get_mut(last).and_then(|i| i.as_value_mut()) {
        Some(existing) => {
            let decor = existing.decor().clone();
            *existing = Value::Array(array);
            *existing.decor_mut() = decor;
        }
        None => item[last] = Item::Value(Value::Array(array)),
    }
    Ok(doc.to_string())
}

/// How many rows the array of tables at `key` has; zero for a missing
/// key.
pub fn count_records(text: &str, key: &str) -> usize {
    let Ok(doc) = text.parse::<Document>() else {
        return 0;
    };
    let mut item: &Item = doc.as_item();
    for part in key.split('.') {
        match item.get(part) {
            Some(next) => item = next,
            None => return 0,
        }
    }
    item.as_array_of_tables().map_or(0, |a| a.len())
}

/// Read one field of one row of an array of tables — `schedule.sends`,
/// row 2, field `room`. `None` for a missing row or an omitted field.
pub fn read_record_field(text: &str, key: &str, row: usize, field: &str) -> Option<SettingValue> {
    let doc: Document = text.parse().ok()?;
    let mut item: &Item = doc.as_item();
    for part in key.split('.') {
        item = item.get(part)?;
    }
    let table = item.as_array_of_tables()?.get(row)?;
    match table.get(field)?.as_value()? {
        Value::Boolean(b) => Some(SettingValue::Bool(*b.value())),
        Value::Integer(n) => Some(SettingValue::Int(*n.value())),
        Value::String(s) => Some(SettingValue::Text(s.value().clone())),
        Value::Float(f) => Some(SettingValue::Float(*f.value())),
        _ => None,
    }
}

/// Set one field of one row. Refuses a row that is not there rather
/// than growing the array to reach it — an index past the end means the
/// file changed under the pane, not that rows should be invented.
pub fn write_record_field(
    text: &str,
    key: &str,
    row: usize,
    field: &str,
    value: &SettingValue,
) -> Result<String, PluginError> {
    let mut doc: Document = text
        .parse()
        .map_err(|e| PluginError::BadManifest(format!("plug-in config is not valid TOML: {e}")))?;
    let table = record_mut(&mut doc, key, row)?;
    let new: Value = match value {
        SettingValue::Bool(b) => (*b).into(),
        SettingValue::Int(n) => (*n).into(),
        SettingValue::Float(f) => (*f).into(),
        SettingValue::Text(s) => s.as_str().into(),
    };
    // Decor-preserving replacement, as in `write_setting`: the trailing
    // comment on the line is the user's note to themselves.
    match table.get_mut(field).and_then(|i| i.as_value_mut()) {
        Some(existing) => {
            let decor = existing.decor().clone();
            *existing = new;
            *existing.decor_mut() = decor;
        }
        None => table[field] = Item::Value(new),
    }
    Ok(doc.to_string())
}

/// Append an empty row to the array of tables at `key`, creating the
/// array if it is not there yet. Empty rather than pre-filled: the
/// plug-in's own defaults apply to every field left out.
pub fn add_record(text: &str, key: &str) -> Result<String, PluginError> {
    let mut doc: Document = text
        .parse()
        .map_err(|e| PluginError::BadManifest(format!("plug-in config is not valid TOML: {e}")))?;
    let (item, last) = reach_mut(&mut doc, key)?;
    if item.get(last).is_none() {
        item[last] = Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }
    let array = item
        .get_mut(last)
        .and_then(|i| i.as_array_of_tables_mut())
        .ok_or_else(|| {
            PluginError::BadPane(format!("config key {key:?} is not an array of tables"))
        })?;
    array.push(toml_edit::Table::new());
    Ok(doc.to_string())
}

/// Delete row `row` of the array of tables at `key`. A row that is not
/// there is a no-op, not an error.
pub fn remove_record(text: &str, key: &str, row: usize) -> Result<String, PluginError> {
    let mut doc: Document = text
        .parse()
        .map_err(|e| PluginError::BadManifest(format!("plug-in config is not valid TOML: {e}")))?;
    let (item, last) = reach_mut(&mut doc, key)?;
    if let Some(array) = item.get_mut(last).and_then(|i| i.as_array_of_tables_mut()) {
        if row < array.len() {
            array.remove(row);
        }
    }
    Ok(doc.to_string())
}

fn record_mut<'a>(
    doc: &'a mut Document,
    key: &'a str,
    row: usize,
) -> Result<&'a mut toml_edit::Table, PluginError> {
    let (item, last) = reach_mut(doc, key)?;
    let array = item
        .get_mut(last)
        .and_then(|i| i.as_array_of_tables_mut())
        .ok_or_else(|| {
            PluginError::BadPane(format!("config key {key:?} is not an array of tables"))
        })?;
    let len = array.len();
    array
        .get_mut(row)
        .ok_or_else(|| PluginError::BadPane(format!("{key:?} has {len} row(s), not {}", row + 1)))
}

/// Walk a dotted key down to the table holding its last segment,
/// creating the tables in between. A segment that is a *value* is
/// refused rather than replaced — turning it into a table would throw
/// away whatever the user had there.
fn reach_mut<'a>(
    doc: &'a mut Document,
    key: &'a str,
) -> Result<(&'a mut Item, &'a str), PluginError> {
    let parts: Vec<&str> = key.split('.').collect();
    let (last, tables) = parts
        .split_last()
        .ok_or_else(|| PluginError::BadPane(format!("empty config key {key:?}")))?;

    let mut item: &mut Item = doc.as_item_mut();
    for part in tables {
        if !item.is_table_like() && !item.is_none() {
            return Err(PluginError::BadPane(format!(
                "config key {key:?} runs through {part:?}, which is not a table"
            )));
        }
        if item.get(part).is_none() {
            item[*part] = toml_edit::table();
        }
        item = &mut item[*part];
    }
    Ok((item, last))
}

#[cfg(test)]
mod tests;
