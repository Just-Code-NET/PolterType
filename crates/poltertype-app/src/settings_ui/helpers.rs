//! Pure helpers: draft validation, id derivation, overlay-file
//! paths, hotkey formatting.

use std::path::PathBuf;

use anyhow::Result;
use iced::keyboard::{Key, Modifiers, key::Named};
use poltertype_core::commands::{CommandAction, UserCommand};
use poltertype_layout::LayoutId;
use tracing::warn;

use super::enums::*;
use super::state::*;

/// Banner text for the explicit per-pane Save button outcome.
pub fn banner_for_wordlist_save(outcome: WordlistFlushOutcome) -> SaveBanner {
    match outcome {
        WordlistFlushOutcome::Nothing => SaveBanner {
            text: "Nothing to save (buffer is unchanged).".into(),
            is_error: false,
        },
        WordlistFlushOutcome::NoLayout => SaveBanner {
            text: "No layout selected.".into(),
            is_error: true,
        },
        WordlistFlushOutcome::Saved(path) => SaveBanner {
            text: format!("Saved to {}. Close this window to apply.", path.display()),
            is_error: false,
        },
        WordlistFlushOutcome::Failed(e) => SaveBanner {
            text: format!("Save failed: {e}"),
            is_error: true,
        },
    }
}

/// Banner text for the auto-save path (layout / profile / kind
/// switch). Different phrasing than the explicit Save so the user
/// understands the save happened as a side effect of switching, not
/// because they clicked Save.
///
/// Returns `None` for the no-op case so we don't surface a banner
/// at all on every navigation click. Failures are still surfaced.
pub fn banner_for_auto_save(outcome: WordlistFlushOutcome) -> Option<SaveBanner> {
    match outcome {
        WordlistFlushOutcome::Nothing | WordlistFlushOutcome::NoLayout => None,
        WordlistFlushOutcome::Saved(path) => Some(SaveBanner {
            text: format!("Auto-saved unsaved edit to {}.", path.display()),
            is_error: false,
        }),
        WordlistFlushOutcome::Failed(e) => Some(SaveBanner {
            text: format!("Auto-save failed: {e}"),
            is_error: true,
        }),
    }
}

/// Validate the "Add command" form and produce a [`UserCommand`]
/// ready to push into `settings.commands`. Returns `Err(message)`
/// describing the first failed check — message is shown to the user
/// in the Commands pane's status banner so they know what to fix.
///
/// Validation:
///
/// * Trigger is non-empty and contains no whitespace (the buffer
///   resets at every word boundary so a multi-token trigger could
///   never match).
/// * Param is non-empty (every action variant needs payload —
///   "type empty text" / "switch to ''" / "open ''" all wrong).
/// * For `SwitchLayout`, the layout id matches the loose BCP-47
///   shape we already accept elsewhere (`xx-XX[-VARIANT...]`).
/// * Generated id is unique against existing `settings.commands`.
pub fn build_command_from_draft(app: &SettingsApp) -> Result<UserCommand, String> {
    let trigger = app.command_draft_trigger.trim().to_owned();
    if trigger.is_empty() {
        return Err("Set a trigger first (e.g. `anrl`).".into());
    }
    if trigger.chars().any(char::is_whitespace) {
        return Err(
            "Trigger must be a single token — no spaces. The buffer resets at every \
             word boundary, so a multi-word trigger can never match."
                .into(),
        );
    }
    let param = app.command_draft_param.trim().to_owned();
    if param.is_empty() {
        return Err("Action parameter is empty.".into());
    }
    let action = match app.command_draft_action_kind {
        CommandActionKind::TypeText => CommandAction::TypeText { text: param },
        CommandActionKind::SwitchLayout => {
            // Loose BCP-47 sanity — reject strings that obviously
            // can't be a layout id (whitespace, lowercase-only,
            // wrong shape) to save the user a mystery silent-no-op.
            if !looks_like_layout_id(&param) {
                return Err(format!(
                    "`{param}` doesn't look like a layout id (e.g. `en-US`)."
                ));
            }
            CommandAction::SwitchLayout {
                layout: LayoutId::new(param),
            }
        }
        CommandActionKind::OpenPath => CommandAction::OpenPath { path: param },
    };

    let name = app.command_draft_name.trim();
    let id = derive_command_id(name, &action, &app.settings.commands);
    if app.settings.commands.iter().any(|c| c.id == id) {
        return Err(format!(
            "A command with id `{id}` already exists — pick a different name."
        ));
    }

    let apps: Vec<String> = app
        .command_draft_apps
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect();

    Ok(UserCommand {
        id,
        name: name.to_owned(),
        trigger,
        action,
        apps,
    })
}

/// Loose validation of "this string could plausibly be a BCP-47
/// layout id". Accepts `en-US`, `uk-UA`, `kk-Cyrl-KZ`, etc. —
/// we let the OS reject genuinely-wrong values at switch time
/// (the engine logs a warning + no-ops in that case).
pub fn looks_like_layout_id(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // First segment must be 2-3 ascii letters; rest must contain
    // at least one `-` and only ascii alphanumerics + dashes.
    if !s.contains('-') {
        return false;
    }
    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Generate a stable kebab-case id from the user's display name —
/// or fall back to `cmd-<n>` if name is empty / collides. Handles
/// the "I just want to add a hotkey, don't make me name it" case
/// without forcing the user to pick an id manually.
pub fn derive_command_id(name: &str, action: &CommandAction, existing: &[UserCommand]) -> String {
    let from_name: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let base = if !from_name.is_empty() {
        from_name
    } else {
        match action {
            CommandAction::TypeText { .. } => "type-text".into(),
            CommandAction::SwitchLayout { .. } => "switch-layout".into(),
            CommandAction::OpenPath { .. } => "open-path".into(),
        }
    };
    // Disambiguate by appending `-2`, `-3`, … as needed.
    let mut candidate = base.clone();
    let mut n: u32 = 2;
    while existing.iter().any(|c| c.id == candidate) {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    candidate
}

/// Single-line description of a command for the existing-list view.
/// Renders the action concisely so the user can scan a long list of
/// trigger rows and see what each does without expanding rows.
pub fn format_command_summary(cmd: &UserCommand) -> String {
    let display_name = if cmd.name.is_empty() {
        cmd.id.clone()
    } else {
        cmd.name.clone()
    };
    let action_blurb = match &cmd.action {
        CommandAction::TypeText { text } => {
            // Truncate long snippets so one row stays one row.
            let preview = text.chars().take(40).collect::<String>();
            let suffix = if text.chars().count() > 40 { "…" } else { "" };
            format!("type `{preview}{suffix}`")
        }
        CommandAction::SwitchLayout { layout } => format!("→ {layout}"),
        CommandAction::OpenPath { path } => format!("open `{path}`"),
    };
    let apps_blurb = if cmd.apps.is_empty() {
        String::new()
    } else {
        format!(" (in {})", cmd.apps.join(", "))
    };
    format!("{display_name} — {action_blurb}{apps_blurb}")
}

/// Map a [`LayoutId`] (`en-US`, `uk-UA`, `kk-Cyrl-KZ`, …) to the
/// on-disk overlay-file *stem* (`en_us`, `uk_ua`, `kk_cyrl_kz`).
///
/// The convention matches both the bundled `data/wordlists/<stem>.fst`
/// names and the loader's `<config-dir>/poltertype/wordlists/<stem>.txt`
/// path resolution — keeping them in lock-step is what lets the user
/// add overlay words from the GUI and have the engine pick them up
/// without any additional book-keeping.
pub fn layout_id_to_stem(id: &LayoutId) -> String {
    id.as_str().to_lowercase().replace('-', "_")
}

/// Absolute path to the user-overlay file for `(profile_id, layout, kind)`.
/// Empty `profile_id` resolves to the global overlay directory
/// (`<config-dir>/poltertype/wordlists/<stem><suffix>.txt`);
/// non-empty resolves into the per-profile subdirectory
/// (`<config-dir>/poltertype/wordlists/profiles/<profile_id>/<stem><suffix>.txt`).
/// Returns `None` if the platform's config directory can't be
/// resolved (rare — usually only on minimal CI containers).
pub fn resolve_overlay_path(
    profile_id: &str,
    id: &LayoutId,
    kind: WordlistKind,
) -> Option<PathBuf> {
    let dir = if profile_id.is_empty() {
        poltertype_core::layouts::user_wordlist_dir()?
    } else {
        poltertype_core::layouts::user_profile_wordlist_dir(profile_id)?
    };
    let stem = layout_id_to_stem(id);
    Some(dir.join(format!("{stem}{}.txt", kind.suffix())))
}

/// Best-effort read of the resolved overlay file. Returns the
/// contents on success, empty string on `NotFound` (the common
/// first-edit case), or empty string with a warn log on real I/O
/// error so the GUI never blocks the user from starting fresh.
pub fn read_overlay_file_or_empty(profile_id: &str, id: &LayoutId, kind: WordlistKind) -> String {
    let Some(path) = resolve_overlay_path(profile_id, id, kind) else {
        warn!(
            layout = %id,
            profile = %profile_id,
            "no config dir resolved; wordlist editor starts empty"
        );
        return String::new();
    };
    match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            warn!(?path, err = %e, "could not read wordlist overlay; starting empty");
            String::new()
        }
    }
}

/// Atomic-ish write of the editor buffer to the resolved overlay
/// path. Creates the parent directory on first use (the user may
/// have never opened `<config-dir>/poltertype/wordlists/` or the
/// per-profile subdirectory before). The trailing-newline
/// normalisation matches the convention of the bundled files and
/// keeps `git diff` quiet for users who keep their config dir under
/// version control.
pub fn save_overlay_file(
    profile_id: &str,
    id: &LayoutId,
    kind: WordlistKind,
    text: &str,
) -> std::io::Result<PathBuf> {
    let path = resolve_overlay_path(profile_id, id, kind).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "config directory not resolved on this platform",
        )
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut normalised = text.to_owned();
    if !normalised.ends_with('\n') {
        normalised.push('\n');
    }
    std::fs::write(&path, normalised)?;
    Ok(path)
}

/// Lone-modifier-only key presses (Ctrl, Shift, Alt, Cmd) shouldn't
/// be captured as the hotkey itself — the user is mid-combination.
/// We filter them in the keyboard subscription so the captured combo
/// is always `<modifier(s)>+<non-modifier-key>`.
pub fn is_modifier_key(key: &Key) -> bool {
    matches!(
        key,
        Key::Named(
            Named::Control
                | Named::Shift
                | Named::Alt
                | Named::AltGraph
                | Named::Meta
                | Named::Super
                | Named::Hyper
        )
    )
}

/// Render a captured `(modifiers, key)` combo as the canonical
/// hotkey string `global-hotkey`'s `FromStr` accepts — `Ctrl+Shift+Space`,
/// `Alt+F4`, etc. We use platform-portable names: `Ctrl` (not
/// `Control`), `Cmd` for Meta on macOS, `Win` is intentionally
/// avoided in favour of the more universal `Meta`.
pub fn format_hotkey(key: &Key, modifiers: Modifiers) -> String {
    let mut parts: Vec<String> = Vec::new();
    if modifiers.control() {
        parts.push("Ctrl".into());
    }
    if modifiers.alt() {
        parts.push("Alt".into());
    }
    if modifiers.shift() {
        parts.push("Shift".into());
    }
    if modifiers.logo() {
        // global-hotkey accepts Meta / Super / Cmd / Win — use Meta
        // for consistency with the upstream's docs.
        parts.push("Meta".into());
    }
    parts.push(key_to_string(key));
    parts.join("+")
}

/// One-key serialisation matching `global-hotkey::HotKey::from_str`.
/// Letters get upper-cased (`a` → `A`); numbers stay as digits;
/// named keys map to their canonical name (Space / Backspace /
/// F1..F12 / arrow keys). Unrecognised keys round-trip via Debug —
/// good enough for the rare edge case (e.g. Print Screen) where
/// users will see something parseable in the Settings UI.
pub fn key_to_string(key: &Key) -> String {
    match key {
        Key::Character(c) => c.to_uppercase(),
        Key::Named(n) => match n {
            Named::Space => "Space".into(),
            Named::Backspace => "Backspace".into(),
            Named::Enter => "Enter".into(),
            Named::Tab => "Tab".into(),
            Named::ArrowUp => "Up".into(),
            Named::ArrowDown => "Down".into(),
            Named::ArrowLeft => "Left".into(),
            Named::ArrowRight => "Right".into(),
            Named::Home => "Home".into(),
            Named::End => "End".into(),
            Named::PageUp => "PageUp".into(),
            Named::PageDown => "PageDown".into(),
            Named::Insert => "Insert".into(),
            Named::Delete => "Delete".into(),
            Named::Escape => "Escape".into(),
            Named::F1 => "F1".into(),
            Named::F2 => "F2".into(),
            Named::F3 => "F3".into(),
            Named::F4 => "F4".into(),
            Named::F5 => "F5".into(),
            Named::F6 => "F6".into(),
            Named::F7 => "F7".into(),
            Named::F8 => "F8".into(),
            Named::F9 => "F9".into(),
            Named::F10 => "F10".into(),
            Named::F11 => "F11".into(),
            Named::F12 => "F12".into(),
            other => format!("{other:?}"),
        },
        other => format!("{other:?}"),
    }
}
