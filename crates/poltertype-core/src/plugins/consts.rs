//! What a data-only pack is allowed to contain.

/// Directory under `<data_dir>` holding installed packs.
pub const PLUGINS_DIR: &str = "plugins";

/// The manifest every pack must have.
pub const MANIFEST_NAME: &str = "manifest.toml";

/// Sub-directories a pack may populate, and the file extensions
/// allowed in each.
///
/// An allow-list rather than a deny-list on purpose: a deny-list has
/// to guess every dangerous thing, and it will be wrong about the one
/// that matters. This says what a *language pack* is, and everything
/// else is left on the floor with a warning.
pub const ALLOWED_CONTENT: &[(&str, &[&str])] = &[
    ("layout-mappings", &["toml"]),
    ("wordlists", &["fst", "txt", "gz"]),
    ("i18n", &["toml"]),
];

/// Sub-directories an **extension** may populate, on top of
/// [`ALLOWED_CONTENT`] — an extension may also carry data.
///
/// The whole difference between the two kinds lives in this one entry:
/// an extension may ship programs, in `bin/`, and nowhere else. Every
/// other guarantee (no traversal, no symlinks, a size budget, atomic
/// replacement) is identical, because none of them stopped being true
/// just because the pack contains a binary.
///
/// The extension list is empty rather than enumerating `.exe`, `.so`
/// and friends: on Unix an executable has no extension at all, so an
/// extension allow-list would be a list of the shapes we happened to
/// think of. What bounds `bin/` instead is that nothing in it is ever
/// *loaded* — it is spawned as a separate process, only by the name
/// the manifest declares, and only after the user installed it
/// knowing it was an extension.
pub const EXTENSION_CONTENT: &[(&str, &[&str])] = &[("bin", &[])];

/// Files permitted at the top level of a pack, beyond the manifest.
/// Documentation and licence text, so a pack can carry its own terms.
pub const ALLOWED_TOP_LEVEL: &[&str] = &[
    "manifest.toml",
    "README.md",
    "LICENSE",
    "LICENSE.txt",
    "LICENSE.md",
    "CREDITS.md",
];

/// Total bytes one pack may occupy. Generous next to a bundled
/// language (the Turkish FST alone is 15 MB) and still a bound: a
/// "language pack" that wants a gigabyte is not one.
pub const MAX_PACK_BYTES: u64 = 256 * 1024 * 1024;

/// Most files a pack may contain. Guards the enumeration itself, so a
/// directory with a million entries cannot make installation hang.
pub const MAX_PACK_FILES: usize = 512;
