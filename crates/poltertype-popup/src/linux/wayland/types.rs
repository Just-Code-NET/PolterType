//! Plain data used by [`super::state`] to place a popup on an output.

use wayland_client::protocol::wl_output;

/// An output the tooltip can be placed on, reduced to what placement
/// needs: the handle to pin the layer surface to, and its rectangle in
/// the compositor's global logical space.
pub(super) struct TargetOutput {
    pub(super) output: wl_output::WlOutput,
    pub(super) origin: (i32, i32),
    pub(super) size: (i32, i32),
    pub(super) scale: i32,
}
