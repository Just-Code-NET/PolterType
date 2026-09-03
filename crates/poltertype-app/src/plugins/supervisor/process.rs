//! Spawning a plug-in process, and waiting on one with a deadline.
//!
//! Shared by the service half ([`super::lifecycle`]) and the one-shot
//! command half ([`super::commands`]): both eventually spawn `ext.exe`,
//! and the commands that need an answer wait for it without being taken
//! hostage by a plug-in that never responds.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use poltertype_core::plugins::DiscoveredExtension;
use poltertype_core::settings::SettingsStore;
use tracing::warn;

use super::consts::{LOG_LINE_CHARS, LOG_TAIL_BYTES};

pub(super) fn spawn(
    exe: &PathBuf,
    args: &[String],
    dir: &PathBuf,
    log: Option<&(PathBuf, std::fs::File)>,
) -> std::io::Result<Child> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        // The plug-in's own directory, so a relative path in its
        // config means what its author expected.
        .current_dir(dir)
        // The manifest is translated from its own catalog, but what a
        // plug-in *prints* — report text, the rows of a list — only it
        // can translate, and only if it is told which language to use.
        .env(
            poltertype_core::i18n::LOCALE_ENV,
            poltertype_core::i18n::active_locale(),
        )
        .stdin(Stdio::null());
    // Both streams to the same file, in the order the plug-in wrote them.
    // Without a log file we inherit, which is right when there *is* a
    // terminal.
    if let Some((_, file)) = log {
        match (file.try_clone(), file.try_clone()) {
            (Ok(out), Ok(err)) => {
                cmd.stdout(Stdio::from(out)).stderr(Stdio::from(err));
            }
            _ => warn!("cannot hand the plug-in its log file; letting it inherit"),
        }
    }
    // A tray app owns no console, so a console child would be handed a
    // window of its own. See `poltertype_shell::configure_child`.
    poltertype_shell::configure_child(&mut cmd);
    cmd.spawn()
}

/// Run `args` against the plug-in and collect stdout, or give up.
/// Shared by the state read and the report read, so there is one place
/// that knows how to wait for a plug-in without being taken hostage.
pub(super) fn capture_output(
    ext: &DiscoveredExtension,
    args: &[String],
    timeout: Duration,
    what: &str,
) -> Result<String, String> {
    use std::io::Read as _;

    let mut cmd = Command::new(&ext.exe);
    cmd.args(args)
        .current_dir(&ext.dir)
        .env(
            poltertype_core::i18n::LOCALE_ENV,
            poltertype_core::i18n::active_locale(),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        // Drained but never *shown* on success: a program says why it
        // failed on stderr, and "exited with status 1" tells the user
        // nothing they can act on.
        .stderr(Stdio::piped());
    // The state read runs every time the tray menu is drawn, so a
    // console window here would flash on every click, not once at
    // startup.
    poltertype_shell::configure_child(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;

    // Each on its own thread: a plug-in printing more than a pipe
    // buffer would block forever if we polled without reading, and two
    // pipes can fill in either order.
    let mut out_pipe = child.stdout.take().ok_or("no stdout")?;
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = out_pipe.read_to_string(&mut buf);
        buf
    });
    let mut err_pipe = child.stderr.take().ok_or("no stderr")?;
    let err_reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = err_pipe.read_to_string(&mut buf);
        buf
    });

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = reader.join().unwrap_or_default();
                let err = err_reader.join().unwrap_or_default();
                return if status.success() {
                    Ok(out)
                } else {
                    // The plug-in's own sentence, unprefixed: the caller
                    // knows what it ran. The prefix stays only where there
                    // are no words to quote.
                    Err(match last_words(&err) {
                        Some(why) => why,
                        None => format!("{what} exited {status}"),
                    })
                };
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{what} did not answer within {}ms",
                    timeout.as_millis()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// The last thing a failing plug-in said, cleaned up enough to put in a
/// sentence.
///
/// The *last* line rather than the first: a program that logs while it
/// works ends with the thing that stopped it. Bounded, because this goes
/// into a status line and a plug-in's stack trace would take the window.
pub(super) fn last_words(stderr: &str) -> Option<String> {
    let line = stderr.lines().map(str::trim).rfind(|l| !l.is_empty())?;
    let line = line.strip_prefix("Error: ").unwrap_or(line);
    let mut trimmed: String = line.chars().take(160).collect();
    if line.chars().count() > 160 {
        trimmed.push('…');
    }
    Some(trimmed)
}

/// Open this service's log, truncating whatever the last run left, so
/// the file always answers "what happened this run" and cannot grow
/// without bound. Best-effort: a plug-in must still start where the log
/// directory cannot be created.
pub(super) fn service_log(id: &str) -> Option<(PathBuf, std::fs::File)> {
    let dir = SettingsStore::log_dir().ok()?;
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!(id = %id, "cannot create the log directory for the plug-in: {e}");
        return None;
    }
    // The id comes from a directory name on disk and could contain a
    // separator; keep it to one file in one place.
    let path = dir.join(format!("plugin-{}.log", id.replace(['/', '\\'], "-")));
    match std::fs::File::create(&path) {
        Ok(file) => Some((path, file)),
        Err(e) => {
            warn!(id = %id, path = ?path, "cannot open a log for the plug-in: {e}");
            None
        }
    }
}

/// The last non-blank line a plug-in wrote, trimmed to fit a
/// notification. Reads the end of the file only — a plug-in that logged
/// all day must not be pulled into memory to answer one question.
pub(super) fn last_line(path: &std::path::Path) -> Option<String> {
    use std::io::{Read as _, Seek as _, SeekFrom};

    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let from = len.saturating_sub(LOG_TAIL_BYTES);
    file.seek(SeekFrom::Start(from)).ok()?;
    let mut buf = Vec::new();
    file.take(LOG_TAIL_BYTES).read_to_end(&mut buf).ok()?;

    let text = String::from_utf8_lossy(&buf);
    let line = text.lines().rev().find(|l| !l.trim().is_empty())?.trim();
    Some(match line.char_indices().nth(LOG_LINE_CHARS) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line.to_owned(),
    })
}
