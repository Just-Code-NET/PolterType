//! Turning a layout into a flag, or admitting there isn't one.

use super::paint::{desaturate, edge};
use super::region::region_of;
use super::table;
use crate::icon_render::{H, PanelPolarity, TRANSPARENT, W, fill};
use poltertype_types::LayoutId;

/// The tray icon for `layout` drawn as its country's flag, or `None`
/// where there is no drawing for that country — which is not a
/// failure: the caller falls back to the lettered badge, and two
/// letters name the layout better than a flag nobody drew.
pub(crate) fn render(layout: &LayoutId, paused: bool, polarity: PanelPolarity) -> Option<Vec<u8>> {
    let region = region_of(layout)?;
    let mut buf = vec![0u8; (W * H * 4) as usize];
    fill(&mut buf, TRANSPARENT);
    if !table::draw(&mut buf, &region) {
        return None;
    }
    if paused {
        desaturate(&mut buf);
    }
    edge(&mut buf, polarity);
    Some(buf)
}
