//! What a macOS user has to grant, and whether they have.
//!
//! Two separate permissions that people routinely confuse, because
//! macOS shows them in adjacent rows of the same pane and neither
//! name says "keyboard":
//!
//! * **Accessibility** — required to create the `CGEventTap` we read
//!   keys with, and to post the corrected ones back.
//! * **Input Monitoring** — required to *receive* key events from the
//!   tap. Grant one without the other and the app looks half-alive.
//!
//! Both are checked without prompting. `AXIsProcessTrustedWithOptions`
//! shows the system dialog when asked to, and a guide that throws a
//! dialog every time the user presses *Check again* is a guide they
//! close.

use core_foundation::base::TCFType;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::CFStringRef;

use super::consts::{ACCESSIBILITY_PANE_URL, INPUT_MONITORING_PANE_URL};
use super::enums::{Permission, StepAction, StepState};
use super::types::{SetupReport, SetupStep};

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    /// The no-argument form: reports trust and never prompts. The
    /// options form below can do the same with an empty dictionary,
    /// but an *empty* `CFDictionary::from_CFType_pairs(&[])` has no
    /// key or value type to infer and does not compile — and reaching
    /// for turbofish to build a dictionary we do not want is worse
    /// than calling the function Apple provides for exactly this.
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

/// `IOHIDCheckAccess` / `IOHIDRequestAccess`, macOS 10.15+.
///
/// The request type we care about is `kIOHIDRequestTypeListenEvent`
/// (1) — "may this process observe keystrokes", i.e. Input Monitoring.
/// The returned `IOHIDAccessType` is 0 granted, 1 denied, 2 unknown,
/// and we keep that third value rather than folding it into "denied":
/// unknown means the system has not decided, which is a different
/// sentence to write on screen.
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOHIDCheckAccess(request: u32) -> u32;
    fn IOHIDRequestAccess(request: u32) -> bool;
}

const K_IOHID_REQUEST_TYPE_LISTEN_EVENT: u32 = 1;
const K_IOHID_ACCESS_TYPE_GRANTED: u32 = 0;
const K_IOHID_ACCESS_TYPE_DENIED: u32 = 1;

pub(super) fn probe() -> SetupReport {
    SetupReport {
        backend: Some("macos-cg-event-tap".to_owned()),
        steps: vec![
            SetupStep {
                title: "Grant Accessibility".to_owned(),
                detail: "System Settings → Privacy & Security → Accessibility, then switch \
                         PolterType on. This is what lets the app watch for a wrong-layout \
                         word and type the corrected one back."
                    .to_owned(),
                state: accessibility_state(),
                action: Some(StepAction::RequestPermission(Permission::Accessibility)),
            },
            SetupStep {
                title: "Grant Input Monitoring".to_owned(),
                detail: "System Settings → Privacy & Security → Input Monitoring, then switch \
                         PolterType on. Separate from Accessibility and easy to miss — with \
                         only one of the two granted the app starts but never sees a keystroke."
                    .to_owned(),
                state: input_monitoring_state(),
                action: Some(StepAction::RequestPermission(Permission::InputMonitoring)),
            },
            SetupStep {
                title: "Open the right pane".to_owned(),
                detail: "Both switches live in Privacy & Security. If the buttons above don't \
                         bring the window forward, these open the panes directly."
                    .to_owned(),
                state: StepState::Unknown,
                action: Some(StepAction::Open(ACCESSIBILITY_PANE_URL.to_owned())),
            },
        ],
    }
}

fn accessibility_state() -> StepState {
    // Safety: a nullary C call that reads the trust database. No
    // prompt — a guide that throws a system dialog every time the user
    // presses *Check again* is a guide they close.
    if unsafe { AXIsProcessTrusted() } {
        StepState::Done
    } else {
        StepState::Todo
    }
}

fn input_monitoring_state() -> StepState {
    // Safety: a plain C call taking an integer request type.
    match unsafe { IOHIDCheckAccess(K_IOHID_REQUEST_TYPE_LISTEN_EVENT) } {
        K_IOHID_ACCESS_TYPE_GRANTED => StepState::Done,
        K_IOHID_ACCESS_TYPE_DENIED => StepState::Todo,
        _ => StepState::Unknown,
    }
}

/// Ask macOS to show its own permission dialog. Returns whether the
/// permission is granted *after* the call — the Accessibility prompt
/// is asynchronous, so a `false` there means "the user has been asked",
/// not "the user said no".
pub(super) fn request(permission: Permission) -> bool {
    match permission {
        Permission::Accessibility => {
            // Safety: same call as above, with the prompt option set.
            unsafe {
                let key = core_foundation::string::CFString::wrap_under_get_rule(
                    kAXTrustedCheckOptionPrompt,
                );
                let value = core_foundation::boolean::CFBoolean::true_value();
                let options =
                    CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
                AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef())
            }
        }
        // Safety: a plain C call taking an integer request type.
        Permission::InputMonitoring => unsafe {
            IOHIDRequestAccess(K_IOHID_REQUEST_TYPE_LISTEN_EVENT)
        },
    }
}

/// The deep link for a permission, used when the system prompt has
/// already been answered once — macOS then never shows it again, and
/// the only way through is the Settings pane.
pub(super) fn settings_pane_url(permission: Permission) -> &'static str {
    match permission {
        Permission::Accessibility => ACCESSIBILITY_PANE_URL,
        Permission::InputMonitoring => INPUT_MONITORING_PANE_URL,
    }
}
