//! `tracing` setup for the tray process.

use poltertype_core::settings::SettingsStore;

/// Init `tracing` with a stderr layer and a daily-rotating file appender
/// under `<data_dir>/poltertype/logs/`. Returns the file writer's guard —
/// dropping it closes the file.
pub(crate) fn init_tracing() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{EnvFilter, fmt};

    // cosmic-text / fontdb log the *text being shaped* at debug level
    // ("Failed to find script fallback …: '<word>'") — and the
    // suggestion tooltip shapes the user's words. Those targets are
    // capped at warn no matter what RUST_LOG says: typed text stays
    // out of the logs at any level.
    let mut filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        // `[general].log_level` from `config.toml`, applied to our own
        // crates only: it is the knob a user actually has when the app
        // was started from a menu entry, and a global `debug` buries
        // their own lines under iced and zbus. `RUST_LOG` still wins.
        let mut base = EnvFilter::new("info");
        if let Some(level) = SettingsStore::peek_log_level()
            && let Ok(directive) = format!("poltertype={level}").parse()
        {
            base = base.add_directive(directive);
        }
        base
    });
    for target in ["cosmic_text=warn", "fontdb=warn"] {
        if let Ok(directive) = target.parse() {
            filter = filter.add_directive(directive);
        }
    }

    let stderr_layer = fmt::layer().with_writer(std::io::stderr).with_target(false);

    let (file_layer, guard) = match SettingsStore::log_dir() {
        Ok(dir) => {
            if let Err(e) = std::fs::create_dir_all(&dir) {
                eprintln!("poltertype: could not create log dir {dir:?}: {e}");
                (None, None)
            } else {
                let appender = tracing_appender::rolling::daily(&dir, "poltertype.log");
                let (writer, guard) = tracing_appender::non_blocking(appender);
                let layer = fmt::layer()
                    .with_writer(writer)
                    .with_ansi(false)
                    .with_target(false);
                (Some(layer), Some(guard))
            }
        }
        Err(e) => {
            eprintln!("poltertype: cannot resolve log dir: {e}");
            (None, None)
        }
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();

    guard
}
