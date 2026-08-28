//! Pure helpers: draft validation, id derivation, overlay-file
//! paths, hotkey formatting.

use std::path::PathBuf;

use anyhow::Result;
use iced::keyboard::{Key, Modifiers, key::Named, key::Physical};
use poltertype_core::commands::{CommandAction, UserCommand};
use poltertype_core::engine::{ModRole, ModSet};
use poltertype_layout::LayoutId;
use tracing::warn;

use super::consts::*;
use super::enums::*;
use super::state::*;

/// The window's own icon: title bar, Alt-Tab and the taskbar button.
///
/// Drawn here rather than loaded, from the same geometry the
/// executable's icon resource is built from — so the window and the
/// Start-menu entry cannot end up wearing different marks.
///
/// **Windows and X11 only, whatever it returns.** winit implements
/// `set_window_icon` as an empty function on Wayland, where the
/// protocol has no window icon to set: there the icon comes from the
/// `.desktop` entry named by the window's app id, which is what
/// `poltertype_shell::install_desktop_entry` is for. macOS has no
/// per-window icon either — its icon is the bundle's.
///
/// Best-effort: a window that opens with the wrong icon beats one that
/// refuses to open.
pub fn window_icon() -> Option<iced::window::Icon> {
    let px = WINDOW_ICON_PX;
    match iced::window::icon::from_rgba(poltertype_icon::rasterise(px), px, px) {
        Ok(icon) => Some(icon),
        Err(e) => {
            warn!(?e, "could not build the window icon; using the shell's");
            None
        }
    }
}

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

/// Banner text for the auto-save path (layout / profile / kind switch).
/// Phrased differently from the explicit Save so the user sees the save
/// happened as a side effect of switching. `None` for the no-op case,
/// so navigation clicks don't each raise a banner; failures still do.
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

/// Validate the "Add command" form and produce a [`UserCommand`] ready
/// to push into `settings.commands`. `Err(message)` describes the first
/// failed check and is shown in the Commands pane's status banner.
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
            // Reject what obviously can't be a layout id, to save the
            // user a mystery silent no-op at switch time.
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

/// Loose validation of "this string could plausibly be a BCP-47 layout
/// id" (`en-US`, `kk-Cyrl-KZ`, …). Genuinely-wrong values are left for
/// the OS to reject at switch time; the engine warns and no-ops.
pub fn looks_like_layout_id(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    if !s.contains('-') {
        return false;
    }
    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// A stable kebab-case id derived from the user's display name, or from
/// the action kind when the name is empty — so "I just want to add a
/// hotkey" never turns into "pick an id first".
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
            CommandAction::RunShell(_) => "run-shell".into(),
        }
    };
    let mut candidate = base.clone();
    let mut n: u32 = 2;
    while existing.iter().any(|c| c.id == candidate) {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    candidate
}

/// Single-line description of a command, so a long list of trigger rows
/// can be scanned without expanding anything.
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
        // ASCII arrow: the default UI font may lack U+2192 (renders
        // as tofu on a clean Linux install).
        CommandAction::SwitchLayout { layout } => format!("-> {layout}"),
        CommandAction::OpenPath { path } => format!("open `{path}`"),
        // Shown with its arguments: a reader of the list must be able
        // to see exactly what would run.
        CommandAction::RunShell(shell) => {
            let argv = std::iter::once(shell.program.as_str())
                .chain(shell.args.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ");
            let preview = argv.chars().take(40).collect::<String>();
            let suffix = if argv.chars().count() > 40 { "…" } else { "" };
            format!("run `{preview}{suffix}`")
        }
    };
    let apps_blurb = if cmd.apps.is_empty() {
        String::new()
    } else {
        format!(" (in {})", cmd.apps.join(", "))
    };
    format!("{display_name} — {action_blurb}{apps_blurb}")
}

/// Map a [`LayoutId`] (`en-US`, `kk-Cyrl-KZ`) to the on-disk overlay
/// file *stem* (`en_us`, `kk_cyrl_kz`).
///
/// The convention matches both the bundled `data/wordlists/<stem>.fst`
/// names and the loader's overlay path resolution — in lock-step, a
/// word added from the GUI is picked up with no extra book-keeping.
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

/// Write the editor buffer to the resolved overlay path, creating the
/// parent directory on first use. The trailing newline matches the
/// bundled files and keeps `git diff` quiet for users who version their
/// config directory.
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

/// Whether `[suggestions].accept_modifiers` actually arms the
/// keyboard-accept chord. Delegates to the engine's own
/// `AcceptModifiers::parse` so the pane's hint cannot contradict what
/// the engine does. Bare `Shift` fails on purpose — `Shift+1` is just
/// `!` on most layouts, and the pane must say so instead of looking
/// configured.
pub fn accept_modifiers_enable_keyboard(s: &str) -> bool {
    poltertype_core::engine::AcceptModifiers::parse(s).is_some()
}

/// Display form of a stored hotkey token for keycap chips. Config
/// keeps the portable names (`Ctrl`, `Alt`, `Meta`); where the
/// platform prints a glyph on the key instead, the chip shows the
/// glyph — that is what the user reads off their keyboard.
pub fn display_key_token(tok: &str) -> String {
    poltertype_shell::key_glyph(tok).map_or_else(|| tok.to_owned(), str::to_owned)
}

/// Join key names the way the platform writes them: the glyph where
/// there is one, the portable name otherwise. Used for prose that
/// only has to *identify* the keys ("at least one of …").
pub fn key_list(names: &[&str], sep: &str) -> String {
    names
        .iter()
        .map(|n| poltertype_shell::key_glyph(n).unwrap_or(n).to_owned())
        .collect::<Vec<_>>()
        .join(sep)
}

/// Join key names annotated with their glyphs (`Ctrl (⌃)`). Used for
/// prose the user has to act on — the name is what goes in
/// `config.toml`, so it has to stay visible even where the keyboard
/// says something else.
pub fn named_key_list(names: &[&str], sep: &str) -> String {
    names
        .iter()
        .map(|n| poltertype_shell::key_name_with_glyph(n))
        .collect::<Vec<_>>()
        .join(sep)
}

/// Render a captured `(modifiers, key)` combo as the canonical hotkey
/// string `global-hotkey`'s `FromStr` accepts — `Ctrl+Shift+Space`,
/// `Alt+F4` — using the portable names, `Ctrl` rather than `Control`.
pub fn format_hotkey(key: &Key, modifiers: Modifiers) -> String {
    // `Super` and `Cmd`, and nothing else: measured against
    // `global-hotkey` 0.6.4, whose parser refuses both `Meta` and `Win`
    // despite the name `Modifiers::META`. Writing `Meta` there produced
    // a string our own reader rejected, and a rejected string is
    // silently replaced by the default — a rebind that looked accepted
    // and did nothing.
    let mut combo = modifier_prefix(modifiers);
    combo.push_str(&key_to_string(key));
    combo
}

/// Which modifier a captured key stands for, or `None` for every
/// other key. `AltGraph` counts as Alt: it is the same physical key on
/// the layouts that have it.
pub fn mod_role_of(key: &Key) -> Option<ModRole> {
    Some(match key {
        Key::Named(Named::Control) => ModRole::Ctrl,
        Key::Named(Named::Shift) => ModRole::Shift,
        Key::Named(Named::Alt | Named::AltGraph) => ModRole::Alt,
        Key::Named(Named::Meta | Named::Super | Named::Hyper) => ModRole::Meta,
        _ => return None,
    })
}

/// iced's modifier state as the engine's set.
pub fn mods_from_iced(modifiers: Modifiers) -> ModSet {
    ModSet {
        ctrl: modifiers.control(),
        shift: modifiers.shift(),
        alt: modifiers.alt(),
        meta: modifiers.logo(),
    }
}

/// Render a modifier-only chord as the canonical hotkey string — the
/// same portable names [`format_hotkey`] uses, in the same order, with
/// the modifier named twice for a double tap.
pub fn format_mod_chord(mods: ModSet, double_tap: bool) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if mods.ctrl {
        parts.push("Ctrl");
    }
    if mods.alt {
        parts.push("Alt");
    }
    if mods.shift {
        parts.push("Shift");
    }
    if mods.meta {
        parts.push("Super");
    }
    if double_tap {
        parts.extend_from_within(..);
    }
    parts.join("+")
}

/// One modifier key event during a rebind. Returns the combination to
/// bind once the gesture is complete, or `None` while it still could
/// become something else.
///
/// The whole of the modifier-only capture (issue #32), kept out of
/// `update` so it can be driven straight from a test: the iced
/// subscription that feeds it cannot be.
pub fn mod_capture_step(
    cap: &mut ModCapture,
    role: ModRole,
    pressed: bool,
    held: ModSet,
) -> Option<String> {
    // `held` rather than our own count: a window that lost focus
    // mid-gesture never delivers the release, and a capture waiting
    // for it would take no further input.
    if pressed {
        cap.down = held.with(role);
        cap.peak = cap.peak.with(role);
        return None;
    }
    cap.down = held.without(role);
    if !cap.down.is_empty() {
        return None;
    }
    let peak = std::mem::take(&mut cap.peak);
    match peak.count() {
        // Two or more modifiers held together and let go with nothing
        // typed in between: a chord.
        2.. => Some(format_mod_chord(peak, false)),
        // One alone is only ever half a binding — the pane stays in
        // capture until its twin arrives.
        1 if cap.pending_tap == Some(peak) => {
            cap.pending_tap = None;
            Some(format_mod_chord(peak, true))
        }
        1 => {
            cap.pending_tap = Some(peak);
            None
        }
        _ => None,
    }
}

/// Whether this build can read a captured combo back.
///
/// A key is captured as the character it *produced*, so the pane can
/// offer a combination the reader refuses — and a refused binding is
/// silently replaced by the default, which is a rebind that looks
/// accepted and does something else.
pub fn is_usable_hotkey(combo: &str) -> bool {
    crate::hotkeys::parse_mod_chord(combo).is_some()
        || combo.parse::<global_hotkey::hotkey::HotKey>().is_ok()
}

/// The captured key written the way the *keyboard* is laid out rather
/// than the way the current layout renders it, when the rendering is
/// something the reader cannot take back.
///
/// The logical key is what a hotkey should normally be: the user reads
/// `Ctrl+Ж` off their own keycap. But a Cyrillic letter, or the `§` an
/// Apple ISO keyboard puts left of `Z`, has no name in the hotkey
/// parser — and a refused capture is a rebind that silently does
/// nothing. The physical code (`KeyA`, `Backquote`) always has one.
///
/// `None` when the physical key has no `Code` either, or when its name
/// is one the parser does not know — `IntlBackslash` is the ISO key the
/// reporter of issue #43 wanted, and `global-hotkey` 0.6.4 has no
/// spelling for it at all.
pub fn physical_hotkey(physical: Physical, modifiers: Modifiers) -> Option<String> {
    let Physical::Code(code) = physical else {
        return None;
    };
    let mut combo = modifier_prefix(modifiers);
    combo.push_str(&format!("{code:?}"));
    is_usable_hotkey(&combo).then_some(combo)
}

/// The modifier half of a hotkey string, in the canonical order, each
/// part followed by its `+`.
fn modifier_prefix(modifiers: Modifiers) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if modifiers.control() {
        parts.push("Ctrl");
    }
    if modifiers.alt() {
        parts.push("Alt");
    }
    if modifiers.shift() {
        parts.push("Shift");
    }
    if modifiers.logo() {
        parts.push("Super");
    }
    parts.iter().map(|p| format!("{p}+")).collect()
}

/// One-key serialisation matching `global-hotkey::HotKey::from_str`:
/// letters upper-cased, named keys under their canonical name.
/// Unrecognised keys round-trip via `Debug` — good enough for the rare
/// edge case (Print Screen and friends), where the user still sees
/// something readable in the Settings UI.
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

/// The first few lines of what a plug-in printed, for a status line: a
/// command that prints a paragraph would push the whole pane down the
/// page. The tail is not lost — it is in the report the same command
/// feeds.
pub fn first_lines(text: &str, n: usize) -> String {
    let kept: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .take(n)
        .collect();
    let total = text.lines().filter(|l| !l.trim().is_empty()).count();
    if total > n {
        format!("{} …", kept.join(" · "))
    } else {
        kept.join(" · ")
    }
}

#[cfg(test)]
mod status_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::first_lines;

    #[test]
    fn a_short_answer_is_shown_whole() {
        assert_eq!(
            first_lines("Ранкове: sent to Бронза in Element.\n", 3),
            "Ранкове: sent to Бронза in Element."
        );
    }

    #[test]
    fn a_long_one_is_cut_and_says_so() {
        let out = first_lines("a\nb\nc\nd\ne\n", 3);
        assert_eq!(out, "a · b · c …");
    }

    #[test]
    fn blank_lines_do_not_count_as_answers() {
        assert_eq!(first_lines("\n\nsent\n\n", 3), "sent");
    }
}
