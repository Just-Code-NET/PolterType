//! AT-SPI2 caret watcher — the suggestion tooltip's only source of true
//! caret coordinates on Linux.
//!
//! No Wayland protocol or X11 property exposes where the text caret is;
//! the accessibility stack is the one API that does. A background
//! thread folds every `object:text-caret-moved` event into one mutex
//! slot holding the freshest [`CaretSample`]; a missing bus or registry
//! fails [`AtspiCaretWatcher::try_new`] and callers fall back to window
//! anchoring.
//!
//! Every application on the bus emits into that one slot, so a sample
//! is worth nothing without the identity of the window it came from
//! attached — see [`super::atspi_owner`]. An event whose owner cannot
//! be established is dropped rather than stored: the consumer would
//! otherwise compose one app's coordinates with another app's window.
//!
//! PRIVACY: this module must never read or log *text*. Offsets and
//! glyph rectangles only — no `GetText` / `GetTextAtOffset`, ever.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use tracing::{debug, warn};
use zbus::blocking::connection::Builder;
use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::{ObjectPath, Value};
use zbus::{MatchRule, Message, message};

use super::atspi_caret_geometry::{anchor_from_rect, is_caret_shaped, is_degenerate, retry_offset};
use super::atspi_caret_queries::{caret_offset_property, character_extents, object_extents};
use super::atspi_owner::OwnerLookup;
use super::enums::AtspiCaretError;
use super::types::CaretSample;

/// Per-iterator signal queue. Caret events burst during fast typing
/// and we only ever serve the newest sample, so a small queue that
/// sheds backlog under pressure is exactly right.
const SIGNAL_QUEUE: usize = 32;

/// A `GetCharacterExtents` reply: x, y, width, height.
pub(super) type Extents = (i32, i32, i32, i32);

/// Handle to the background caret watcher. Cheap to share (`Arc` it);
/// dropping the handle intentionally leaves the thread running — the
/// tracker holding it lives for the process, so a shutdown path would
/// buy nothing but complexity.
pub(crate) struct AtspiCaretWatcher {
    latest: Arc<Mutex<Option<CaretSample>>>,
}

impl AtspiCaretWatcher {
    /// Connect to the a11y bus, register interest in caret events and
    /// start the watcher thread. All bus round-trips happen here, on
    /// the caller's thread, so a dead a11y stack surfaces as an error
    /// instead of a silently idle thread.
    pub(crate) fn try_new() -> Result<Self, AtspiCaretError> {
        // The a11y bus is separate from the session bus; its address
        // is published by `org.a11y.Bus` *on* the session bus. (X11
        // also mirrors it in a root-window property, but the D-Bus
        // route works on X11 and Wayland alike.)
        let session = Connection::session().map_err(AtspiCaretError::SessionBus)?;
        let reply = session
            .call_method(
                Some("org.a11y.Bus"),
                "/org/a11y/bus",
                Some("org.a11y.Bus"),
                "GetAddress",
                &(),
            )
            .map_err(AtspiCaretError::A11yAddress)?;
        let address: String = reply
            .body()
            .deserialize()
            .map_err(AtspiCaretError::A11yAddress)?;
        let conn = Builder::address(address.as_str())
            .map_err(AtspiCaretError::A11yConnect)?
            .build()
            .map_err(AtspiCaretError::A11yConnect)?;

        // Registering is not bookkeeping: toolkits ask the registry
        // which events have listeners and only emit those. Without
        // this call most apps never send caret events at all.
        conn.call_method(
            Some("org.a11y.atspi.Registry"),
            "/org/a11y/atspi/registry",
            Some("org.a11y.atspi.Registry"),
            "RegisterEvent",
            &("object:text-caret-moved",),
        )
        .map_err(AtspiCaretError::Register)?;

        // Raise `org.a11y.Status.IsEnabled`, best-effort: toolkits keep
        // their accessibility bridge dormant while it is false, so on a
        // desktop without a screen reader nothing emits caret events
        // until an AT client sets it — and we are one. Never unset:
        // clearing it on exit would break a real AT that arrived while
        // we ran.
        //
        // Must come after the a11y-bus round-trips above — a Set fired
        // while `at-spi-bus-launcher` is still activating gets
        // overwritten by the launcher's own initial state.
        if let Err(e) = session.call_method(
            Some("org.a11y.Bus"),
            "/org/a11y/bus",
            Some("org.freedesktop.DBus.Properties"),
            "Set",
            &(
                "org.a11y.Status",
                "IsEnabled",
                zbus::zvariant::Value::from(true),
            ),
        ) {
            debug!(%e, "could not raise org.a11y.Status.IsEnabled; apps may stay silent");
        }

        let rule = MatchRule::builder()
            .msg_type(message::Type::Signal)
            .interface("org.a11y.atspi.Event.Object")
            .map_err(AtspiCaretError::Subscribe)?
            .member("TextCaretMoved")
            .map_err(AtspiCaretError::Subscribe)?
            .build();
        let messages = MessageIterator::for_match_rule(rule, &conn, Some(SIGNAL_QUEUE))
            .map_err(AtspiCaretError::Subscribe)?;

        let latest = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&latest);
        std::thread::Builder::new()
            .name("poltertype-atspi-caret".into())
            .spawn(move || watch(&conn, messages, &slot))
            .map_err(AtspiCaretError::Spawn)?;
        Ok(Self { latest })
    }

    /// Newest caret fix, if any event has arrived yet. One mutex lock
    /// plus a copy — safe to call on every tooltip show.
    pub(crate) fn latest(&self) -> Option<CaretSample> {
        *self.latest.lock()
    }
}

/// Blocking signal loop. Ends — with a single `warn` — when the bus
/// dies: the a11y stack restarting mid-session is rare enough that
/// reconnect logic isn't worth its failure modes yet, and the caller
/// degrades to window anchoring either way.
fn watch(conn: &Connection, messages: MessageIterator, latest: &Mutex<Option<CaretSample>>) {
    let mut owners = OwnerLookup::default();
    for msg in messages {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                warn!(%e, "AT-SPI caret watcher: a11y bus error; caret anchoring stops");
                return;
            }
        };
        if let Some(sample) = sample_for_signal(conn, &mut owners, &msg) {
            *latest.lock() = Some(sample);
        }
    }
    warn!("AT-SPI caret watcher: a11y bus stream ended; caret anchoring stops");
}

/// One `TextCaretMoved` signal → a caret sample. The sender's unique
/// bus name plus the signal's object path identify the accessible
/// object; its `org.a11y.atspi.Text` interface answers the extents
/// queries and `owners` answers whose window they describe.
fn sample_for_signal(
    conn: &Connection,
    owners: &mut OwnerLookup,
    msg: &Message,
) -> Option<CaretSample> {
    let header = msg.header();
    let sender = header.sender()?;
    let path = header.path()?;
    let offset = caret_offset_from_body(&msg.body())?;
    let (x, y, height) = resolve_caret_point(conn, sender.as_str(), path, offset)?;
    let owner = owners.resolve(conn, sender.as_str(), path)?;
    Some(CaretSample {
        x,
        y,
        height,
        at: Instant::now(),
        owner,
    })
}

/// Pull `detail1` — the caret offset — out of the event body. Modern
/// at-spi2-core marshals events as `(siiva{sv})`; older releases sent
/// `(siiv(so))` (the trailing argument was an application reference),
/// so both shapes are accepted.
fn caret_offset_from_body(body: &message::Body) -> Option<i32> {
    type Modern<'m> = (&'m str, i32, i32, Value<'m>, HashMap<&'m str, Value<'m>>);
    type Legacy<'m> = (&'m str, i32, i32, Value<'m>, (&'m str, ObjectPath<'m>));
    if let Ok((_, offset, ..)) = body.deserialize::<Modern<'_>>() {
        return Some(offset);
    }
    if let Ok((_, offset, ..)) = body.deserialize::<Legacy<'_>>() {
        return Some(offset);
    }
    // Never log the body itself — `any_data` may carry text.
    debug!("AT-SPI caret watcher: unrecognised TextCaretMoved body shape");
    None
}

/// Turn an event offset into a screen point, working around the
/// end-of-text quirk: the caret sitting *after* the last character
/// has no glyph of its own, so `GetCharacterExtents` returns a zero
/// rect there. The previous glyph's right edge is where that caret
/// actually blinks.
fn resolve_caret_point(
    conn: &Connection,
    sender: &str,
    path: &ObjectPath<'_>,
    offset: i32,
) -> Option<(i32, i32, u32)> {
    if let Some(rect) = character_extents(conn, sender, path, offset) {
        if !is_degenerate(rect) {
            return Some(anchor_from_rect(rect, false));
        }
    }
    if let Some(prev) = retry_offset(offset) {
        if let Some(rect) = character_extents(conn, sender, path, prev) {
            if !is_degenerate(rect) {
                return Some(anchor_from_rect(rect, true));
            }
        }
    }
    // Some clients emit the event before their own state settles — ask
    // the object where it now thinks the caret is and try once more.
    if let Some(caret) = caret_offset_property(conn, sender, path) {
        if let Some(rect) = character_extents(conn, sender, path, caret) {
            if !is_degenerate(rect) {
                return Some(anchor_from_rect(rect, false));
            }
        }
    }
    // Web-based editors draw their own text and answer no glyph
    // rectangle at all; the caret event then comes from the invisible
    // one-character input they park *at* the caret for IME, whose own
    // rectangle is the answer we wanted (measured in VS Code, whose
    // every offset returns a zero rect).
    let rect = object_extents(conn, sender, path)?;
    is_caret_shaped(rect).then(|| anchor_from_rect(rect, false))
}
