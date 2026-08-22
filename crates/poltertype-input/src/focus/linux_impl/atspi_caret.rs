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

use crate::focus::CaretHint;

/// `ATSPI_COORD_TYPE_WINDOW` — extents relative to the object's
/// toplevel window. Chosen over `SCREEN` (0) deliberately: a
/// native-Wayland toolkit cannot know its global position, so its
/// SCREEN answers are anchored at the window's *initial* placement
/// and go stale the moment the compositor re-tiles it (observed live
/// with kate on Hyprland). Window-relative extents stay correct; the
/// consumer composes them with the compositor's live window rect.
const COORD_TYPE_WINDOW: u32 = 1;

/// Per-iterator signal queue. Caret events burst during fast typing
/// and we only ever serve the newest sample, so a small queue that
/// sheds backlog under pressure is exactly right.
const SIGNAL_QUEUE: usize = 32;

/// A `GetCharacterExtents` reply: x, y, width, height.
type Extents = (i32, i32, i32, i32);

/// One caret-position fix, in coordinates relative to the caret's
/// toplevel window (see [`COORD_TYPE_WINDOW`] for why not screen).
#[derive(Debug, Clone, Copy)]
pub(crate) struct CaretSample {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) height: u32,
    pub(crate) at: Instant,
}

impl CaretSample {
    /// `age` is computed at read time so the caller can judge staleness
    /// — an old sample usually means the focused app emits no a11y
    /// events at all.
    pub(crate) fn into_hint(self) -> CaretHint {
        CaretHint {
            x: self.x,
            y: self.y,
            height: self.height,
            age: self.at.elapsed(),
        }
    }
}

/// Why the watcher could not start. Every variant boils down to "no
/// usable a11y stack in this session" — headless CI, a11y disabled,
/// no registry daemon — which is why the caller treats construction
/// failure as a normal, log-once condition.
#[derive(Debug, thiserror::Error)]
pub(crate) enum AtspiCaretError {
    #[error("session bus unavailable: {0}")]
    SessionBus(#[source] zbus::Error),
    #[error("a11y bus address lookup failed: {0}")]
    A11yAddress(#[source] zbus::Error),
    #[error("a11y bus connection failed: {0}")]
    A11yConnect(#[source] zbus::Error),
    #[error("caret event registration failed: {0}")]
    Register(#[source] zbus::Error),
    #[error("caret signal subscription failed: {0}")]
    Subscribe(#[source] zbus::Error),
    #[error("watcher thread spawn failed: {0}")]
    Spawn(#[source] std::io::Error),
}

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
    for msg in messages {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                warn!(%e, "AT-SPI caret watcher: a11y bus error; caret anchoring stops");
                return;
            }
        };
        if let Some(sample) = sample_for_signal(conn, &msg) {
            *latest.lock() = Some(sample);
        }
    }
    warn!("AT-SPI caret watcher: a11y bus stream ended; caret anchoring stops");
}

/// One `TextCaretMoved` signal → a caret sample. The sender's unique
/// bus name plus the signal's object path identify the accessible
/// object; its `org.a11y.atspi.Text` interface answers the extents
/// queries.
fn sample_for_signal(conn: &Connection, msg: &Message) -> Option<CaretSample> {
    let header = msg.header();
    let sender = header.sender()?;
    let path = header.path()?;
    let offset = caret_offset_from_body(&msg.body())?;
    let (x, y, height) = resolve_caret_point(conn, sender.as_str(), path, offset)?;
    Some(CaretSample {
        x,
        y,
        height,
        at: Instant::now(),
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
    // Last resort: some clients emit the event before their own state
    // settles — ask the object where it now thinks the caret is and
    // try once more. Still degenerate → give up on this event.
    let caret = caret_offset_property(conn, sender, path)?;
    let rect = character_extents(conn, sender, path, caret)?;
    (!is_degenerate(rect)).then(|| anchor_from_rect(rect, false))
}

/// `GetCharacterExtents` on the signal's accessible, in [window
/// coordinates](COORD_TYPE_WINDOW). Failures (app exited, object
/// destroyed, interface not implemented) are normal churn — `None`,
/// logged at debug.
fn character_extents(
    conn: &Connection,
    sender: &str,
    path: &ObjectPath<'_>,
    offset: i32,
) -> Option<Extents> {
    let reply = conn
        .call_method(
            Some(sender),
            path.clone(),
            Some("org.a11y.atspi.Text"),
            "GetCharacterExtents",
            &(offset, COORD_TYPE_WINDOW),
        )
        .map_err(|e| debug!(%e, "AT-SPI caret watcher: GetCharacterExtents failed"))
        .ok()?;
    reply.body().deserialize::<Extents>().ok()
}

/// The object's current caret offset, via the `CaretOffset` property.
/// (libatspi's `atspi_text_get_caret_offset` maps to this property —
/// there is no `GetCaretOffset` *method* on the wire.)
fn caret_offset_property(conn: &Connection, sender: &str, path: &ObjectPath<'_>) -> Option<i32> {
    let reply = conn
        .call_method(
            Some(sender),
            path.clone(),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.a11y.atspi.Text", "CaretOffset"),
        )
        .map_err(|e| debug!(%e, "AT-SPI caret watcher: CaretOffset query failed"))
        .ok()?;
    match reply.body().deserialize::<Value<'_>>().ok()? {
        Value::I32(offset) => Some(offset),
        _ => None,
    }
}

/// A zero-area rect: no glyph at that offset. Typical for the
/// end-of-text caret position and for objects that cannot answer.
/// (A zero *width* alone is legitimate — zero-advance combining
/// marks — so only the fully collapsed rect counts as degenerate.)
pub(super) fn is_degenerate((_, _, width, height): Extents) -> bool {
    width == 0 && height == 0
}

/// The offset to retry with when the event offset has no glyph: the
/// character *before* the caret, if there is one.
pub(super) fn retry_offset(offset: i32) -> Option<i32> {
    if offset > 0 { Some(offset - 1) } else { None }
}

/// Collapse a glyph rect to the tooltip anchor point. `right_edge`
/// selects the rect's right edge — used when the rect belongs to the
/// character *before* the caret, whose trailing edge is where the
/// caret actually is.
pub(super) fn anchor_from_rect(
    (x, y, width, height): Extents,
    right_edge: bool,
) -> (i32, i32, u32) {
    let anchor_x = if right_edge {
        x.saturating_add(width)
    } else {
        x
    };
    // Toolkits answer sane heights, but the wire type is signed —
    // clamp instead of trusting.
    (anchor_x, y, u32::try_from(height.max(0)).unwrap_or(0))
}
