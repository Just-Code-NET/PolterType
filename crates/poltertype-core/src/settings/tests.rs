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

/// Every user upgrading from 0.3.x has a `config.toml` with no
/// `[updates]` section at all. It has to keep working, and it has to
/// land on "updates on" — a silent fallback to *off* would mean the
/// exact population that most needs the updater (people already running
/// an old build) never gets it.
#[test]
fn a_config_predating_the_updater_defaults_to_updates_on() {
    let raw = "schema_version = 1\n\n[general]\nautostart = true\n";
    let s: Settings = toml::from_str(raw).expect("parse");
    assert!(s.updates.enabled);
    assert_eq!(s.updates.check_interval_hours, 24);
}

#[test]
fn updates_can_be_turned_off_from_the_config_file() {
    let raw = "schema_version = 1\n\n[updates]\nenabled = false\n";
    let s: Settings = toml::from_str(raw).expect("parse");
    assert!(!s.updates.enabled);
}

/// A hand-edited `0` — a typo, or someone reasoning that zero means
/// "never" — must not turn every installed copy of the app into a tight
/// request loop against GitHub.
#[test]
fn a_zero_check_interval_is_clamped_not_obeyed() {
    let raw = "schema_version = 1\n\n[updates]\ncheck_interval_hours = 0\n";
    let s: Settings = toml::from_str(raw).expect("parse");
    assert_eq!(
        s.updates.interval(),
        std::time::Duration::from_secs(MIN_UPDATE_INTERVAL_HOURS * 3600)
    );
}

#[test]
fn a_sane_check_interval_is_honoured() {
    let s = UpdateSettings {
        enabled: true,
        check_interval_hours: 12,
    };
    assert_eq!(s.interval(), std::time::Duration::from_secs(12 * 3600));
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

/// A fresh install auto-switches everywhere. We ship no app skip-list:
/// the previous default silently disabled the app in every editor,
/// IDE and terminal, which — once a Linux focus tracker existed to
/// enforce it — was reported as "layout switching is broken".
#[test]
fn default_disabled_apps_is_empty() {
    assert!(Settings::default().exceptions.disabled_apps.is_empty());
}

/// The list is still honoured — it is opt-in, not gone.
#[test]
fn user_supplied_disabled_apps_round_trips() {
    let raw = r#"
schema_version = 1

[exceptions]
disabled_apps = ["Code.exe", "kitty"]
"#;
    let parsed: Settings = toml::from_str(raw).expect("parse exceptions");
    assert_eq!(parsed.exceptions.disabled_apps, ["Code.exe", "kitty"]);
}

/// A `config.toml` with no `[exceptions]` block at all must not
/// resurrect a skip-list through some other default path.
#[test]
fn absent_exceptions_block_yields_no_skips() {
    let parsed: Settings = toml::from_str("schema_version = 1\n").expect("parse minimal");
    assert!(parsed.exceptions.disabled_apps.is_empty());
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

// ─── Retiring the shipped default skip-list ───────────────────────

/// The whole point: a config written by v0.4.1 or earlier carries the
/// 69-entry default, and an upgrade must clear it — otherwise the new
/// empty default in the binary changes nothing for existing users.
#[test]
fn retires_an_untouched_shipped_skip_list() {
    let mut s = Settings::default();
    s.exceptions.disabled_apps = LEGACY_DEFAULT_DISABLED_APPS
        .iter()
        .map(|a| (*a).to_owned())
        .collect();

    assert!(retire_default_skip_list(&mut s));
    assert!(s.exceptions.disabled_apps.is_empty());
}

/// Order is not part of the identity — TOML round-trips and hand edits
/// reorder freely, and a reordered list is still the untouched default.
#[test]
fn retires_the_shipped_list_regardless_of_order() {
    let mut apps: Vec<String> = LEGACY_DEFAULT_DISABLED_APPS
        .iter()
        .map(|a| (*a).to_owned())
        .collect();
    apps.reverse();
    let mut s = Settings::default();
    s.exceptions.disabled_apps = apps;

    assert!(retire_default_skip_list(&mut s));
    assert!(s.exceptions.disabled_apps.is_empty());
}

/// The load-bearing guard. Anything the user actually curated survives
/// — dropping one entry from the old default is enough to prove intent,
/// and wiping a list somebody wrote on purpose would be a worse bug
/// than the one this migration exists to fix.
#[test]
fn leaves_a_curated_skip_list_alone() {
    for curated in [
        // Shipped default minus one entry — the user took kitty out.
        LEGACY_DEFAULT_DISABLED_APPS
            .iter()
            .filter(|a| **a != "kitty")
            .map(|a| (*a).to_owned())
            .collect::<Vec<_>>(),
        // Shipped default plus one — they added their own.
        LEGACY_DEFAULT_DISABLED_APPS
            .iter()
            .map(|a| (*a).to_owned())
            .chain(["obs".to_owned()])
            .collect(),
        // Nothing like the default at all.
        vec!["Code.exe".to_owned()],
    ] {
        let mut s = Settings::default();
        s.exceptions.disabled_apps = curated.clone();

        assert!(
            !retire_default_skip_list(&mut s),
            "curated list must not be reported as migrated: {curated:?}"
        );
        assert_eq!(s.exceptions.disabled_apps, curated);
    }
}

/// Runs on every load, so it has to be a no-op the second time.
#[test]
fn retiring_the_skip_list_is_idempotent() {
    let mut s = Settings::default();
    s.exceptions.disabled_apps = LEGACY_DEFAULT_DISABLED_APPS
        .iter()
        .map(|a| (*a).to_owned())
        .collect();

    assert!(retire_default_skip_list(&mut s));
    assert!(!retire_default_skip_list(&mut s));
    assert!(!retire_default_skip_list(&mut Settings::default()));
}

// ─── AI plug-ins ──────────────────────────────────────────────────────

/// The `[[ai.plugins]]` table has to survive the trip from a config
/// file to the struct the AI factory reads. Before 0.8.0 there was no
/// such table at all and `[ai]` was two booleans nothing consulted.
#[test]
fn ai_plugins_parse_from_config() {
    let raw = r#"
schema_version = 1

[ai]
enabled = true
allow_remote = false

[[ai.plugins]]
type = "remote-llm"
id = "claude"
provider = "anthropic"
model = "claude-sonnet-4"
api_key_ref = "keyring:anthropic"

[[ai.plugins]]
type = "local-onnx"
id = "lid176"
model_path = "/models/lid.176.onnx"
"#;
    let s: Settings = toml::from_str(raw).expect("parse");
    assert!(s.ai.enabled);
    assert!(!s.ai.allow_remote);
    assert_eq!(s.ai.plugins.len(), 2);
    assert_eq!(s.ai.plugins[0].id, "claude");
    assert_eq!(
        s.ai.plugins[0].api_key_ref.as_deref(),
        Some("keyring:anthropic")
    );
    assert_eq!(s.ai.plugins[1].r#type, "local-onnx");
    assert!(s.ai.plugins[1].model_path.is_some());
}

/// The schema lives in `poltertype-types`, not in the optional
/// `poltertype-ai` crate, precisely so that a build *without* the `ai`
/// feature still reads a config file that configures it. A user who
/// switches between builds must not find their config rejected.
#[test]
fn a_config_with_ai_plugins_parses_in_a_build_without_the_ai_feature() {
    // This test crate never enables `ai`; parsing here IS the
    // assertion.
    let raw = r#"
schema_version = 1
[[ai.plugins]]
type = "remote-llm"
id = "x"
"#;
    let s: Settings = toml::from_str(raw).expect("must parse without the ai feature");
    assert_eq!(s.ai.plugins.len(), 1);
}

/// An entry naming a plug-in kind this build has never heard of must
/// reach the factory as data, not blow up the whole settings file on
/// the way. `type` is a plain string for exactly this reason.
#[test]
fn an_unknown_plugin_type_still_parses_and_is_left_to_the_factory() {
    let raw = r#"
schema_version = 1
[[ai.plugins]]
type = "some-future-backend"
id = "tomorrow"
"#;
    let s: Settings = toml::from_str(raw).expect("parse");
    assert_eq!(s.ai.plugins[0].r#type, "some-future-backend");
}

#[test]
fn no_ai_section_means_no_plugins() {
    let s: Settings = toml::from_str("schema_version = 1").expect("parse");
    assert!(s.ai.plugins.is_empty());
}
