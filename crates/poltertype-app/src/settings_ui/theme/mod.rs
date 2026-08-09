//! PolterType branding for the Settings window.
//!
//! The two palettes are a token-for-token port of the landing page's
//! design system (`poltertype-web/src/styles/global.css`), so the app
//! and the site read as one product.
//!
//! [`types`] holds the [`BrandPalette`] struct, [`consts`] the light
//! and dark values, [`themes`] the two custom [`iced::Theme`]s and the
//! mapping back to a palette, [`styles`] the widget style functions,
//! and [`ghost_mark`] the keycap-ghost logo as a `canvas` program.

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
