//! The per-file checks, one function per rule.
//!
//! Every rule here is stated in `CONTRIBUTING.md` — § File
//! organization for the roles, § Style & guarantees for the platform
//! split. When the two disagree, the document is right and this file
//! is the bug.

use std::path::Path;

use super::consts::{MAX_PRIVATE_CONSTS, PLATFORM_FREE_CRATES, PLATFORM_NAMES};
use super::enums::{Kind, Role, Rule};
use super::types::{FileScan, Finding};

pub(crate) fn role_of(path: &Path) -> Role {
    match path.file_name().and_then(|n| n.to_str()).unwrap_or("") {
        "lib.rs" | "mod.rs" => Role::Wiring,
        "main.rs" => Role::Main,
        "build.rs" => Role::Build,
        "consts.rs" => Role::Consts,
        "enums.rs" => Role::Enums,
        "types.rs" => Role::Types,
        "traits.rs" => Role::Traits,
        name if name == "tests.rs" || name.ends_with("_tests.rs") => Role::Tests,
        _ => Role::Free,
    }
}

/// The OS a path already restricts itself to, if any. Inside such a
/// module a further `cfg` can only refine a choice already made.
pub(crate) fn platform_of(path: &Path) -> Option<&'static str> {
    path.components()
        .filter_map(|c| c.as_os_str().to_str())
        .filter_map(|part| {
            let stem = part.strip_suffix(".rs").unwrap_or(part);
            PLATFORM_NAMES.iter().copied().find(|p| {
                stem == *p || stem.starts_with(&format!("{p}_")) || stem.ends_with(&format!("_{p}"))
            })
        })
        .next_back()
}

pub(crate) fn check_file(rel: &Path, scan: &FileScan) -> Vec<Finding> {
    let role = role_of(rel);
    let mut out = Vec::new();
    let mut add = |line: usize, rule: Rule, message: String| {
        out.push(Finding {
            file: rel.to_path_buf(),
            line,
            rule,
            message,
        })
    };

    match role {
        Role::Tests | Role::Build => {}
        Role::Wiring => {
            for it in &scan.items {
                if !it.kind.is_dispatch() {
                    add(
                        it.line,
                        Rule::Wiring,
                        format!(
                            "`{} {}` in a wiring file — `mod.rs` / `lib.rs` hold declarations and re-exports only",
                            it.kind.as_str(),
                            it.name
                        ),
                    );
                }
            }
        }
        Role::Main => {
            for it in &scan.items {
                let is_main = it.kind == Kind::Fn && it.name == "main";
                if !it.kind.is_dispatch() && !is_main {
                    add(
                        it.line,
                        Rule::Wiring,
                        format!(
                            "`{} {}` in `main.rs` — the binary entry holds wiring and `fn main`",
                            it.kind.as_str(),
                            it.name
                        ),
                    );
                }
            }
        }
        Role::Consts | Role::Enums | Role::Types | Role::Traits => {
            for it in &scan.items {
                if !role_admits(role, it.kind) {
                    add(
                        it.line,
                        Rule::RoleFile,
                        format!(
                            "`{} {}` in `{}` — that file holds {} only",
                            it.kind.as_str(),
                            it.name,
                            file_name(rel),
                            admitted(role)
                        ),
                    );
                }
            }
        }
        Role::Free => {
            let mut exported_types = 0;
            let mut private_consts = 0;
            for it in &scan.items {
                match it.kind {
                    Kind::Const | Kind::Static if it.exported => add(
                        it.line,
                        Rule::Consts,
                        format!(
                            "exported `{} {}` outside `consts.rs` — a constant another file can \
                             name lives in the file whose name says so",
                            it.kind.as_str(),
                            it.name
                        ),
                    ),
                    Kind::Const | Kind::Static => {
                        private_consts += 1;
                        if private_consts > MAX_PRIVATE_CONSTS {
                            add(
                                it.line,
                                Rule::Consts,
                                format!(
                                    "more than {MAX_PRIVATE_CONSTS} module-level constants in one \
                                     file — `{}` and the rest are a table, and tables live in \
                                     `consts.rs`",
                                    it.name
                                ),
                            );
                        }
                    }
                    Kind::Trait if it.exported => add(
                        it.line,
                        Rule::Types,
                        format!("exported `trait {}` outside `traits.rs`", it.name),
                    ),
                    k if k.is_type_def() && it.exported => {
                        exported_types += 1;
                        if exported_types > 1 {
                            add(
                                it.line,
                                Rule::Types,
                                format!(
                                    "`{} {}` is a second exported type in one file — a file holds \
                                     one type with its behaviour, or none",
                                    k.as_str(),
                                    it.name
                                ),
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    for it in &scan.items {
        if it.kind == Kind::Mod && it.name == "tests" && it.has_body {
            add(
                it.line,
                Rule::InlineTests,
                "inline `mod tests { … }` — unit tests live in a sibling `tests.rs`".to_owned(),
            );
        }
        if it.path_attr {
            add(
                it.line,
                Rule::ModTree,
                format!(
                    "`#[path]` on `mod {}` — a module is found by its file name, so that the \
                     directory tree can be read as the module tree",
                    it.name
                ),
            );
        }
    }

    out.extend(check_platform(rel, scan, role));
    out
}

fn check_platform(rel: &Path, scan: &FileScan, role: Role) -> Vec<Finding> {
    let mut out = Vec::new();
    // A test asserting per-OS behaviour is the one place the split
    // cannot help: the assertion is *about* the difference.
    if role == Role::Tests {
        return out;
    }
    let platform_free = PLATFORM_FREE_CRATES
        .iter()
        .any(|c| rel.starts_with(Path::new("crates").join(c)));
    let own_platform = platform_of(rel);

    let mut add = |line: usize, message: String| {
        out.push(Finding {
            file: rel.to_path_buf(),
            line,
            rule: Rule::Platform,
            message,
        })
    };

    for it in &scan.items {
        if !it.platform_cfg {
            continue;
        }
        if platform_free {
            add(
                it.line,
                format!(
                    "platform `cfg` on `{} {}` in a crate that must hold none — the answer is a \
                     capability crate, not a `cfg`",
                    it.kind.as_str(),
                    it.name
                ),
            );
        } else if !it.kind.is_dispatch() && own_platform.is_none() {
            add(
                it.line,
                format!(
                    "platform `cfg` on `{} {}` in a file that is not per-OS — gate the `mod` or \
                     the `use`, and put the body in a per-OS file",
                    it.kind.as_str(),
                    it.name
                ),
            );
        }
    }

    for &line in &scan.nested_platform_cfg {
        if platform_free {
            add(
                line,
                "platform `cfg` in a crate that must hold none — the answer is a capability \
                 crate, not a `cfg`"
                    .to_owned(),
            );
        } else if own_platform.is_none() {
            add(
                line,
                "platform `cfg` inside a body or a field — the choice belongs on the `mod` or \
                 `use` that picks the per-OS module, made once"
                    .to_owned(),
            );
        }
    }

    out
}

fn role_admits(role: Role, kind: Kind) -> bool {
    if matches!(kind, Kind::Use | Kind::Mod | Kind::TypeAlias) {
        return true;
    }
    match role {
        Role::Consts => matches!(kind, Kind::Const | Kind::Static),
        Role::Enums => matches!(kind, Kind::Enum | Kind::Impl),
        Role::Types => matches!(kind, Kind::Struct | Kind::Union | Kind::Impl),
        Role::Traits => matches!(kind, Kind::Trait | Kind::Impl),
        _ => true,
    }
}

fn admitted(role: Role) -> &'static str {
    match role {
        Role::Consts => "constants",
        Role::Enums => "enums and their `impl`s",
        Role::Types => "data structs and their `impl`s",
        Role::Traits => "traits and their `impl`s",
        _ => "its own kind",
    }
}

fn file_name(path: &Path) -> &str {
    path.file_name().and_then(|n| n.to_str()).unwrap_or("")
}
