//! Unit tests for the pure parts of each backend.
//!
//! Only the string building is testable off-platform, and it is also
//! where the bugs live: an unescaped `&` in a home directory produces a
//! plist macOS silently refuses, and an unquoted space in `Exec=`
//! produces an entry the desktop silently ignores. Both fail quietly,
//! which is why they are worth pinning down.

#[cfg(target_os = "macos")]
mod macos {
    use std::path::Path;

    use crate::macos::{plist_body, xml_escape};

    #[test]
    fn xml_escape_covers_the_characters_that_break_a_text_node() {
        assert_eq!(xml_escape("Rock & Roll"), "Rock &amp; Roll");
        assert_eq!(xml_escape("a<b>c"), "a&lt;b&gt;c");
        assert_eq!(xml_escape("plain"), "plain");
    }

    #[test]
    fn plist_escapes_an_ampersand_in_the_program_path() {
        let body = plist_body(
            "dev.opensource.poltertype",
            Path::new("/Users/rock & roll/poltertype"),
        );
        assert!(
            body.contains("<string>/Users/rock &amp; roll/poltertype</string>"),
            "raw ampersand would make the plist unparseable: {body}"
        );
        assert!(!body.contains("roll & poltertype"));
    }

    #[test]
    fn plist_carries_the_label_and_runs_at_load() {
        let body = plist_body("dev.opensource.poltertype", Path::new("/tmp/poltertype"));
        assert!(body.contains("<string>dev.opensource.poltertype</string>"));
        assert!(body.contains("<key>RunAtLoad</key>"));
        assert!(body.starts_with("<?xml"));
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::Path;

    use crate::linux::{desktop_body, exec_quote, systemd_quote, unit_body};
    use crate::types::App;

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
    fn systemd_quote_escapes_what_systemd_reads_specially() {
        assert_eq!(
            systemd_quote(Path::new("/home/a b/poltertype")),
            "\"/home/a b/poltertype\"",
            "an unquoted space splits ExecStart into a command and an argument"
        );
        assert_eq!(systemd_quote(Path::new(r"/tmp/a\b")), r#""/tmp/a\\b""#);
        // A dollar starts a variable reference here; the escape is a
        // second dollar, not the backslash a desktop entry wants.
        assert_eq!(systemd_quote(Path::new("/tmp/a$b")), r#""/tmp/a$$b""#);
        // ...and a backtick is nothing special, so it stays as typed.
        assert_eq!(systemd_quote(Path::new("/tmp/a`b")), "\"/tmp/a`b\"");
    }

    #[test]
    fn unit_hangs_off_the_session_target() {
        let body = unit_body(
            App {
                id: "dev.opensource.poltertype",
                name: "PolterType",
                icon: "poltertype",
            },
            Path::new("/home/a b/poltertype"),
        );
        assert!(body.starts_with("[Unit]\n"));
        assert!(body.contains("\nDescription=PolterType\n"), "{body}");
        assert!(
            body.contains("\nExecStart=\"/home/a b/poltertype\"\n"),
            "{body}"
        );
        // All three, and all on the session target: `After` for
        // ordering, `PartOf` so logging out stops it, `WantedBy` so
        // `systemctl enable` has somewhere to put the symlink.
        assert!(
            body.contains("\nAfter=graphical-session.target\n"),
            "{body}"
        );
        assert!(
            body.contains("\nPartOf=graphical-session.target\n"),
            "{body}"
        );
        assert!(
            body.contains("\nWantedBy=graphical-session.target\n"),
            "{body}"
        );
        // A unit that restarts what the user quit, or reinstalls what
        // an update just replaced, is worse than no unit.
        assert!(!body.contains("Restart="), "{body}");
    }

    #[test]
    fn desktop_body_uses_the_human_name_and_a_quoted_exec() {
        let body = desktop_body(
            App {
                id: "dev.opensource.poltertype",
                name: "PolterType",
                icon: "poltertype",
            },
            Path::new("/home/a b/poltertype"),
        );
        assert!(body.starts_with("[Desktop Entry]\n"));
        assert!(body.contains("\nName=PolterType\n"), "{body}");
        assert!(body.contains("\nExec=\"/home/a b/poltertype\"\n"), "{body}");
        // The icon is keyed on the theme name, not the reverse-DNS id
        // the file itself is named after.
        assert!(body.contains("\nIcon=poltertype\n"), "{body}");
        assert!(body.contains("\nTerminal=false\n"));
        assert!(body.ends_with('\n'), "desktop entries are line-based");
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use std::path::Path;

    use crate::windows::run_value;

    #[test]
    fn run_value_quotes_the_path() {
        assert_eq!(
            run_value(Path::new(r"C:\Program Files\PolterType\poltertype.exe")),
            r#""C:\Program Files\PolterType\poltertype.exe""#,
            "an unquoted Program Files path is parsed as C:\\Program"
        );
    }
}
