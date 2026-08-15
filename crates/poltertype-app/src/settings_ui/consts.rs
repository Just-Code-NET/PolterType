//! Fixed values for the Settings window: its icon, and the link
//! targets the About pane points at.

/// Side of the window icon, in pixels. 64 is the largest size a title
/// bar or an Alt-Tab card asks for on a 200%-scaled display; the mark
/// is redrawn at that size rather than scaled from a smaller one, and
/// the whole render is well under a millisecond.
pub const WINDOW_ICON_PX: u32 = 64;

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
