//! The relaunch script's shape, whose unit we think we are in, and —
//! on Linux — the swap itself against a real filesystem.

use super::*;

const TARGET: &str = "/home/a b/Apps/PolterType.AppImage";

fn body(unit: Option<&str>) -> String {
    script_body(4242, &launch_line(unit, Path::new(TARGET)))
}

#[test]
fn the_script_says_it_started_before_anything_can_stop_it() {
    let s = body(None);
    let hello = s.find(super::HELLO).unwrap_or(usize::MAX);
    let wait = s.find("kill -0").unwrap_or(0);

    assert!(hello < wait, "the script can fail before it says it began");
}

#[test]
fn nothing_is_started_until_the_old_process_is_gone() {
    let s = body(None);
    let wait = s.find("while kill -0 4242").unwrap_or(usize::MAX);
    let launch = s.find("exec ").unwrap_or(0);

    // A second instance started under the first would be refused by
    // the instance lock, which looks exactly like a relaunch that
    // never happened.
    assert!(wait < launch, "a second instance can start under the first");
}

#[test]
fn a_wedged_app_does_not_get_a_second_instance() {
    let s = body(None);

    assert!(s.contains("if [ $i -gt 1500 ]"));
    assert!(s.contains("not starting a second one"));
}

#[test]
fn the_script_no_longer_installs_anything() {
    let s = body(None);

    // The swap is done in-process before this script exists: a helper
    // in the app's cgroup is killed the instant the app exits, which
    // is the instant this script would have acted.
    assert!(!s.contains("mv -f"), "the swap is back in the script");
    assert!(!s.contains("chmod"), "the swap is back in the script");
}

#[test]
fn under_a_service_the_unit_is_started_before_the_file_is_tried() {
    let s = body(Some("dev.opensource.poltertype.service"));

    // Launching the AppImage directly would leave the unit dead with
    // the app running beside it, and the next login would start a
    // second copy for the instance lock to refuse. It is still the
    // fallback: a unit that cannot be started twice must not cost the
    // user their app.
    let unit = s
        .find("systemctl --user start 'dev.opensource.poltertype.service' && exit 0")
        .unwrap_or(usize::MAX);
    let file = s.find(&format!("exec '{TARGET}'")).unwrap_or(0);

    assert!(unit < file, "the file is launched before the unit is tried");
}

#[test]
fn a_unit_name_is_quoted_like_every_other_argument() {
    let s = body(Some("weird'name.service"));

    assert!(s.contains(r"'weird'\''name.service'"));
}

#[test]
fn only_a_service_is_treated_as_one() {
    let service = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/\
                   dev.opensource.poltertype.service\n";
    assert_eq!(
        service_from_cgroup(service),
        Some("dev.opensource.poltertype.service")
    );

    // A scope stops when its last process exits, not when a main
    // process does, so a helper left in one survives and needs none of
    // this.
    let scope = "0::/user.slice/user-1000.slice/session-23.scope\n";
    assert_eq!(service_from_cgroup(scope), None);

    // cgroup v1 lists numbered controllers and no `0::` line.
    let v1 = "12:pids:/user.slice\n11:memory:/user.slice\n";
    assert_eq!(service_from_cgroup(v1), None);
    assert_eq!(service_from_cgroup(""), None);
}

/// `sh -n` parses without executing. The script restarts the user's
/// desktop application, so a syntax error in it is not something to
/// discover on someone's machine.
#[test]
#[cfg(unix)]
fn a_shell_accepts_the_script() {
    super::super::tests_util::assert_sh_parses(&body(None));
    super::super::tests_util::assert_sh_parses(&body(Some("poltertype.service")));
}

#[cfg(target_os = "linux")]
#[test]
fn the_swap_replaces_the_target_and_leaves_it_runnable() {
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!("poltertype-swap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let artifact = dir.join("poltertype-9.9.9-x86_64.AppImage");
    let target = dir.join("PolterType.AppImage");
    std::fs::write(&artifact, b"new").unwrap();
    std::fs::write(&target, b"old").unwrap();
    // What the download leaves behind.
    std::fs::set_permissions(&artifact, std::fs::Permissions::from_mode(0o644)).unwrap();

    swap_in_place(&artifact, &target).unwrap();

    assert_eq!(std::fs::read(&target).unwrap(), b"new");
    assert!(!artifact.exists(), "the staged artifact outlived the swap");
    let mode = std::fs::metadata(&target).unwrap().permissions().mode();
    assert_eq!(
        mode & 0o111,
        0o111,
        "the installed AppImage is not executable"
    );

    std::fs::remove_dir_all(&dir).ok();
}
