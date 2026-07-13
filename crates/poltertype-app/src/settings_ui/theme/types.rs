//! [`BrandPalette`] — one theme variant's worth of design tokens.

use iced::Color;

/// The full set of brand tokens for one theme variant, mirroring the
/// CSS custom properties on poltertype.com (`--c-bg`, `--c-surface`,
/// …). Everything the stock iced palette can't carry — surface,
/// hairline, muted text — lives here; widget style fns look tokens up
/// through [`super::brand_palette`].
pub struct BrandPalette {
    /// Window background (`--c-bg`).
    pub bg: Color,
    /// Raised surfaces: sidebar, cards, keycap chips (`--c-surface`).
    pub surface: Color,
    /// Primary text (`--c-ink`).
    pub ink: Color,
    /// Secondary / hint text (`--c-muted`).
    pub muted: Color,
    /// Brand indigo (`--c-brand`).
    pub brand: Color,
    /// Hover / highlight flavour of brand (`--c-brand-hi`).
    pub brand_hi: Color,
    /// Success green (`--c-ecto` — the "fixed word" flash colour).
    pub ecto: Color,
    /// Danger pink (`--c-garble` — the "wrong layout" colour).
    pub garble: Color,
    /// Hairlines and borders (`--c-line`).
    pub line: Color,
    /// Warning amber for "unsaved changes" / hotkey-capture states.
    /// No CSS counterpart — the site never needs a warning state.
    pub warn: Color,
    /// Bottom edge of keycap chips (`--keycap-side`) — rendered as a
    /// hard 2px shadow so chips get the site's "physical key" look.
    pub keycap_side: Color,
}
