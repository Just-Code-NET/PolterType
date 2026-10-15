//! The Accessibility permission prompt the event tap depends on.
//!
//! `CGEventTapCreate` fails *silently* without Accessibility rights — no
//! system dialog. `AXIsProcessTrustedWithOptions` with the prompt option
//! is the supported way to ask; without it a first-launch user gets a
//! dead tray icon and no explanation.

use core_foundation::base::TCFType;
use core_foundation::dictionary::CFDictionaryRef;
use core_foundation::string::CFStringRef;
use tracing::debug;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

pub(super) fn request_accessibility_prompt() {
    unsafe {
        let key =
            core_foundation::string::CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let value = core_foundation::boolean::CFBoolean::true_value();
        let options = core_foundation::dictionary::CFDictionary::from_CFType_pairs(&[(
            key.as_CFType(),
            value.as_CFType(),
        )]);
        let trusted = AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef());
        debug!(trusted, "AXIsProcessTrustedWithOptions(prompt) result");
    }
}
