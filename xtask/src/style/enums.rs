//! What a file is for, what it declares, and what a check found.

use std::fmt;

/// The role a file's *name* assigns it, per `CONTRIBUTING.md`
/// § File organization. The role decides what may be declared in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    /// `lib.rs` / `mod.rs` — `mod` and `use` only.
    Wiring,
    /// `main.rs` — wiring plus `fn main`.
    Main,
    /// `build.rs` — cargo mandates one file; role rules do not apply.
    Build,
    /// `tests.rs` / `*_tests.rs` — test code, whatever it needs.
    Tests,
    Consts,
    Enums,
    Types,
    Traits,
    /// `<purpose>.rs` — free functions, or one type with its `impl`s.
    Free,
}

/// A top-level declaration, as far as the line scanner can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Const,
    Static,
    TypeAlias,
    Enum,
    Struct,
    Union,
    Trait,
    Fn,
    Impl,
    Mod,
    Use,
    Macro,
}

impl Kind {
    /// Does this declare a type whose shape other code can name?
    pub(crate) fn is_type_def(self) -> bool {
        matches!(self, Kind::Enum | Kind::Struct | Kind::Union | Kind::Trait)
    }

    /// `mod` and `use` are the two places a platform `cfg` is allowed
    /// to choose between per-OS implementations.
    pub(crate) fn is_dispatch(self) -> bool {
        matches!(self, Kind::Mod | Kind::Use)
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Kind::Const => "const",
            Kind::Static => "static",
            Kind::TypeAlias => "type",
            Kind::Enum => "enum",
            Kind::Struct => "struct",
            Kind::Union => "union",
            Kind::Trait => "trait",
            Kind::Fn => "fn",
            Kind::Impl => "impl",
            Kind::Mod => "mod",
            Kind::Use => "use",
            Kind::Macro => "macro_rules!",
        }
    }
}

/// Which rule a finding breaks. Named after the section that states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rule {
    /// A wiring file declaring something other than `mod` / `use`.
    Wiring,
    /// A role file holding a kind that is not its own.
    RoleFile,
    /// A module-level constant outside `consts.rs`.
    Consts,
    /// A second exported type in one file, or a `trait` outside
    /// `traits.rs`.
    Types,
    /// An inline `mod tests { … }` block.
    InlineTests,
    /// A platform `cfg` somewhere other than a dispatch or a per-OS
    /// module.
    Platform,
    /// A `mod` with no file, or a file no `mod` declares.
    ModTree,
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Rule::Wiring => "wiring",
            Rule::RoleFile => "role-file",
            Rule::Consts => "consts",
            Rule::Types => "types",
            Rule::InlineTests => "inline-tests",
            Rule::Platform => "platform",
            Rule::ModTree => "mod-tree",
        };
        f.write_str(s)
    }
}
