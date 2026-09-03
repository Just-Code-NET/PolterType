//! `sh -n`, shared by the two POSIX backends' tests.
//!
//! Parsing without executing is as far as a script that replaces the
//! user's installed application can be exercised here — but a syntax
//! error in one is not something to find out about on somebody's
//! machine, and until now nothing checked even that.

use std::io::Write;
use std::process::{Command, Stdio};

pub(crate) fn assert_sh_parses(body: &str) {
    let mut sh = Command::new("sh")
        .arg("-n")
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sh");
    sh.stdin
        .take()
        .expect("stdin")
        .write_all(body.as_bytes())
        .expect("write script");
    let out = sh.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "sh refused the script: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
