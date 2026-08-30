//! What a macOS user has to grant, and whether they have.
//!
//! Two permissions people routinely confuse, because macOS shows them
//! in adjacent rows and neither name says "keyboard":
//! **Accessibility**, required to create the `CGEventTap` and post
//! corrected keys back, and **Input Monitoring**, required to
//! *receive* events from the tap. Grant one without the other and the
//! app looks half-alive.
//!
//! Both are checked without prompting: a guide that throws a system
//! dialog every time the user presses *Check again* is a guide they
//! close.

use core_foundation::base::TCFType;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::CFStringRef;

use super::consts::{ACCESSIBILITY_PANE_URL, INPUT_MONITORING_PANE_URL, NOTIFICATIONS_PANE_URL};
use super::enums::{Permission, StepAction, StepState};
use super::types::{SetupReport, SetupStep};

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    /// The no-argument form: reports trust and never prompts. The
    /// options form below could do the same with an empty dictionary,
    /// but an *empty* `CFDictionary::from_CFType_pairs(&[])` has no key
    /// or value type to infer and does not compile.
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

// `IOHIDCheckAccess` / `IOHIDRequestAccess`, macOS 10.15+. Plain `//`
// because rustdoc generates nothing for an extern block and `-D
// warnings` rejects a doc comment that documents nothing.
//
// The request type we care about is `kIOHIDRequestTypeListenEvent` (1)
// — Input Monitoring. The returned access type is 0 granted, 1 denied,
// 2 unknown, and the three are three different sentences: only
// "unknown" means the system has not decided, and so only there can a
// prompt still appear. "Denied" is a record, and a record is what
// makes the prompt silent.
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOHIDCheckAccess(request: u32) -> u32;
    fn IOHIDRequestAccess(request: u32) -> bool;
}

const K_IOHID_REQUEST_TYPE_LISTEN_EVENT: u32 = 1;
const K_IOHID_ACCESS_TYPE_GRANTED: u32 = 0;
const K_IOHID_ACCESS_TYPE_DENIED: u32 = 1;
const K_IOHID_ACCESS_TYPE_UNKNOWN: u32 = 2;

pub(super) fn probe(local_signing_identity: &str) -> SetupReport {
    let listen = input_monitoring_state();
    let accessibility = accessibility_state(listen);
    SetupReport {
        backend: Some("macos-cg-event-tap".to_owned()),
        steps: vec![
            SetupStep {
                title: "Grant Accessibility".to_owned(),
                detail: "System Settings → Privacy & Security → Accessibility, then switch \
                         PolterType on. This is what lets the app watch for a wrong-layout \
                         word and type the corrected one back. If PolterType is not in the \
                         list at all, press \u{201c}+\u{201d} under the list and add it from \
                         Applications."
                    .to_owned(),
                action: Some(step_action(accessibility, Permission::Accessibility)),
                state: accessibility,
            },
            SetupStep {
                title: "Grant Input Monitoring".to_owned(),
                // On current macOS the Accessibility grant covers this
                // too — measured on 26: one system prompt, and both
                // probes answer granted. Two Ask buttons for one
                // decision read as two decisions, so the button shows
                // only when Accessibility is done and this is somehow
                // still not — the older-macOS case this project also
                // supports.
                detail: if listen == StepState::Done {
                    "Granted — on current macOS this comes with the Accessibility grant above."
                        .to_owned()
                } else if accessibility == StepState::Done {
                    "Usually granted together with Accessibility, but this system still says \
                     no. Use the button; if PolterType is not in the list at all, press \
                     \u{201c}+\u{201d} under the list and add it from Applications."
                        .to_owned()
                } else {
                    "Covered by the Accessibility grant above on current macOS — do that one \
                     first and this row turns Ready by itself. A separate switch exists only \
                     on older systems."
                        .to_owned()
                },
                action: (accessibility == StepState::Done && listen != StepState::Done)
                    .then(|| step_action(listen, Permission::InputMonitoring)),
                state: listen,
            },
            SetupStep {
                title: "Open the right pane".to_owned(),
                detail: "Both switches live in Privacy & Security. If the buttons above don't \
                         bring the window forward, these open the panes directly."
                    .to_owned(),
                state: StepState::Unknown,
                action: Some(StepAction::Open(ACCESSIBILITY_PANE_URL.to_owned())),
            },
            signing_step(local_signing_identity),
            notifications_step(),
        ],
    }
}

/// A button that cannot work is worse than no button: once TCC holds a
/// denial, `AXIsProcessTrustedWithOptions` and `IOHIDRequestAccess`
/// both return quietly without raising a dialog. Send the user to the
/// pane instead.
fn step_action(state: StepState, permission: Permission) -> StepAction {
    if state == StepState::NeedsReset {
        StepAction::Open(settings_pane_url(permission).to_owned())
    } else {
        StepAction::RequestPermission(permission)
    }
}

/// `AXIsProcessTrusted` is a bare yes/no: unlike `IOHIDCheckAccess` it
/// cannot say whether a *record* exists, so nothing here can tell "the
/// user has never been asked" from "the user was asked and the answer
/// no longer matches this bundle". Input Monitoring can, and it
/// answers for both: TCC is asked for the two at the same moment, so a
/// recorded denial there means a record exists here too.
fn accessibility_state(listen: StepState) -> StepState {
    // Safety: a nullary C call that reads the trust database.
    if unsafe { AXIsProcessTrusted() } {
        StepState::Done
    } else if listen == StepState::NeedsReset {
        StepState::NeedsReset
    } else {
        StepState::Todo
    }
}

fn input_monitoring_state() -> StepState {
    // Safety: a plain C call taking an integer request type.
    match unsafe { IOHIDCheckAccess(K_IOHID_REQUEST_TYPE_LISTEN_EVENT) } {
        K_IOHID_ACCESS_TYPE_GRANTED => StepState::Done,
        // A decision is on record and it says no. `Todo` used to stand
        // here, which offered a prompt macOS will never show again.
        K_IOHID_ACCESS_TYPE_DENIED => StepState::NeedsReset,
        // Nothing decided yet — the prompt still works.
        K_IOHID_ACCESS_TYPE_UNKNOWN => StepState::Todo,
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

// ─── Local update signing ─────────────────────────────────────────────
//
// The permission loss this prevents: an ad-hoc bundle's TCC grants key
// on the hash of its bytes, so every self-update kills both grants
// (issue #42). Signed with a *stable* identity — any identity, it does
// not need Apple behind it — the grants key on certificate +
// identifier instead and survive every update. Measured on an M1 Pro:
// a bundle re-signed with a self-made keychain identity kept both
// grants across repeated rebuilds and one full staged update.

/// The step the Setup pane shows about it.
fn signing_step(identity: &str) -> SetupStep {
    let (state, detail) = if identity.is_empty() {
        (
            StepState::Todo,
            "Every update currently costs both permissions above, because macOS ties them \
             to the exact copy of the app. One click creates a private signing identity in \
             your keychain; every update is then re-signed with it and the permissions \
             survive. Nothing leaves your machine. macOS will show one password prompt — \
             \u{201c}codesign wants to access key\u{201d}: that is your new key being used for \
             the first time. Enter your login password and press \u{201c}Always Allow\u{201d}, \
             and it never appears again."
                .to_owned(),
        )
    } else {
        match identity_in_keychain(identity) {
            Some(true) => (
                StepState::Done,
                format!(
                    "Updates are re-signed with “{identity}” from your keychain, so the \
                     permissions above survive them."
                ),
            ),
            Some(false) => (
                StepState::Todo,
                format!(
                    "The config names “{identity}”, but no such identity is in your \
                     keychain — updates fall back to resetting the permissions. The button \
                     recreates it."
                ),
            ),
            None => (
                StepState::Unknown,
                "Could not read the keychain to check the signing identity.".to_owned(),
            ),
        }
    };
    SetupStep {
        title: "Keep permissions across updates".to_owned(),
        detail,
        action: (state != StepState::Done).then_some(StepAction::SetupLocalSigning),
        state,
    }
}

/// Does the login keychain hold a codesigning identity by this name?
/// `None` when `security` itself failed — an answer we must not guess.
fn identity_in_keychain(name: &str) -> Option<bool> {
    let out = std::process::Command::new("/usr/bin/security")
        .args(["find-identity", "-p", "codesigning"])
        .output()
        .ok()?;
    let listing = String::from_utf8_lossy(&out.stdout);
    Some(listing.contains(&format!("\"{name}\"")))
}

/// Create the identity, or adopt one already present under this name.
///
/// The key and certificate are generated with the system LibreSSL and
/// imported into the login keychain with `codesign` pre-authorised
/// (`-T`), so signing never has to prompt. The certificate is
/// self-signed and trusted by nobody — which is exactly enough: TCC
/// matching wants a *stable* certificate, not a trusted one, and
/// Gatekeeper never sees an app that was built or updated locally
/// without a quarantine flag.
pub(super) fn setup_local_signing(name: &str) -> Result<(), String> {
    if identity_in_keychain(name) == Some(true) {
        return Ok(()); // adopt, never duplicate
    }
    if name.contains(['"', '\'', '\\', '/']) {
        return Err("the identity name must not contain quotes or slashes".to_owned());
    }

    let dir = std::env::temp_dir().join(format!("poltertype-signing-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let key = dir.join("key.pem");
    let cert = dir.join("cert.pem");
    let run = |cmd: &str, args: &[&str]| -> Result<(), String> {
        let out = std::process::Command::new(cmd)
            .args(args)
            .output()
            .map_err(|e| format!("{cmd}: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "{cmd} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    };

    let result = (|| {
        run(
            "/usr/bin/openssl",
            &[
                "req", "-x509", "-newkey", "rsa:2048",
                "-keyout", key.to_str().ok_or("bad tmp path")?,
                "-out", cert.to_str().ok_or("bad tmp path")?,
                "-days", "3650", "-nodes",
                "-subj", &format!("/CN={name}"),
                "-addext", "keyUsage=critical,digitalSignature",
                "-addext", "extendedKeyUsage=critical,codeSigning",
                "-addext", "basicConstraints=critical,CA:FALSE",
            ],
        )?;
        // Two imports, PEM by PEM: LibreSSL's PKCS#12 defaults are not
        // accepted by `security import` (MAC verification failure), and
        // the -legacy escape hatch is an OpenSSL-3-ism it lacks.
        run(
            "/usr/bin/security",
            &[
                "import", key.to_str().ok_or("bad tmp path")?,
                "-T", "/usr/bin/codesign",
            ],
        )?;
        run(
            "/usr/bin/security",
            &["import", cert.to_str().ok_or("bad tmp path")?],
        )?;
        if identity_in_keychain(name) != Some(true) {
            return Err("imported, but the identity did not appear in the keychain".to_owned());
        }
        // Use the key once, right now, on a scratch copy of a system
        // binary. The keychain confirms first use of a fresh key with
        // its "codesign wants to access key" password prompt, and the
        // moment for that dialog is HERE — the user just pressed the
        // button and is told to expect it — not in the middle of the
        // first background update, where it reads as malware. After
        // "Always Allow" it never returns.
        let scratch = dir.join("scratch-sign");
        std::fs::copy("/usr/bin/true", &scratch).map_err(|e| e.to_string())?;
        run(
            "/usr/bin/codesign",
            &[
                "--force",
                "--sign",
                name,
                scratch.to_str().ok_or("bad tmp path")?,
            ],
        )?;
        Ok(())
    })();

    let _ = std::fs::remove_dir_all(&dir);
    result
}

// ─── Notifications ───────────────────────────────────────────────────
//
// Three independent layers decide whether a toast is seen, and every
// one of them fails silently: the app's own switch, the per-app
// "Allow" + alert style in System Settings, and Focus/Do Not Disturb
// over everything. The first is ours; the middle one macOS offers no
// public query for; the last is readable from the session's DND
// assertions file. So this step says what can be said: whether Focus
// is muting banners right now, and where the per-app switches live.

/// Is a Focus mode (Do Not Disturb) holding banners back right now?
/// `None` when the assertions file cannot be read — possible, it sits
/// behind TCC on some setups — in which case we say nothing rather
/// than guess.
fn focus_is_on() -> Option<bool> {
    let home = std::env::var_os("HOME")?;
    let path = std::path::Path::new(&home).join("Library/DoNotDisturb/DB/Assertions.json");
    let text = std::fs::read_to_string(path).ok()?;
    Some(text.contains("assertionDetailsIdentifier"))
}

/// The Setup step about it.
fn notifications_step() -> SetupStep {
    let (state, detail) = match focus_is_on() {
        Some(true) => (
            StepState::Todo,
            "A Focus mode (Do Not Disturb) is on right now: notifications go silently to \
             Notification Center and no banner appears. Turn it off in Control Centre (the \
             moon in the menu bar). Separately, the app must be allowed under System \
             Settings → Notifications, with an alert style other than “None”."
                .to_owned(),
        ),
        _ => (
            StepState::Unknown,
            "macOS offers no way to check this from here, so after the first notification \
             ever sent, look for PolterType under System Settings → Notifications: “Allow \
             notifications” on, and an alert style other than “None”. If banners still do \
             not appear, check Focus (Do Not Disturb) in Control Centre — it silences \
             banners while everything reads as enabled."
                .to_owned(),
        ),
    };
    SetupStep {
        title: "Let notifications through".to_owned(),
        detail,
        state,
        action: Some(StepAction::Open(NOTIFICATIONS_PANE_URL.to_owned())),
    }
}
