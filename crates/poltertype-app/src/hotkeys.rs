//! Hotkey string parsing, scancode mapping, and which chord each
//! hotkey actually answers to here.

use crossbeam_channel::Sender;
use global_hotkey::GlobalHotKeyManager;
use global_hotkey::hotkey::{Code, HotKey, Modifiers as HkMods};
use poltertype_core::engine::{
    Binding, EngineCommand, KeystreamHotkeys, ModChord, ModRole, ModSet,
};
use poltertype_input::HotkeyEnvironment;
use tracing::{info, warn};

use crate::consts::{
    DEFAULT_PAUSE_TOGGLE, DEFAULT_SWITCH_LAST, MACOS_SAFE_PAUSE_TOGGLE, WAYLAND_SAFE_SWITCH_LAST,
};

/// Why the chord in force is not the one in `config.toml`.
///
/// A value rather than a sentence: the tray writes it to the log, the
/// Settings window renders it as translated prose, and neither has to
/// know how the other says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Substitution {
    /// The default reaches the focused app as well as us, and
    /// `Ctrl+Backspace` there deletes the very word we are correcting.
    DefaultIsDestructiveHere,
    /// The OS already owns the default chord.
    SystemOwnsDefault,
}

/// The chord a hotkey answers to on this machine.
///
/// Both substitutions apply **only while the user is still on the
/// default** — an explicit binding is always honoured — and neither is
/// written back to `config.toml`, so one config file keeps meaning the
/// same thing on every machine. That is also why this is resolved in
/// two places at once and must stay one function: the tray decides
/// what to listen for, the Settings window decides what to show, and
/// they disagreed for a whole release (issue #31).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EffectiveHotkey<'a> {
    pub(crate) chord: &'a str,
    pub(crate) substitution: Option<Substitution>,
}

pub(crate) fn effective_pause_toggle(
    configured: &str,
    env: HotkeyEnvironment,
) -> EffectiveHotkey<'_> {
    if env.system_owns_ctrl_shift_space && configured == DEFAULT_PAUSE_TOGGLE {
        return EffectiveHotkey {
            chord: MACOS_SAFE_PAUSE_TOGGLE,
            substitution: Some(Substitution::SystemOwnsDefault),
        };
    }
    EffectiveHotkey {
        chord: configured,
        substitution: None,
    }
}

pub(crate) fn effective_switch_last(
    configured: &str,
    env: HotkeyEnvironment,
) -> EffectiveHotkey<'_> {
    if env.observed_not_consumed && configured == DEFAULT_SWITCH_LAST {
        return EffectiveHotkey {
            chord: WAYLAND_SAFE_SWITCH_LAST,
            substitution: Some(Substitution::DefaultIsDestructiveHere),
        };
    }
    EffectiveHotkey {
        chord: configured,
        substitution: None,
    }
}

/// One hotkey as it is actually held here: an OS-level grab, or a
/// modifier-only gesture the key stream matches (issue #32).
#[derive(Debug, Clone, Copy)]
pub(crate) enum ActiveBinding {
    Key(HotKey),
    Mods(ModChord),
}

impl ActiveBinding {
    /// Whether an OS hotkey event belongs to this binding. Always false
    /// for a modifier-only chord: nothing registers it, so nothing can
    /// deliver an event for it.
    pub(crate) fn owns_event(self, id: u32) -> bool {
        matches!(self, Self::Key(hk) if hk.id() == id)
    }

    fn os_grab(self) -> Option<HotKey> {
        match self {
            Self::Key(hk) => Some(hk),
            Self::Mods(_) => None,
        }
    }
}

/// The two chords in force right now, and — through their ids — what
/// the event loop dispatches an OS hotkey event on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ActiveHotkeys {
    pub(crate) pause: ActiveBinding,
    pub(crate) switch_last: ActiveBinding,
}

/// Put the configured chords in force, replacing whatever was in force
/// before.
///
/// Called at startup **and again every time `config.toml` is re-read**.
/// It has to be both: the chords used to be resolved once before the
/// event loop and never again, so a hotkey changed in the Settings
/// window sat there doing nothing until the app was restarted — which
/// from the outside is indistinguishable from the setting being
/// ignored, and was reported as exactly that (issue #34).
pub(crate) fn apply_hotkeys(
    configured_pause: &str,
    configured_switch_last: &str,
    env: HotkeyEnvironment,
    use_keystream: bool,
    manager: Option<&GlobalHotKeyManager>,
    engine_tx: &Sender<EngineCommand>,
    previous: Option<ActiveHotkeys>,
) -> ActiveHotkeys {
    let pause = effective_pause_toggle(configured_pause, env);
    let switch = effective_switch_last(configured_switch_last, env);
    if pause.substitution.is_some() {
        info!(
            rebound_to = pause.chord,
            "macOS: default pause ({DEFAULT_PAUSE_TOGGLE}) is the system input-source shortcut; using a free chord"
        );
    }
    if switch.substitution.is_some() {
        info!(
            rebound_to = switch.chord,
            "Wayland: default switch-last ({DEFAULT_SWITCH_LAST}) is destructive in-app; using a safe key"
        );
    }
    let active = ActiveHotkeys {
        pause: parse_binding_or_default(pause.chord, DEFAULT_PAUSE_TOGGLE),
        switch_last: parse_binding_or_default(switch.chord, DEFAULT_SWITCH_LAST),
    };

    // A modifier-only chord is matched off the key stream on every
    // backend — there is no key code to register — while an ordinary
    // chord is matched there only where the OS grab is deaf. Never both
    // for one hotkey, so no double-fire.
    let keystream = |b: ActiveBinding, what: &str| match b {
        ActiveBinding::Mods(m) => Some(Binding::Mods(m)),
        ActiveBinding::Key(hk) if use_keystream => match chord_from_hotkey(&hk) {
            Some(c) => Some(Binding::Key(c)),
            None => {
                warn!(hotkey = ?hk, what, "hotkey key not mappable to a scancode; disabled");
                None
            }
        },
        ActiveBinding::Key(_) => None,
    };
    let chords = KeystreamHotkeys {
        pause: keystream(active.pause, "pause"),
        switch_last: keystream(active.switch_last, "switch-last"),
    };
    // Sent unconditionally, including when both are empty: this is also
    // what retires a modifier chord the user has just rebound away.
    let _ = engine_tx.send(EngineCommand::SetKeystreamHotkeys(chords));

    if let Some(manager) = manager {
        // Order matters: the old grab has to go before the new one is
        // taken, or rebinding A→B while B is still held by us fails
        // with "already registered" and leaves the user on A.
        if let Some(old) = previous {
            for hk in [old.pause, old.switch_last]
                .into_iter()
                .filter_map(ActiveBinding::os_grab)
            {
                if let Err(e) = manager.unregister(hk) {
                    warn!(?e, hotkey = ?hk, "could not release the previous hotkey");
                }
            }
        }
        for (hk, what) in [(active.pause, "pause"), (active.switch_last, "switch-last")]
            .into_iter()
            .filter_map(|(b, what)| b.os_grab().map(|hk| (hk, what)))
        {
            if let Err(e) = manager.register(hk) {
                warn!(?e, hotkey = ?hk, what, "could not register hotkey");
            }
        }
    }
    info!(
        pause = %pause.chord,
        switch_last = %switch.chord,
        pause_on_keystream = chords.pause.is_some(),
        switch_last_on_keystream = chords.switch_last.is_some(),
        "hotkeys in force"
    );
    active
}

/// Read a `[hotkeys]` string as whichever kind of binding it describes,
/// falling back to `default_str` on a bad value so a typo cannot
/// silently cost the user their hotkeys.
pub(crate) fn parse_binding_or_default(s: &str, default_str: &str) -> ActiveBinding {
    match parse_mod_chord(s) {
        Some(m) => ActiveBinding::Mods(m),
        None => ActiveBinding::Key(parse_hotkey_or_default(s, default_str)),
    }
}

/// Read a modifier-only chord — `Ctrl+Shift`, `Shift+Shift` — or `None`
/// for everything else, which is then an ordinary hotkey string.
///
/// Two shapes are accepted and no others: two or more *different*
/// modifiers held together, and the same modifier named twice, which
/// means two taps. A single lone modifier is refused on purpose — see
/// [`ModChord::double_tap`].
pub(crate) fn parse_mod_chord(s: &str) -> Option<ModChord> {
    let mut mods = ModSet::NONE;
    let mut count = 0usize;
    let mut repeated = false;
    for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        let role = match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => ModRole::Ctrl,
            "shift" => ModRole::Shift,
            "alt" | "option" => ModRole::Alt,
            "meta" | "super" | "cmd" | "command" | "win" => ModRole::Meta,
            _ => return None,
        };
        repeated |= mods.contains(role);
        mods = mods.with(role);
        count += 1;
    }
    match (count, mods.count(), repeated) {
        (2, 1, true) => Some(ModChord {
            mods,
            double_tap: true,
        }),
        (n, distinct, false) if n == distinct && distinct >= 2 => Some(ModChord {
            mods,
            double_tap: false,
        }),
        _ => None,
    }
}

/// Parse a `[hotkeys]` string, falling back to `default_str` on a bad
/// value so a typo cannot silently cost the user their hotkeys.
pub(crate) fn parse_hotkey_or_default(s: &str, default_str: &str) -> HotKey {
    match s.parse::<HotKey>() {
        Ok(h) => h,
        Err(e) => {
            warn!(
                ?e,
                raw = s,
                fallback = default_str,
                "could not parse hotkey; using fallback"
            );
            // The fallback is itself a parse; the hard-coded combo is
            // the last resort, so a real hotkey always comes back.
            default_str
                .parse::<HotKey>()
                .unwrap_or_else(|_| HotKey::new(Some(HkMods::CONTROL | HkMods::SHIFT), Code::Space))
        }
    }
}

/// Resolve a parsed [`HotKey`] into an engine-side [`Chord`] for the
/// keystream (Wayland) path. Returns `None` when the main key has no
/// entry in our SC Set-1 table — the chord is then simply unbound on
/// that backend (best-effort, per the Wayland support policy).
pub(crate) fn chord_from_hotkey(hk: &HotKey) -> Option<poltertype_core::engine::Chord> {
    Some(poltertype_core::engine::Chord {
        ctrl: hk.mods.contains(HkMods::CONTROL),
        shift: hk.mods.contains(HkMods::SHIFT),
        alt: hk.mods.contains(HkMods::ALT),
        // `HotKey::new` normalises META to SUPER, so testing META
        // alone was always false and a Super chord could never match
        // here — measured, not assumed.
        meta: hk.mods.intersects(HkMods::META | HkMods::SUPER),
        scancode: code_to_sc1(hk.key)?,
    })
}

/// W3C `Code` → Win SC Set-1 scancode. On Linux these coincide with the
/// evdev key codes the listener reports (see `evdev_to_sc1`), so the
/// same table serves matching against the live stream. Covers the keys
/// realistically used in a hotkey; anything else returns `None`.
///
/// The main-block punctuation is here because every one of it used to
/// be missing, which made the whole class unbindable on the key-stream
/// backends by construction (issue #43). `IntlBackslash` — the ISO key
/// left of `Z` — is deliberately absent: `global-hotkey`'s parser has
/// no name for it, so no config file can ever carry it here.
pub(crate) fn code_to_sc1(code: Code) -> Option<u32> {
    Some(match code {
        Code::Escape => 0x01,
        Code::Digit1 => 0x02,
        Code::Digit2 => 0x03,
        Code::Digit3 => 0x04,
        Code::Digit4 => 0x05,
        Code::Digit5 => 0x06,
        Code::Digit6 => 0x07,
        Code::Digit7 => 0x08,
        Code::Digit8 => 0x09,
        Code::Digit9 => 0x0A,
        Code::Digit0 => 0x0B,
        Code::Minus => 0x0C,
        Code::Equal => 0x0D,
        Code::Backspace => 0x0E,
        Code::Tab => 0x0F,
        Code::KeyQ => 0x10,
        Code::KeyW => 0x11,
        Code::KeyE => 0x12,
        Code::KeyR => 0x13,
        Code::KeyT => 0x14,
        Code::KeyY => 0x15,
        Code::KeyU => 0x16,
        Code::KeyI => 0x17,
        Code::KeyO => 0x18,
        Code::KeyP => 0x19,
        Code::Enter => 0x1C,
        Code::KeyA => 0x1E,
        Code::KeyS => 0x1F,
        Code::KeyD => 0x20,
        Code::KeyF => 0x21,
        Code::KeyG => 0x22,
        Code::KeyH => 0x23,
        Code::KeyJ => 0x24,
        Code::KeyK => 0x25,
        Code::KeyL => 0x26,
        Code::KeyZ => 0x2C,
        Code::KeyX => 0x2D,
        Code::KeyC => 0x2E,
        Code::KeyV => 0x2F,
        Code::KeyB => 0x30,
        Code::KeyN => 0x31,
        Code::KeyM => 0x32,
        Code::BracketLeft => 0x1A,
        Code::BracketRight => 0x1B,
        Code::Semicolon => 0x27,
        Code::Quote => 0x28,
        Code::Backquote => 0x29,
        Code::Backslash => 0x2B,
        Code::Comma => 0x33,
        Code::Period => 0x34,
        Code::Slash => 0x35,
        // Bindable, and it fires on a bare press — `Shift+CapsLock`
        // does not match a chord with no Shift in it, which is the
        // escape hatch that still latches the lock (issue #41).
        Code::CapsLock => 0x3A,
        Code::Space => 0x39,
        Code::F1 => 0x3B,
        Code::F2 => 0x3C,
        Code::F3 => 0x3D,
        Code::F4 => 0x3E,
        Code::F5 => 0x3F,
        Code::F6 => 0x40,
        Code::F7 => 0x41,
        Code::F8 => 0x42,
        Code::F9 => 0x43,
        Code::F10 => 0x44,
        Code::F11 => 0x57,
        Code::F12 => 0x58,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
