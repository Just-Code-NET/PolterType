//! The installer script's shape. Asserted as text because the only
//! other way to find out is to install something.

use super::*;

const EXE: &str = r"C:\Users\a b\AppData\Local\PolterType\poltertype.exe";
const RELAUNCH: &str =
    r"Start-Process -FilePath 'C:\Users\a b\AppData\Local\PolterType\poltertype.exe'";

fn body(relaunch: bool) -> String {
    script_body(
        "0.99.0",
        Path::new(r"C:\Users\a b\AppData\Local\opensource\poltertype\data\updates\p.msi"),
        Path::new(EXE),
        Path::new(r"C:\Users\a b\AppData\Local\opensource\poltertype\data\updates"),
        4242,
        relaunch,
    )
}

#[test]
fn a_busy_installer_is_retried_rather_than_written_off() {
    let s = body(true);

    assert!(s.contains("$code = 1618"));
    assert!(s.contains("$i -lt 20 -and $code -eq 1618"));
    assert!(s.contains("Start-Sleep -Seconds 15"));
    // The success test must read the loop's result, not a handle from
    // whichever attempt happened to run last.
    assert!(s.contains("if ($code -eq 0 -or $code -eq 3010)"));
}

#[test]
fn the_script_waits_for_us_before_touching_the_msi() {
    let s = body(true);
    let wait = s.find("Wait-Process -Id 4242").unwrap_or(usize::MAX);
    let install = s.find("msiexec.exe").unwrap_or(0);

    assert!(wait < install, "the MSI runs before the app has exited");
}

#[test]
fn the_script_says_it_started_before_anything_can_stop_it() {
    let s = body(true);
    let hello = s
        .find("PolterType installer: started")
        .unwrap_or(usize::MAX);
    let wait = s.find("Wait-Process").unwrap_or(0);

    // The line that distinguishes "the installer refused the package"
    // from "the installer never ran", so it must precede every
    // statement that could fail.
    assert!(hello < wait, "the script can fail before it says it began");
}

#[test]
fn a_refused_install_still_gives_the_user_their_app_back() {
    let s = body(true);
    let relaunch = s.find(RELAUNCH).unwrap_or(usize::MAX);
    let branch = s.find("if ($code -eq 0").unwrap_or(0);

    // Outside the success branch entirely: an install the OS turned
    // down leaves the old binary in place and runnable, and a machine
    // with no PolterType on it is the one outcome worth ruling out.
    assert!(relaunch < branch, "a failed install leaves the app down");
}

#[test]
fn without_a_relaunch_only_the_cleanup_remains() {
    let s = body(false);

    assert!(!s.contains(RELAUNCH));
    assert!(s.contains("Remove-Item"));
}

#[test]
fn a_refused_install_leaves_its_exit_code_behind() {
    let s = body(true);

    assert!(s.contains("install-failed.txt"));
    assert!(s.contains("msiexec exit code: $code"));
    // In the else branch, so a successful install never writes it —
    // and never has to clean it up either.
    assert!(matches!(
        (s.find("} else {"), s.find("install-failed.txt")),
        (Some(e), Some(f)) if e < f
    ));
}

#[test]
fn the_installer_records_why_and_not_only_that() {
    let s = body(true);

    // An exit code names a failure; only msiexec's own verbose log
    // says what it tripped over.
    assert!(s.contains("'/l*v'"));
    assert!(s.contains("msiexec.log"));
}

#[test]
fn an_app_that_is_still_running_is_never_installed_over() {
    let s = body(true);
    let guard = s.find("Get-Process -Id 4242").unwrap_or(usize::MAX);
    let install = s.find("msiexec.exe").unwrap_or(0);

    // The caller now stays alive when the hand-off fails, so "the wait
    // timed out" no longer implies "the app is about to be gone" — and
    // an MSI aimed at a live keyboard hook is the one thing this whole
    // design exists to prevent.
    assert!(guard < install, "the MSI can run against a live app");
    assert!(s.contains("exit 1"));
}

#[test]
#[ignore = "writes the script out for an external PowerShell parse check"]
fn dump_for_syntax_check() {
    let out = std::env::var("POLTERTYPE_SCRIPT_DUMP").expect("POLTERTYPE_SCRIPT_DUMP");
    std::fs::write(out, body(true)).expect("write");
}
