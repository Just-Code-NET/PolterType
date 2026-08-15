//! Shapes the icon is composed of.

/// Axis-aligned rounded rectangle, in design units.
pub(crate) struct RoundRect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
    pub(crate) r: f32,
}
