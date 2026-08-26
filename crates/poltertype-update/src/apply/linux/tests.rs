//! The AppImage swap script's shape, and that a shell will accept it.

use super::*;

const TARGET: &str = "/home/a b/Apps/PolterType.AppImage";
const RELAUNCH: &str = "'/home/a b/Apps/PolterType.AppImage' &";

fn body(relaunch: bool) -> String {
    script_body(
        "0.99.0",
        Path::new("/home/a b/.local/share/poltertype/updates/new.AppImage"),
        Path::new(TARGET),
        Path::new("/home/a b/.local/share/poltertype/updates"),
        4242,
        relaunch,
    )
}

#[test]
fn the_script_says_it_started_before_anything_can_stop_it() {
    let s = body(true);
    let hello = s.find(super::HELLO).unwrap_or(usize::MAX);
    let wait = s.find("kill -0").unwrap_or(0);

    assert!(hello < wait, "the script can fail before it says it began");
}

#[test]
fn the_swap_waits_for_us_to_be_gone() {
    let s = body(true);
    let wait = s.find("while kill -0 4242").unwrap_or(usize::MAX);
    let swap = s.find("mv -f").unwrap_or(0);

    assert!(wait < swap, "the AppImage is replaced under a live app");
}

#[test]
fn a_failed_swap_still_gives_the_user_their_app_back() {
    let s = body(true);
    let relaunch = s.find(RELAUNCH).unwrap_or(usize::MAX);
    let verdict = s.find(r#"if [ "$ok" = 1 ]; then"#).unwrap_or(0);

    // `set -e` used to abort the whole script on a failed `mv`, taking
    // the relaunch with it: the old AppImage was still perfectly
    // runnable and the user was left with nothing running.
    assert!(relaunch < verdict, "a failed swap leaves the app down");
    assert!(s.contains("install-failed.txt"));
}

#[test]
fn without_a_relaunch_only_the_cleanup_remains() {
    let s = body(false);

    assert!(!s.contains(RELAUNCH));
    assert!(s.contains("rm -rf"));
}

/// `sh -n` parses without executing. The script manipulates the user's
/// installed app, so a syntax error in it is not something to discover
/// on someone's machine.
#[test]
#[cfg(unix)]
fn a_shell_accepts_the_script() {
    super::super::tests_util::assert_sh_parses(&body(true));
    super::super::tests_util::assert_sh_parses(&body(false));
}
