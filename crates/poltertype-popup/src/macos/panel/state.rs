//! The `PanelState` struct: the panel, its view and renderer.
//! Behaviour lives in the sibling files, one `impl` per concern.

use objc2::MainThreadOnly;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSPanel, NSStatusWindowLevel, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};

use crate::renderer::Renderer;

use super::popup_view::PopupView;
use super::types::Shown;

/// Panel + renderer + whatever is displayed. Main-thread only.
pub(super) struct PanelState {
    pub(super) panel: Retained<NSPanel>,
    pub(super) view: Retained<PopupView>,
    pub(super) renderer: Renderer,
    pub(super) shown: Option<Shown>,
}

impl PanelState {
    pub(super) fn create(mtm: MainThreadMarker) -> Option<Self> {
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)),
            NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            false,
        );
        panel.setLevel(NSStatusWindowLevel);
        // Visible on every space, unmoved by space switches, out of the
        // Cmd+Tab window cycle — the macOS spelling of WS_EX_TOOLWINDOW.
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(false);
        panel.setIgnoresMouseEvents(false);
        // We create and own it; AppKit must not free it under us.
        unsafe { panel.setReleasedWhenClosed(false) };

        let view = PopupView::new(mtm);
        panel.setContentView(Some(&view));

        Some(Self {
            panel,
            view,
            renderer: Renderer::new(),
            shown: None,
        })
    }
}
