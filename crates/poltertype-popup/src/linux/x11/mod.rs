//! X11 backend: an override-redirect window on the root. The WM never
//! manages (or focuses) such windows, which gives us the "never steal
//! keyboard focus" guarantee for free — and needs no permissions.

mod enums;
mod popup;
mod state;

pub(crate) use popup::X11Popup;
