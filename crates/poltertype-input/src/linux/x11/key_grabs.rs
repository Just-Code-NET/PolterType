//! `KeyGrabs` — the suggestion tooltip's claimed keys, enforced on X11
//! with passive key grabs on the root window.
//!
//! What the server does with a grabbed key, measured on Cinnamon X11
//! (2026-10-15): the raw press reaches every listener as usual, and the
//! grabbing client alone then gets a core `KeyPress` for the same
//! keycode. From that moment the grab is active, and for as long as the
//! key is down its raw events — repeats and the release — reach nobody,
//! not even us; only the core ones do. So a raw press of a grabbed key
//! counts as claimed only if its core twin follows, and everything after
//! it is read from the core events. Never from our own idea of which
//! grabs are in force or which modifiers are held: a key stamped
//! claimed that the application did see would have the engine accept a
//! suggestion behind a newline the user just typed.

use tracing::debug;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{ConnectionExt as _, GrabMode, ModMask, Window};
use x11rb::rust_connection::RustConnection;

use super::codes::{claim_key_for_evdev, evdev_codes_for, evdev_to_x11, x11_to_evdev};
use super::events::key_event;
use crate::claims::KeyClaims;
use crate::{ClaimKey, ClaimSet, KeyDirection, KeyEvent, Modifiers};

/// Lock-key states a grab must cover: a passive grab matches its
/// modifier mask exactly, so without these the grab silently stops
/// working the moment Caps Lock or Num Lock is on.
fn lock_variants() -> [ModMask; 4] {
    [
        ModMask::from(0u16),
        ModMask::LOCK,
        ModMask::M2,
        ModMask::LOCK | ModMask::M2,
    ]
}

/// The core modifier mask a chord's modifiers are held as — Alt and
/// Super where every stock keymap puts them.
fn chord_mask(m: Modifiers) -> ModMask {
    let mut mask = ModMask::from(0u16);
    for (on, bit) in [
        (m.control, ModMask::CONTROL),
        (m.shift, ModMask::SHIFT),
        (m.alt, ModMask::M1),
        (m.meta, ModMask::M4),
    ] {
        if on {
            mask |= bit;
        }
    }
    mask
}

pub(crate) struct KeyGrabs {
    root: Window,
    /// Keys whose grabs the server accepted.
    held: ClaimSet,
    /// Keys whose press went to us and whose release has not come yet.
    /// The server keeps delivering them to us until that release,
    /// grab or no grab.
    down: ClaimSet,
}

impl KeyGrabs {
    pub(crate) fn new(root: Window) -> Self {
        Self {
            root,
            held: ClaimSet::default(),
            down: ClaimSet::default(),
        }
    }

    /// Is anything grabbed, or still down from a grab? Either way the
    /// listener must keep looking in often.
    pub(crate) fn active(&self) -> bool {
        !self.held.is_empty() || !self.down.is_empty()
    }

    /// Bring the grabs in line with what the engine claims now. A
    /// change regrabs the lot: the digits' modifiers can change with
    /// the keys, and it happens once per offer.
    pub(crate) fn sync(&mut self, conn: &RustConnection, claims: &KeyClaims) {
        let want = claims.armed();
        if want == self.held {
            return;
        }
        self.release_all(conn);
        let mut held = ClaimSet::default().with_digit_modifiers(want.digit_modifiers());
        for key in want.keys() {
            if self.grab(conn, key, want) {
                held = held.with(key);
            }
        }
        self.held = held;
        let _ = conn.flush();
        debug!(keys = held.keys().count(), "claim grabs in step");
    }

    /// Drop every grab — the claim ended or the listener is going away.
    /// A key down right now stays ours until its release: ungrabbing
    /// does not end an active grab.
    pub(crate) fn release_all(&mut self, conn: &RustConnection) {
        for key in self.held.keys() {
            self.ungrab(conn, key, self.held);
        }
        self.held = ClaimSet::default();
        let _ = conn.flush();
    }

    /// Should this raw event wait for its core twin? Only a press of a
    /// grabbed key that is not already ours.
    pub(crate) fn may_claim(&self, ev: &KeyEvent) -> bool {
        ev.direction == KeyDirection::Press
            && claim_key_for_evdev(ev.vk)
                .is_some_and(|k| self.held.contains(k) && !self.down.contains(k))
    }

    /// Is this raw event's key one whose press went to us? Its core
    /// events speak for it until the release, so the raw ones — where a
    /// server sends them at all — would only say it twice.
    pub(crate) fn owns(&self, ev: &KeyEvent) -> bool {
        claim_key_for_evdev(ev.vk).is_some_and(|k| self.down.contains(k))
    }

    /// Note what a delivered event did to the keys that are ours.
    pub(crate) fn record(&mut self, ev: &KeyEvent, claimed: bool) {
        let Some(key) = claim_key_for_evdev(ev.vk) else {
            return;
        };
        let without = ClaimSet::from_bits(self.down.bits() & !key.bit());
        self.down = match ev.direction {
            KeyDirection::Press if claimed => self.down.with(key),
            KeyDirection::Press => self.down,
            KeyDirection::Release => without,
        };
    }

    /// Grab every keycode of `key` under every lock variant. All or
    /// nothing: a key half-grabbed would reach the application under
    /// one lock state and not under another.
    fn grab(&self, conn: &RustConnection, key: ClaimKey, set: ClaimSet) -> bool {
        let chord = Self::chord_for(key, set);
        let mut cookies = Vec::new();
        for keycode in evdev_codes_for(key).into_iter().filter_map(evdev_to_x11) {
            for mods in lock_variants() {
                cookies.push(conn.grab_key(
                    false,
                    self.root,
                    mods | chord,
                    keycode,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                ));
            }
        }
        // Checked, not fire-and-forget: another client holding the same
        // grab (a desktop binding a bare F-key) answers `BadAccess`, and
        // that key is then not ours to claim.
        let ok = cookies
            .into_iter()
            .all(|c| c.is_ok_and(|c| c.check().is_ok()));
        if !ok {
            debug!(?key, "key grab refused — another client owns it");
            self.ungrab(conn, key, set);
        }
        ok
    }

    fn ungrab(&self, conn: &RustConnection, key: ClaimKey, set: ClaimSet) {
        let chord = Self::chord_for(key, set);
        for keycode in evdev_codes_for(key).into_iter().filter_map(evdev_to_x11) {
            for mods in lock_variants() {
                let _ = conn.ungrab_key(keycode, self.root, mods | chord);
            }
        }
    }

    fn chord_for(key: ClaimKey, set: ClaimSet) -> ModMask {
        match key {
            ClaimKey::Digit(_) => chord_mask(set.digit_modifiers()),
            _ => ModMask::from(0u16),
        }
    }
}

/// Is `ev` the core event our grab delivers for the raw event `raw`?
pub(crate) fn is_core_twin(raw: &KeyEvent, ev: &Event) -> bool {
    let (detail, direction) = match ev {
        Event::KeyPress(e) => (e.detail, KeyDirection::Press),
        Event::KeyRelease(e) => (e.detail, KeyDirection::Release),
        _ => return false,
    };
    direction == raw.direction && x11_to_evdev(u32::from(detail)) == Some(raw.vk)
}

/// A core key event as the engine's event — only our grabs deliver
/// these, so it is claimed by construction. `None` for anything else.
pub(crate) fn core_key_event(ev: &Event, mods: Modifiers) -> Option<KeyEvent> {
    let (detail, direction) = match ev {
        Event::KeyPress(e) => (e.detail, KeyDirection::Press),
        Event::KeyRelease(e) => (e.detail, KeyDirection::Release),
        _ => return None,
    };
    let evdev = x11_to_evdev(u32::from(detail))?;
    claim_key_for_evdev(evdev)?;
    Some(KeyEvent {
        claimed: true,
        ..key_event(evdev, direction, mods)
    })
}
