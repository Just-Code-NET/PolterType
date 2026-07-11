use super::*;

#[test]
fn defaults_serialise_and_round_trip() {
    let s = Settings::default();
    let serialized = toml::to_string_pretty(&s).expect("serialize");
    let back: Settings = toml::from_str(&serialized).expect("parse");
    assert_eq!(s, back);
}

#[test]
fn missing_keys_use_defaults() {
    // Minimal valid TOML — every section uses its `Default::default()`.
    let s: Settings = toml::from_str("schema_version = 1").expect("parse");
    assert_eq!(s.engine.min_word_length, 3);
    assert_eq!(s.general.log_level, "info");
    assert!(!s.ai.enabled);
    assert!(s.engine.suppress_in_identifiers);
    assert!(s.engine.suppress_for_all_caps);
}

/// Forward-compat regression: a config that's missing a struct
/// field added after the user wrote the file must still parse —
/// that's what `#[serde(default)]` on every settings struct buys
/// us.
#[test]
fn old_config_missing_new_field_still_parses() {
    let raw = "schema_version = 1\n\n[engine]\nmin_word_length = 4\nconfidence_threshold = 0.7\n";
    let s: Settings = toml::from_str(raw).expect("parse");
    assert_eq!(s.engine.min_word_length, 4);
    // `suppress_in_identifiers` / `suppress_for_all_caps` were
    // missing from the user's file but the defaults kicked in.
    assert!(s.engine.suppress_in_identifiers);
    assert!(s.engine.suppress_for_all_caps);
}

/// User commands sit in their own `[[commands]]` table. A full
/// config block including one must round-trip through the live
/// `Settings` struct — the regression we care about is that
/// `CommandsSettings` is wired in correctly (no `serde(skip)`,
/// no `default` collision dropping the user data on save).
#[test]
fn commands_section_round_trips_inside_full_settings() {
    let raw = r#"
schema_version = 1

[[commands]]
id      = "anrl"
trigger = "anrl"
action  = { type = "type_text", text = "Anatomical Reference List" }
"#;
    let parsed: Settings = toml::from_str(raw).expect("parse with commands");
    assert_eq!(parsed.commands.len(), 1);
    assert_eq!(parsed.commands[0].id, "anrl");
    assert_eq!(parsed.commands[0].trigger, "anrl");

    // And the round-trip back to TOML must preserve the entry —
    // a `Default` collision or stray `serde(skip)` would silently
    // drop it on first save, which is the worst kind of bug.
    let serialised = toml::to_string_pretty(&parsed).expect("serialise");
    let back: Settings = toml::from_str(&serialised).expect("parse round-trip");
    assert_eq!(back.commands.len(), 1);
    assert_eq!(back.commands[0].id, "anrl");
    assert_eq!(back.commands[0].trigger, "anrl");
}

/// Legacy configs from beta.4 and earlier had no `[[commands]]`
/// section. They must still parse — the user shouldn't have to
/// edit their config to keep the app starting.
#[test]
fn legacy_config_without_commands_still_parses() {
    let raw = r#"
schema_version = 1

[hotkeys]
pause_toggle = "Ctrl+Shift+Space"
manual_switch_last = "Ctrl+Shift+Backspace"
"#;
    let parsed: Settings = toml::from_str(raw).expect("parse legacy");
    assert!(parsed.commands.is_empty());
    assert_eq!(parsed.hotkeys.pause_toggle, "Ctrl+Shift+Space");
}

#[test]
fn default_disabled_apps_covers_common_editors() {
    let s = Settings::default();
    let lower: Vec<String> = s
        .exceptions
        .disabled_apps
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    for must in ["code.exe", "cursor.exe", "windowsterminal.exe", "alacritty"] {
        assert!(
            lower.iter().any(|s| s == must),
            "expected `{must}` in default disabled_apps"
        );
    }
}

// ─── Legacy kb-switcher config migration ──────────────────────────

struct TmpDir(std::path::PathBuf);

impl TmpDir {
    fn new(label: &str) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "poltertype-test-{label}-{}-{now}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("mkdir tmp");
        Self(path)
    }

    fn write(&self, rel: &str, body: &str) {
        let path = self.0.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir tmp parent");
        }
        std::fs::write(path, body).expect("write tmp file");
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.0.join(rel)).expect("read tmp file")
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn migrates_legacy_tree_on_first_launch() {
    let legacy = TmpDir::new("legacy-src");
    let fresh = TmpDir::new("legacy-dst");
    legacy.write("config.toml", "schema_version = 1\n");
    legacy.write("wordlists/uk_ua.txt", "своєслово\n");

    assert!(migrate_dir(&legacy.0, &fresh.0));
    assert_eq!(fresh.read("config.toml"), "schema_version = 1\n");
    assert_eq!(fresh.read("wordlists/uk_ua.txt"), "своєслово\n");
    // The legacy tree stays behind as a backup.
    assert_eq!(legacy.read("config.toml"), "schema_version = 1\n");
}

#[test]
fn migration_never_overwrites_existing_files() {
    let legacy = TmpDir::new("clobber-src");
    let fresh = TmpDir::new("clobber-dst");
    legacy.write("config.toml", "schema_version = 1 # legacy\n");
    fresh.write("config.toml", "schema_version = 1 # mine\n");

    assert!(!migrate_dir(&legacy.0, &fresh.0));
    assert_eq!(fresh.read("config.toml"), "schema_version = 1 # mine\n");
}

#[test]
fn migration_skips_present_overlays_but_copies_the_rest() {
    let legacy = TmpDir::new("partial-src");
    let fresh = TmpDir::new("partial-dst");
    legacy.write("config.toml", "schema_version = 1\n");
    legacy.write("wordlists/uk_ua.txt", "старе\n");
    fresh.write("wordlists/uk_ua.txt", "нове\n");

    assert!(migrate_dir(&legacy.0, &fresh.0));
    // Pre-existing file kept, missing one copied.
    assert_eq!(fresh.read("wordlists/uk_ua.txt"), "нове\n");
    assert_eq!(fresh.read("config.toml"), "schema_version = 1\n");
}

#[test]
fn no_migration_without_legacy_config_toml() {
    let legacy = TmpDir::new("noconf-src");
    let fresh = TmpDir::new("noconf-dst");
    legacy.write("wordlists/uk_ua.txt", "слово\n");

    assert!(!migrate_dir(&legacy.0, &fresh.0));
    assert!(!fresh.0.join("wordlists").exists());
}
