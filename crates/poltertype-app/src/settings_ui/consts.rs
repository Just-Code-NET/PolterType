//! Fixed values for the Settings window: its icon, and the link
//! targets the About pane points at.

/// Side of the window icon, in pixels. 64 is the largest a title bar or
/// an Alt-Tab card asks for on a 200%-scaled display; the mark is
/// redrawn at that size rather than scaled up from a smaller one.
pub const WINDOW_ICON_PX: u32 = 64;

pub const SITE_URL: &str = "https://poltertype.com";
pub const REPO_URL: &str = "https://github.com/Just-Code-NET/PolterType";
pub const ISSUES_URL: &str = "https://github.com/Just-Code-NET/PolterType/issues";
/// The permissions guide the Setup pane links out to. Pinned to `main`,
/// like the tray's copy of the same link: it has to describe the
/// current setup script, not the release the reader is running.
pub const PERMISSIONS_DOC_URL: &str = crate::consts::SETUP_GUIDE_URL;

// ── Guides in other languages ───────────────────────────────────────

/// Where the guides live. A catalog says which language one has been
/// translated into and nothing else — the host stays in the program,
/// so no file dropped into the config directory can aim a button in
/// this window at a site of its own choosing.
pub(super) const DOCS_PREFIX: &str = "https://github.com/Just-Code-NET/PolterType/blob/main/docs/";

/// Every guide this window links to, with the catalog key naming the
/// language it exists in. A guide is listed once something opens it;
/// a key nobody translates leaves the link on the English original.
pub(super) const TRANSLATED_DOCS: &[(&str, &str)] = &[("PERMISSIONS.md", "docs.permissions")];

/// What a guide is written in until a catalog says otherwise.
pub(super) const DOC_LANGUAGE: &str = "en";

/// What a translated guide's file name ends in, both halves of the
/// swap: `PERMISSIONS.md` → `PERMISSIONS.uk.md`.
pub(super) const DOC_SUFFIX: &str = ".md";

/// Longest a language tag may be before the value is treated as
/// something other than a language. `zh-Hant-TW` is ten.
pub(super) const DOC_TAG_MAX: usize = 12;

// ── Plug-ins pane layout ────────────────────────────────────────────

/// Shown where a plug-in's config does not set a value. Not "0" and
/// not an empty selection that looks chosen: the plug-in has a default
/// and this pane does not know it.
pub(super) const PLUGIN_DEFAULT: &str = "(plug-in default)";

/// Placeholder for a list typed by hand, saying how to separate the
/// members. The alternative is a user discovering the rule by having
/// their spaces silently become part of a name.
pub(super) const PLUGIN_LIST_HINT: &str = "(empty — separate with commas)";

/// The same for a number box, which is too narrow to say it in full.
pub(super) const PLUGIN_DEFAULT_SHORT: &str = "default";

/// How many suggestions are drawn under a box at once: enough to
/// browse, few enough that the form under it stays on screen. The rest
/// are counted, not scrolled — this pane has one scrolling region.
pub(super) const SUGGEST_ROWS: usize = 8;

/// Width of the value column. Every switch, picker and number lands on
/// the same right-hand edge — which is the whole difference between a
/// form and a list of sentences with boxes after them.
pub(super) const VALUE_COLUMN: f32 = 210.0;

/// Gap between what a setting is and what it is set to.
pub(super) const LABEL_GAP: f32 = 24.0;

/// Width of the section list. Narrower than the window's own nav so the
/// two do not read as one two-level menu of equal weight.
pub(super) const SECTION_NAV: f32 = 186.0;

/// How wide a number gets. A box sized for a paragraph invites one.
pub(super) const NUMBER_WIDTH: f32 = 110.0;
