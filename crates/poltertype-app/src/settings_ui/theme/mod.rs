//! PolterType branding for the Settings window.
//!
//! The two palettes are a token-for-token port of the landing page's
//! design system (`poltertype-web/src/styles/global.css`) so the app
//! and poltertype.com read as one product: same indigo brand colour,
//! same ink/muted text pair, same "ecto" green for success and
//! "garble" pink for danger, same hairline colour for borders.
//!
//! Layout of the module:
//!
//! * [`types`] — the [`BrandPalette`] struct (one field per token).
//! * [`consts`] — the `LIGHT` / `DARK` palette values + mark colours.
//! * [`themes`] — building the two custom [`iced::Theme`]s and
//!   mapping a `&Theme` back to its brand palette.
//! * [`styles`] — widget style fns (sidebar, cards, buttons, chips).
//! * [`ghost_mark`] — the keycap-ghost logo as a `canvas` program.

mod consts;
mod ghost_mark;
mod styles;
mod themes;
mod types;

pub use consts::{FONT_BOLD, FONT_MONO};
pub use ghost_mark::GhostMark;
pub use styles::*;
pub use themes::{brand_palette, dark, light};
pub use types::BrandPalette;
