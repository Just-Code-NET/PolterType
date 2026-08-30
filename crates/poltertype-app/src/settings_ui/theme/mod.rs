//! PolterType branding for the Settings window.
//!
//! The two palettes are a token-for-token port of the landing page's
//! design system (`poltertype-web/src/styles/global.css`), so the app
//! and the site read as one product.
//!
//! [`types`] holds the [`BrandPalette`] struct, [`consts`] the light
//! and dark values, [`themes`] the two custom [`iced::Theme`]s and the
//! mapping back to a palette, [`styles`] the widget style functions,
//! and [`mark`] the keycap-ghost logo as an image.

mod consts;
mod mark;
mod styles;
mod themes;
mod types;

pub use consts::{FONT_MONO, font_bold, font_ui};
pub use mark::mark;
pub use styles::*;
pub use themes::{brand_palette, dark, light};
pub use types::BrandPalette;

#[cfg(test)]
mod tests;
