//! Reading and writing one setting in a plug-in's own config file.
//!
//! That file belongs to the plug-in and is mostly prose in the case
//! this was written for: comments explaining what each switch means and
//! what turning it on costs. Parsing it into a struct and serialising
//! it back would delete all of that the first time a user touched a
//! toggle, so edits go through `toml_edit`, which keeps the document as
//! written and changes only the value asked for.
//!
//! Everything works on strings rather than paths, so the file IO
//! belongs to the caller and the interesting behaviour is testable
//! without a filesystem.
//!
//! Missing tables are created, because a plug-in's config may omit
//! anything it has a default for. Nothing else is invented: an absent
//! key reads back as `None` and the pane shows its own default rather
//! than pretending to know the plug-in's.

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
    /// Kept apart from [`Self::Int`] all the way to the file: TOML's two
    /// number types are not interchangeable to the program that reads
    /// the result, and a plug-in expecting `0.35` refuses to start on
    /// `1`.
    Float(f64),
    Text(String),
}

impl SettingValue {
    /// How it should appear in a text field.
    pub fn as_display(&self) -> String {
        match self {
            Self::Bool(b) => b.to_string(),
            Self::Int(n) => n.to_string(),
            // Always with a point, even for a round number — that is
            // what the file holds, and `25` shown for a `25.0` invites
            // the user to save back an integer the plug-in cannot read.
            Self::Float(f) if f.is_finite() && f.fract() == 0.0 => format!("{f:.1}"),
            Self::Float(f) => f.to_string(),
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
        Value::Float(f) => Some(SettingValue::Float(*f.value())),
        _ => None,
    }
}

/// Read an array of strings at a dotted key.
///
/// Separate from [`read_setting`] rather than another [`SettingValue`]
/// variant, because an array is not a value a control *holds* — it is a
/// set a control adds itself to or removes itself from, one checkbox
/// per candidate asking the same question.
///
/// A missing key is an empty list, not an error: a plug-in whose
/// allow-list is absent allows nothing, which is what the unticked
/// boxes will say.
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

/// Add `member` to the array at `key`, or take it out.
///
/// The array is created if missing and left exactly as written
/// otherwise, comments included — in the file this was written for they
/// explain why each entry is there and, more importantly, why certain
/// applications are deliberately *absent*.
///
/// Adding something already there, or removing something that is not,
/// is a no-op rather than an error: the checkbox and the file can
/// disagree for a moment, and the honest resolution is what the user
/// just asked for.
pub fn set_array_member(
    text: &str,
    key: &str,
    member: &str,
    present: bool,
) -> Result<String, PluginError> {
    set_array_members(text, key, std::slice::from_ref(&member), present)
}

/// The same for a whole set at once.
///
/// One parse and one write, not one per member. A pane offering sixty
/// conversations and a "select all" that wrote the file sixty times
/// would be sixty chances for another program reading the same file to
/// catch it half-finished, and sixty rewrites of comments it does not
/// own — for one action the user thinks of as single.
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
        None => item[last] = Item::Value(new),
    }
    Ok(doc.to_string())
}

/// Replace the whole array at `key` with `values`.
///
/// The counterpart to [`set_array_member`] for a set the plug-in cannot
/// enumerate — host names, window titles — where the user types the
/// members rather than ticking them. The entry's own decor survives, so
/// a trailing comment on the line is kept; the previous members do not,
/// because "what is in this list" is exactly what the user just said.
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

/// How many rows the array of tables at `key` has.
///
/// Zero for a missing key, which is right: a plug-in whose config has no
/// `[[schedule.sends]]` has no scheduled sends.
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
/// row 2, field `room`.
///
/// The pane's repeating-group control is one of these per field per row.
/// `None` for a row that does not exist or a field the row omits, which
/// is the same "the plug-in has a default and we do not know it" the
/// scalar reader already reports.
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

/// Set one field of one row.
///
/// Refuses a row that is not there rather than growing the array to
/// reach it: the pane draws the rows it read, and an index past the end
/// means the file changed underneath it — in which case inventing three
/// empty scheduled messages to make room for the fourth is the worst
/// available answer.
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
    // The same decor-preserving replacement the scalar writer does, and
    // for the same reason: the comment after `when = "weekdays 09:00"`
    // is the user's note to themselves about why.
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
/// array if it is not there yet.
///
/// Empty rather than pre-filled: the plug-in's own defaults apply to
/// every field left out, and guessing at them here would write a
/// scheduled message with an hour nobody chose.
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

/// Delete row `row` of the array of tables at `key`.
///
/// A row that is not there is a no-op, not an error — the pane and the
/// file can disagree for a moment, and the user's intent ("this one is
/// gone") is already satisfied.
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
/// creating the tables in between.
///
/// Shared by every writer so they agree on what a key means. Missing
/// tables are created because a plug-in's config may omit anything it
/// has a default for; a segment that is a *value* is refused rather
/// than replaced, since turning it into a table would throw away
/// whatever the user had there.
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
#[path = "settings_tests.rs"]
mod tests;
