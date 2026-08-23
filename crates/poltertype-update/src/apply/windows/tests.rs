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
fn a_relaunch_is_the_first_thing_the_success_branch_does() {
    let s = body(true);

    assert!(matches!(
        (s.find(RELAUNCH), s.find("Remove-Item")),
        (Some(r), Some(c)) if r < c
    ));
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
