//! The names the rules are written in terms of.

/// Directories scanned, relative to the repo root. Everything else —
/// `data/`, `installers/`, `packaging/` — holds no Rust.
pub(crate) const ROOTS: &[&str] = &["crates", "xtask"];

/// `cfg` predicates that mean "this OS and not the others". A `cfg`
/// naming one of these is platform dispatch; `feature`, `test` and
/// `debug_assertions` are not.
pub(crate) const PLATFORM_PREDICATES: &[&str] = &[
    "target_os",
    "target_family",
    "target_arch",
    "target_env",
    "target_vendor",
    "unix",
    "windows",
];

/// Path components that restrict a module to one OS. A file or
/// directory named for a platform — `linux.rs`, `macos/`,
/// `windows_impl.rs` — may refine its own `cfg` freely; everywhere
/// else the dispatch belongs on a `mod` or `use`.
pub(crate) const PLATFORM_NAMES: &[&str] = &["linux", "macos", "windows", "unix"];

/// How many file-private module-level constants a `<purpose>.rs` may
/// keep beside the code that reads them. Four documented gaps above
/// the one function that places a tooltip are context; past a handful
/// the block has stopped being context and become a table, and a table
/// is what `consts.rs` is for.
pub(crate) const MAX_PRIVATE_CONSTS: usize = 5;

/// When a file holding one type and its `impl`s stops being readable
/// as one thing. `CONTRIBUTING.md` § File organization says "past ~400
/// lines → directory module"; this is that sentence, counted.
pub(crate) const MAX_TYPE_FILE_LINES: usize = 400;

/// How many free functions may sit beside a type in its own file.
/// A constructor and a couple of helpers are part of the type; seven
/// are a second concern that has moved in, and the fix is a
/// `<purpose>.rs` sibling — or moving the type out, when the file was
/// really a function file all along.
pub(crate) const MAX_LOOSE_FNS: usize = 6;

/// Crates that must hold no platform `cfg` at all: the binary and the
/// engine. A `#[cfg(target_os)]` reached for here is the signal that a
/// capability crate is missing — `CONTRIBUTING.md` § Style & guarantees.
pub(crate) const PLATFORM_FREE_CRATES: &[&str] = &["poltertype-app", "poltertype-core"];
