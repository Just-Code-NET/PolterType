//! Why an AT-SPI watcher could not start.

/// Why the watcher could not start. Every variant boils down to "no
/// usable a11y stack in this session" — headless CI, a11y disabled,
/// no registry daemon — which is why the caller treats construction
/// failure as a normal, log-once condition.
#[derive(Debug, thiserror::Error)]
pub(crate) enum AtspiCaretError {
    #[error("session bus unavailable: {0}")]
    SessionBus(#[source] zbus::Error),
    #[error("a11y bus address lookup failed: {0}")]
    A11yAddress(#[source] zbus::Error),
    #[error("a11y bus connection failed: {0}")]
    A11yConnect(#[source] zbus::Error),
    #[error("caret event registration failed: {0}")]
    Register(#[source] zbus::Error),
    #[error("caret signal subscription failed: {0}")]
    Subscribe(#[source] zbus::Error),
    #[error("watcher thread spawn failed: {0}")]
    Spawn(#[source] std::io::Error),
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AtspiFocusError {
    #[error("session bus unavailable: {0}")]
    SessionBus(zbus::Error),
    #[error("a11y bus address lookup failed: {0}")]
    A11yAddress(zbus::Error),
    #[error("a11y bus connection failed: {0}")]
    A11yConnect(zbus::Error),
    #[error("signal subscription failed: {0}")]
    Subscribe(zbus::Error),
    #[error("watcher thread failed to start: {0}")]
    Spawn(std::io::Error),
}
