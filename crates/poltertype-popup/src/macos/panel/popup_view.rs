//! The `NSView` subclass whose mouse callbacks forward into
//! [`super::callbacks`]. `define_class!` is order- and
//! macro-sensitive — nothing in this file may be rearranged.

use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadOnly, define_class, msg_send, rc::Retained};
use objc2_app_kit::{NSEvent, NSTrackingArea, NSTrackingAreaOptions, NSView};
use objc2_foundation::MainThreadMarker;

use super::callbacks::{click_at, hover_at};

#[derive(Default)]
struct PopupViewIvars;

define_class!(
    // Safety:
    // - `NSView` has no subclassing requirements relevant here.
    // - The class is main-thread-only, matching AppKit's rules, and
    //   is never subclassed further.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[name = "PolterTypePopupView"]
    #[ivars = PopupViewIvars]
    pub(super) struct PopupView;

    impl PopupView {
        // Top-down coordinates, so the shared hit-test works on the
        // renderer's row rectangles without a flip.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            click_at(point);
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            hover_at(Some(point));
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            hover_at(None);
        }

        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            // Safety: chaining to super is required by AppKit.
            let () = unsafe { msg_send![super(self), updateTrackingAreas] };
            for area in self.trackingAreas().iter() {
                self.removeTrackingArea(&area);
            }
            let area = unsafe {
                NSTrackingArea::initWithRect_options_owner_userInfo(
                    NSTrackingArea::alloc(),
                    self.bounds(),
                    NSTrackingAreaOptions::MouseEnteredAndExited
                        | NSTrackingAreaOptions::MouseMoved
                        | NSTrackingAreaOptions::InVisibleRect
                        // The panel is never key; without this the
                        // hover events would never fire.
                        | NSTrackingAreaOptions::ActiveAlways,
                    // Safety: same object pointer, retyped; the area
                    // retains its owner.
                    Some(&*std::ptr::from_ref(self).cast::<AnyObject>()),
                    None,
                )
            };
            self.addTrackingArea(&area);
        }
    }
);

impl PopupView {
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(PopupViewIvars);
        // Safety: standard NSView initialisation of our own subclass.
        unsafe { msg_send![super(this), init] }
    }
}
