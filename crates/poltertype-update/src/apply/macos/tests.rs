//! The bundle swap script's shape, and that a shell will accept it.
//!
//! The only assertions this backend has: nobody in the project has a
//! Mac, so every other property of it is reasoned rather than run.

use super::*;

const BUNDLE: &str = "/Applications/PolterType.app";
const RELAUNCH: &str = "open '/Applications/PolterType.app' || true";

fn body(relaunch: bool) -> String {
    script_body(
        "0.99.0",
        Path::new("/Users/a b/Library/Application Support/poltertype/updates/p.dmg"),
        Path::new(BUNDLE),
        Path::new("/Users/a b/Library/Application Support/poltertype/updates"),
        4242,
        relaunch,
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
        .find("rm -rf '/Applications/PolterType.app'.old")
        .unwrap_or(0);

    assert!(copied < removed, "the app is deleted before it is replaced");
    // Moved aside, not deleted outright, so a swap that fails half way
    // can put back something that runs.
    assert!(s.contains("mv '/Applications/PolterType.app' '/Applications/PolterType.app'.old"));
    assert!(s.contains("mv '/Applications/PolterType.app'.old '/Applications/PolterType.app'"));
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
