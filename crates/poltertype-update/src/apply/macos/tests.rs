//! The bundle swap script's shape, and that a shell will accept it.
//!
//! The only assertions this backend has: nobody in the project has a
//! Mac, so every other property of it is reasoned rather than run.

use super::*;

const BUNDLE: &str = "/Applications/PolterType.app";
const RELAUNCH: &str = "open '/Applications/PolterType.app' ||";

fn body(relaunch: bool) -> String {
    body_signed(relaunch, "")
}

fn body_signed(relaunch: bool, identity: &str) -> String {
    script_body(
        "0.99.0",
        Path::new("/Users/a b/Library/Application Support/poltertype/updates/p.dmg"),
        Path::new(BUNDLE),
        Path::new("/Users/a b/Library/Application Support/poltertype/updates"),
        4242,
        relaunch,
        identity,
    )
}

#[test]
fn the_script_says_it_started_before_anything_can_stop_it() {
    let s = body(true);
    let hello = s.find(super::HELLO).unwrap_or(usize::MAX);
    let mount = s.find("hdiutil attach").unwrap_or(0);

    assert!(hello < mount, "the script can fail before it says it began");
}

#[test]
fn the_installed_bundle_is_only_touched_once_a_replacement_exists() {
    let s = body(true);
    let copied = s.find("ditto").unwrap_or(usize::MAX);
    let removed = s
        .find("rm -rf '/Applications/PolterType.app'/Contents.old")
        .unwrap_or(0);

    assert!(copied < removed, "the app is deleted before it is replaced");
    // Moved aside, not deleted outright, so a swap that fails half way
    // can put back something that runs.
    assert!(s.contains(
        "mv '/Applications/PolterType.app'/Contents '/Applications/PolterType.app'/Contents.old"
    ));
    assert!(s.contains(
        "mv '/Applications/PolterType.app'/Contents.old '/Applications/PolterType.app'/Contents"
    ));
}

#[test]
fn a_dmg_that_will_not_mount_still_gives_the_user_their_app_back() {
    let s = body(true);
    let relaunch = s.find(RELAUNCH).unwrap_or(usize::MAX);
    let verdict = s.rfind(r#"if [ "$ok" = 1 ]; then"#).unwrap_or(0);

    assert!(relaunch < verdict, "a failed unpack leaves the app down");
    assert!(s.contains("install-failed.txt"));
}

#[test]
fn without_a_relaunch_only_the_cleanup_remains() {
    let s = body(false);

    assert!(!s.contains(RELAUNCH));
    assert!(s.contains("rm -rf"));
}

#[test]
#[cfg(unix)]
fn a_shell_accepts_the_script() {
    super::super::tests_util::assert_sh_parses(&body(true));
    super::super::tests_util::assert_sh_parses(&body(false));
}

/// Without a signing identity the installer drops the two stale TCC
/// records, and only after a successful swap: an update that never
/// replaced the bundle must leave the working grants alone.
#[test]
fn without_an_identity_the_stale_grants_are_dropped_after_the_swap() {
    let s = body(true);
    assert!(s.contains("tccutil reset Accessibility \"$BID\""));
    assert!(s.contains("tccutil reset ListenEvent \"$BID\""));
    assert!(!s.contains("codesign --force"), "nothing to sign with");
    let swap = s.find("ditto \"$NEW/Contents\"").expect("swap present");
    let reset = s.find("tccutil reset").expect("reset present");
    assert!(swap < reset, "grants die only after the bundle changed");
    let guard = s
        .find("if [ \"$ok\" = 1 ]; then\n\tBID=")
        .expect("ok guard");
    assert!(guard < reset, "reset sits inside the ok guard");
}

/// With an identity the swapped bundle is re-signed, and the reset runs
/// only when the outgoing bundle carried a different signature — the
/// one transition where the grants on file cannot match.
#[test]
fn with_an_identity_the_bundle_is_resigned_and_reset_is_conditional() {
    let s = body_signed(true, "PolterType Local Signing");
    assert!(s.contains("codesign --force --sign 'PolterType Local Signing'"));
    assert!(s.contains("Authority=PolterType Local Signing"));
    assert!(
        s.contains("if [ \"$SIGNED_SAME\" = 0 ]; then"),
        "reset must be gated on the signature transition"
    );
    let probe = s.find("SIGNED_SAME=0").expect("probe present");
    let swap = s.find("ditto \"$NEW/Contents\"").expect("swap present");
    assert!(probe < swap, "the old signature is read before the swap");
    // A failed codesign still clears the records — a bundle that
    // changed hash with no working signature must not keep dead grants.
    assert!(s.contains("else\n\t\ttccutil reset Accessibility"));
}

/// An identity with a quote in it cannot break out of the script.
#[test]
fn a_hostile_identity_name_stays_quoted() {
    let s = body_signed(true, "x' ; rm -rf / ; '");
    assert!(s.contains(r"'x'\'' ; rm -rf / ; '\'''"));
}
