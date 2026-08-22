//! Tunables for the Linux focus tracker.

use std::time::Duration;

/// How long a focus answer may be served from cache. The wordlist-
/// profile watcher asks every 250 ms and the engine asks on every word
/// boundary; one real IPC/X11 round-trip per 150 ms window keeps all
/// of them cheap while staying far below human app-switching cadence.
pub(crate) const FOCUS_CACHE_TTL: Duration = Duration::from_millis(150);

/// `ATSPI_COORD_TYPE_WINDOW` — extents relative to the object's
/// toplevel window. Chosen over `SCREEN` (0) deliberately: a
/// native-Wayland toolkit cannot know its global position, so its
/// SCREEN answers are anchored at the window's *initial* placement
/// and go stale the moment the compositor re-tiles it (observed live
/// with kate on Hyprland). Window-relative extents stay correct; the
/// consumer composes them with the compositor's live window rect.
pub(crate) const COORD_TYPE_WINDOW: u32 = 1;
