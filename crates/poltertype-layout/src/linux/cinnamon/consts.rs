//! Where Cinnamon's input sources live, and what we call each route.

/// Cinnamon 6.6+, driven through `org.Cinnamon` on the session bus.
pub const DBUS_BACKEND_NAME: &str = "linux-cinnamon-dbus";

/// Cinnamon 6.4 and older, driven by locking XKB groups. Named apart
/// from the plain `linux-x11-xkb` fallback on purpose: in a bug report
/// the two look identical, and the difference between "we knew this is
/// how Cinnamon switches" and "no desktop backend answered, so we
/// guessed" is the first thing worth knowing.
pub const XKB_BACKEND_NAME: &str = "linux-cinnamon-xkb";

pub(crate) const BUS_NAME: &str = "org.Cinnamon";
pub(crate) const OBJECT_PATH: &str = "/org/Cinnamon";
pub(crate) const GET_INPUT_SOURCES: &str = "org.Cinnamon.GetInputSources";
pub(crate) const ACTIVATE_INPUT_SOURCE_INDEX: &str = "org.Cinnamon.ActivateInputSourceIndex";

/// `GetInputSources` returns `a(ssisssssssib)`, one tuple per source:
/// `(type, id, index, displayName, shortName, flagName, xkbId,
/// xkbLayout, variant, preferences, dupeId, isCurrent)`. We need three
/// of the twelve, and check the count so an interface change shows up
/// as "no sources" rather than as fields read off by one.
pub(crate) const SOURCE_FIELDS: usize = 12;
pub(crate) const FIELD_INDEX: usize = 2;
pub(crate) const FIELD_XKB_LAYOUT: usize = 7;
pub(crate) const FIELD_IS_CURRENT: usize = 11;
