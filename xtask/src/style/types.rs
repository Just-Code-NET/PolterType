//! What the scanner extracts from a file, and what a check reports.

use std::path::PathBuf;

use super::enums::{Kind, Rule};

/// One top-level declaration.
pub(crate) struct Item {
    pub(crate) line: usize,
    pub(crate) kind: Kind,
    pub(crate) name: String,
    /// `pub` in any form — visible outside the file it is written in.
    pub(crate) exported: bool,
    /// Carries a `#[cfg(…)]` naming a platform.
    pub(crate) platform_cfg: bool,
    /// The declaration ends in `{` on its own line — an inline module
    /// body rather than `mod foo;`.
    pub(crate) has_body: bool,
    /// Carries `#[path = "…"]`, pointing the module somewhere the file
    /// name does not.
    pub(crate) path_attr: bool,
    /// `impl Foo` rather than `impl Trait for Foo`. The type whose
    /// file this is, is the one implemented inherently in it.
    pub(crate) inherent_impl: bool,
}

/// One file, as the line scanner sees it.
pub(crate) struct FileScan {
    pub(crate) items: Vec<Item>,
    /// Lines of an indented `#[cfg(…)]` naming a platform: inside a
    /// function body, or on a struct field or enum variant.
    pub(crate) nested_platform_cfg: Vec<usize>,
    pub(crate) lines: usize,
}

/// A rule violation, at the line a reader should look at.
pub(crate) struct Finding {
    pub(crate) file: PathBuf,
    pub(crate) line: usize,
    pub(crate) rule: Rule,
    pub(crate) message: String,
}
