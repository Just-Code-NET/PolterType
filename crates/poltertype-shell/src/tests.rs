//! Unit tests for the shell quirks.
//!
//! Each assertion is written twice, once per branch of the platform it
//! describes, so the test says what the *other* platforms do rather
//! than silently skipping them.

use crate::{acquire_instance_lock, key_glyph, key_name_with_glyph};

#[test]
fn the_lock_admits_one_holder_and_releases_on_drop() {
    // Exercised through the real primitive rather than a stand-in: what
    // broke before was the primitive's own behaviour, not our logic
    // about it.
    let id = format!("dev.opensource.poltertype-test-{}", std::process::id());
    let dir = std::env::temp_dir().join(&id);

    let first = acquire_instance_lock(&id, &dir).expect("first acquire failed");
    assert!(first.is_some(), "nothing else should hold a per-pid name");

    let second = acquire_instance_lock(&id, &dir).expect("second acquire errored");
    assert!(
        second.is_none(),
        "a second holder must be refused, not granted"
    );

    // Releasing must actually release — otherwise the first crash of
    // the day locks the user out until they reboot.
    drop(first);
    let third = acquire_instance_lock(&id, &dir).expect("third acquire failed");
    assert!(third.is_some(), "the lock did not come back after release");

    drop(third);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_locks_an_absolute_path_rather_than_a_bare_name() {
    // macOS `flock`s the id as a FILE, so a bare name lands in the
    // process working directory — which is `/` under Finder and
    // launchd, and read-only. v0.5.0 aborted at startup there.
    let dir = std::path::Path::new("/tmp/pt-test-cfg");
    let id = crate::instance::lock_id("dev.opensource.poltertype", dir);
    assert!(std::path::Path::new(&id).is_absolute(), "{id}");
    assert!(id.ends_with("dev.opensource.poltertype.lock"), "{id}");
}

#[test]
fn glyphs_are_a_macos_only_presentation() {
    if cfg!(target_os = "macos") {
        assert_eq!(key_glyph("Ctrl"), Some("⌃"));
        assert_eq!(key_glyph("meta"), Some("⌘"), "matching is case-insensitive");
        assert_eq!(key_glyph("Backspace"), Some("⌫"));
        assert_eq!(key_glyph("F9"), None, "unknown tokens keep their name");
    } else {
        assert_eq!(key_glyph("Ctrl"), None);
        assert_eq!(key_glyph("Meta"), None);
    }
}

#[test]
fn annotation_keeps_the_name_the_config_uses() {
    let annotated = key_name_with_glyph("Ctrl");
    if cfg!(target_os = "macos") {
        assert_eq!(
            annotated, "Ctrl (⌃)",
            "the glyph alone would leave the user guessing what to type in config.toml"
        );
    } else {
        assert_eq!(annotated, "Ctrl");
    }
    // `Space` maps to itself on macOS; annotating it would read
    // "Space (Space)".
    assert_eq!(key_name_with_glyph("Space"), "Space");
}

#[test]
fn a_linux_window_declares_the_app_id_and_others_declare_nothing() {
    // Empty is not a neutral value: it is passed on as an empty Wayland
    // `app_id` and an empty X11 `WM_CLASS`, leaving the window
    // belonging to no application at all.
    #[cfg(target_os = "linux")]
    {
        let ps = crate::window_platform_specific();
        assert_eq!(ps.application_id, crate::DESKTOP_ID);
        assert!(!ps.application_id.is_empty());
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Nothing to assert — the struct has no such field. What
        // matters is that the call compiles, so the binary needs no
        // `#[cfg]` of its own.
        let _ = crate::window_platform_specific();
    }
}

#[cfg(target_os = "linux")]
mod desktop {
    use std::path::{Path, PathBuf};

    use crate::DESKTOP_ID;
    use crate::desktop::{entry_body, exec_quote};

    /// The entry the AppImage and the AUR package install.
    fn packaged_entry() -> String {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../installers/linux/poltertype.desktop");
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
    }

    fn keys(body: &str) -> Vec<(String, String)> {
        body.lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
    }

    #[test]
    fn exec_quote_wraps_and_escapes() {
        assert_eq!(
            exec_quote(Path::new("/usr/bin/poltertype")),
            "\"/usr/bin/poltertype\""
        );
        assert_eq!(
            exec_quote(Path::new("/home/a b/poltertype")),
            "\"/home/a b/poltertype\"",
            "a space must survive inside the quotes, not split the value"
        );
        assert_eq!(exec_quote(Path::new(r"/tmp/a\b")), r#""/tmp/a\\b""#);
        assert_eq!(exec_quote(Path::new("/tmp/a$b")), r#""/tmp/a\$b""#);
        assert_eq!(exec_quote(Path::new("/tmp/a`b")), r#""/tmp/a\`b""#);
    }

    #[test]
    fn the_written_entry_agrees_with_the_packaged_one() {
        // Two files describe the same app to the same desktop: this one
        // and the AppImage's. `Exec` is the only key that legitimately
        // differs — a package can say `poltertype` because it put the
        // binary on `PATH`. Anything else drifting means the name in
        // the menu depends on how the user installed.
        let ours = keys(&entry_body(&PathBuf::from("/opt/poltertype")));
        for (key, value) in keys(&packaged_entry()) {
            if key == "Exec" {
                continue;
            }
            let mine = ours.iter().find(|(k, _)| *k == key);
            assert_eq!(
                mine.map(|(_, v)| v.as_str()),
                Some(value.as_str()),
                "key {key} disagrees with installers/linux/poltertype.desktop"
            );
        }
    }

    #[test]
    fn the_entry_names_the_icon_and_the_window_with_one_id() {
        let body = entry_body(&PathBuf::from("/home/a b/poltertype"));
        assert!(body.starts_with("[Desktop Entry]\n"));
        assert!(body.contains(&format!("\nIcon={DESKTOP_ID}\n")), "{body}");
        // X11 matches a window to its entry by `WM_CLASS`; some
        // desktops look at nothing else.
        assert!(
            body.contains(&format!("\nStartupWMClass={DESKTOP_ID}\n")),
            "{body}"
        );
        assert!(body.contains("\nExec=\"/home/a b/poltertype\"\n"), "{body}");
        assert!(body.ends_with('\n'), "desktop entries are line-based");
    }

    #[test]
    fn installing_writes_an_entry_and_every_icon_size_then_stops() {
        let root = std::env::temp_dir().join(format!("pt-desktop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let exec = PathBuf::from("/opt/poltertype/poltertype");

        assert!(
            crate::desktop::install_into(&root, &exec),
            "the first install must write"
        );

        let entry = root.join("applications").join("poltertype.desktop");
        assert_eq!(
            std::fs::read_to_string(&entry).expect("entry not written"),
            entry_body(&exec)
        );
        for &size in crate::desktop::HICOLOR_SIZES {
            let icon = root
                .join("icons/hicolor")
                .join(format!("{size}x{size}"))
                .join("apps")
                .join("poltertype.png");
            let bytes = std::fs::read(&icon).unwrap_or_else(|e| panic!("read {icon:?}: {e}"));
            assert_eq!(
                &bytes[..4],
                b"\x89PNG",
                "{icon:?} is not a PNG the desktop can read"
            );
        }

        // The second call runs on every subsequent launch and must be a
        // read and a compare, not five rasterisations of an icon
        // already on disk.
        assert!(
            !crate::desktop::install_into(&root, &exec),
            "an up-to-date install must not rewrite anything"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rewriting_the_entry_replaces_the_file_rather_than_its_contents() {
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!("pt-desktop-menu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let apps = root.join("applications");
        let entry = apps.join("poltertype.desktop");

        let old = PathBuf::from("/opt/PolterType/poltertype-0.25.3-x86_64.AppImage");
        assert!(crate::desktop::install_into(&root, &old));
        let before = std::fs::metadata(&entry).expect("entry not written").ino();

        // A menu cache decides it is fresh from the *directory's*
        // mtime, which rewriting a file inside it does not move: KDE
        // went on launching the old `Exec` (issue #48). A new inode is
        // that mtime moving — the name was unlinked and created again.
        let new = PathBuf::from("/opt/PolterType/poltertype-0.25.4-x86_64.AppImage");
        assert!(crate::desktop::install_into(&root, &new));
        let after = std::fs::metadata(&entry)
            .expect("entry not rewritten")
            .ino();
        assert_ne!(
            before, after,
            "the entry was rewritten in place, so no menu cache will notice it"
        );

        let left: Vec<_> = std::fs::read_dir(&apps)
            .expect("applications directory")
            .filter_map(Result::ok)
            .map(|e| e.file_name())
            .collect();
        assert_eq!(
            left,
            vec![std::ffi::OsString::from("poltertype.desktop")],
            "the file the rename went through must not be left behind"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_body_carries_the_version_that_makes_an_upgrade_refresh_it() {
        // Without this key an installed entry never changes, and a
        // redrawn mark would reach new users only.
        let body = entry_body(&PathBuf::from("/opt/poltertype"));
        assert!(
            body.contains(&format!(
                "\nX-PolterType-Version={}\n",
                env!("CARGO_PKG_VERSION")
            )),
            "{body}"
        );
    }
}
