#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Duration;

use super::config_stamp;

/// The edit that started this is a hotkey rebind, and one chord often
/// weighs exactly as much as the next: `Ctrl+Shift+F9` → `Ctrl+Shift+F8`
/// leaves `config.toml` the same length it was. A stamp made of size
/// alone would report that nothing happened and leave the running app
/// on the old chord until a restart (issue #45).
#[test]
fn a_rewrite_of_the_same_length_is_still_a_change() {
    let dir = std::env::temp_dir().join(format!("poltertype-stamp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("config.toml");

    assert_eq!(config_stamp(&path), None, "a missing file has no stamp");

    std::fs::write(&path, "manual_switch_last = \"Ctrl+Shift+F9\"\n").expect("write");
    let first = config_stamp(&path).expect("stamped");

    // The half that carries a same-length edit is the mtime, and that
    // only moves if the clock has.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&path, "manual_switch_last = \"Ctrl+Shift+F8\"\n").expect("write");
    let second = config_stamp(&path).expect("stamped");

    assert_eq!(first.0, second.0, "the case this test exists for");
    assert_ne!(first, second, "and it still has to read as a change");

    std::fs::remove_dir_all(&dir).ok();
}
