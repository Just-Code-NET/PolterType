//! Link targets for the About pane.

/// Public landing page.
pub const SITE_URL: &str = "https://poltertype.com";
/// Source repository.
pub const REPO_URL: &str = "https://github.com/Just-Code-NET/PolterType";
/// Issue tracker.
pub const ISSUES_URL: &str = "https://github.com/Just-Code-NET/PolterType/issues";
/// The permissions guide the Setup pane links out to, for whatever
/// the pane itself cannot say in two sentences. Pinned to `main`, like
/// the tray's copy of the same link: it has to describe the current
/// setup script, not the release the reader happens to be running.
pub const PERMISSIONS_DOC_URL: &str = crate::consts::SETUP_GUIDE_URL;
