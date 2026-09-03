//! What the line scanner must not misread, and what each rule fires on.
//!
//! The scanner is not a parser, so the cases worth pinning are the
//! ones where source only *looks* like a declaration: raw strings,
//! block comments, indented items inside a body.

use std::path::{Path, PathBuf};

use super::consts::MAX_TYPE_FILE_LINES;
use super::enums::{Kind, Role, Rule};
use super::rules::{check_file, platform_of, role_of};
use super::scan::{names_platform, scan};

fn kinds(src: &str) -> Vec<(Kind, String)> {
    scan(src)
        .items
        .into_iter()
        .map(|i| (i.kind, i.name))
        .collect()
}

fn rules_for(path: &str, src: &str) -> Vec<Rule> {
    check_file(Path::new(path), &scan(src))
        .into_iter()
        .map(|f| f.rule)
        .collect()
}

#[test]
fn reads_the_shapes_a_declaration_comes_in() {
    let src = "\
pub(crate) const MAX: usize = 4;
static NAME: &str = \"x\";
pub type Alias<T> = Vec<T>;
pub enum Kind {}
struct Plain;
pub trait Backend {}
pub async unsafe fn go() {}
pub const fn size() -> usize { 0 }
unsafe extern \"C\" fn cb() {}
impl<T> Backend for Plain {}
pub use crate::x;
mod inner;
";
    let got = kinds(src);
    let want = [
        (Kind::Const, "MAX"),
        (Kind::Static, "NAME"),
        (Kind::TypeAlias, "Alias"),
        (Kind::Enum, "Kind"),
        (Kind::Struct, "Plain"),
        (Kind::Trait, "Backend"),
        (Kind::Fn, "go"),
        (Kind::Fn, "size"),
        (Kind::Fn, "cb"),
        (Kind::Impl, "Plain"),
        (Kind::Use, ""),
        (Kind::Mod, "inner"),
    ];
    assert_eq!(got.len(), want.len(), "got {got:?}");
    for (got, (kind, name)) in got.iter().zip(want) {
        assert_eq!((got.0, got.1.as_str()), (kind, name));
    }
}

#[test]
fn ignores_declarations_that_are_only_text() {
    let src = "\
const REAL: u8 = 1;
const SCRIPT: &str = r#\"
fn not_rust() {}
const NOT_A_CONST: u8 = 0;
\"#;
/*
struct Commented;
*/
fn body() {
    struct Local;
}
";
    let got = kinds(src);
    assert_eq!(
        got.iter()
            .map(|(k, n)| (*k, n.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (Kind::Const, "REAL"),
            (Kind::Const, "SCRIPT"),
            (Kind::Fn, "body"),
        ]
    );
}

#[test]
fn a_feature_named_windows_is_not_a_platform() {
    assert!(names_platform("#[cfg(target_os = \"linux\")]"));
    assert!(names_platform("#[cfg(not(windows))]"));
    assert!(names_platform(
        "#[cfg(any(unix, target_arch = \"wasm32\"))]"
    ));
    assert!(!names_platform("#[cfg(feature = \"windows\")]"));
    assert!(!names_platform("#[cfg(test)]"));
    assert!(!names_platform("#[cfg(feature = \"ai\")]"));
}

#[test]
fn roles_and_platforms_come_from_the_path() {
    assert_eq!(role_of(Path::new("a/mod.rs")), Role::Wiring);
    assert_eq!(role_of(Path::new("a/main.rs")), Role::Main);
    assert_eq!(role_of(Path::new("a/consts.rs")), Role::Consts);
    assert_eq!(role_of(Path::new("a/plugin_pane_tests.rs")), Role::Tests);
    assert_eq!(role_of(Path::new("a/switcher.rs")), Role::Free);

    assert_eq!(platform_of(Path::new("src/linux/x11.rs")), Some("linux"));
    assert_eq!(
        platform_of(Path::new("src/focus/macos_impl.rs")),
        Some("macos")
    );
    assert_eq!(
        platform_of(Path::new("src/apply/windows.rs")),
        Some("windows")
    );
    assert_eq!(platform_of(Path::new("src/engine/decide.rs")), None);
}

#[test]
fn wiring_files_hold_wiring() {
    let findings = rules_for(
        "crates/c/src/mod.rs",
        "mod a;\npub use a::B;\nfn helper() {}\n",
    );
    assert_eq!(findings, vec![Rule::Wiring]);
    assert!(rules_for("crates/c/src/mod.rs", "mod a;\npub use a::B;\n").is_empty());
}

#[test]
fn exported_constants_and_second_types_leave_a_purpose_file() {
    let src = "\
pub(crate) const TTL: u64 = 5;
pub struct One;
pub struct Two;
pub trait Seam {}
";
    let got = rules_for("crates/c/src/thing.rs", src);
    assert_eq!(got, vec![Rule::Consts, Rule::Types, Rule::Types]);
}

#[test]
fn a_handful_of_private_constants_is_context_and_a_table_is_not() {
    let context = "const A: u8 = 1;\nconst B: u8 = 2;\nfn use_them() {}\n";
    assert!(rules_for("crates/c/src/place.rs", context).is_empty());

    let table: String = (0..8).map(|i| format!("const C{i}: u8 = {i};\n")).collect();
    let got = rules_for("crates/c/src/place.rs", &table);
    assert_eq!(got, vec![Rule::Consts; 3], "only the ones past the cap");
}

#[test]
fn one_type_with_its_impls_is_the_point_of_a_purpose_file() {
    let src = "\
pub struct LayoutDb {}
impl LayoutDb {}
fn helper() -> u8 { 0 }
";
    assert!(rules_for("crates/c/src/db.rs", src).is_empty());
}

#[test]
fn platform_cfg_is_allowed_on_the_dispatch_and_inside_a_per_os_file() {
    let dispatch = "#[cfg(target_os = \"linux\")]\nmod linux;\n";
    assert!(rules_for("crates/c/src/lib.rs", dispatch).is_empty());

    let refining = "#[cfg(target_os = \"linux\")]\npub fn go() {}\n";
    assert!(rules_for("crates/c/src/linux.rs", refining).is_empty());

    let scattered = "#[cfg(target_os = \"macos\")]\npub fn go() {}\n";
    assert_eq!(
        rules_for("crates/c/src/notify.rs", scattered),
        vec![Rule::Platform]
    );
}

#[test]
fn a_cfg_block_inside_a_body_is_the_shape_the_rule_exists_to_stop() {
    let src = "\
pub fn create() -> Backend {
    #[cfg(windows)]
    {
        Backend::windows()
    }
}
";
    assert_eq!(
        rules_for("crates/c/src/factory.rs", src),
        vec![Rule::Platform]
    );
    assert!(rules_for("crates/c/src/windows/factory.rs", src).is_empty());
}

#[test]
fn the_binary_and_the_engine_hold_no_platform_cfg_at_all() {
    let dispatch = "#[cfg(target_os = \"linux\")]\nmod linux;\n";
    assert_eq!(
        rules_for("crates/poltertype-app/src/lib.rs", dispatch),
        vec![Rule::Platform]
    );
    assert_eq!(
        rules_for("crates/poltertype-core/src/lib.rs", dispatch),
        vec![Rule::Platform]
    );
}

#[test]
fn tests_may_assert_about_the_difference_between_platforms() {
    let src = "#[cfg(target_os = \"linux\")]\n#[test]\nfn only_here() {}\n";
    assert!(rules_for("crates/poltertype-app/src/tests.rs", src).is_empty());
}

#[test]
fn inline_test_modules_are_rejected_but_the_declaration_is_not() {
    let inline = "#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n";
    assert_eq!(
        rules_for("crates/c/src/thing.rs", inline),
        vec![Rule::InlineTests]
    );
    assert!(rules_for("crates/c/src/thing.rs", "#[cfg(test)]\nmod tests;\n").is_empty());
}

#[test]
fn a_module_is_found_by_its_file_name() {
    let redirected = "#[cfg(test)]\n#[path = \"thing_tests.rs\"]\nmod tests;\n";
    assert_eq!(
        rules_for("crates/c/src/thing.rs", redirected),
        vec![Rule::ModTree]
    );
}

#[test]
fn a_mod_without_a_file_and_a_file_without_a_mod_are_both_found() {
    let files = vec![
        (
            PathBuf::from("crates/c/src/lib.rs"),
            scan("mod present;\nmod missing;\n"),
        ),
        (PathBuf::from("crates/c/src/present.rs"), scan("")),
        (PathBuf::from("crates/c/src/orphan.rs"), scan("")),
    ];
    let found = super::modtree::check(&files);
    let messages: Vec<_> = found.iter().map(|f| f.message.as_str()).collect();
    assert_eq!(found.len(), 2, "{messages:?}");
    assert!(messages[0].contains("`mod missing` has no file"));
    assert!(messages[1].contains("no `mod` declares this file"));
}

#[test]
fn an_impl_block_names_the_type_it_is_for() {
    // Every shape the workspace actually writes, including the two the
    // whitespace split gets wrong: generics on the keyword, and a
    // trait that is itself generic.
    let scanned = scan(
        "\
impl Foo {}
impl<'a> Bar<'a> {}
impl fst::Automaton for Foo {}
impl From<&str> for Baz {}
",
    );
    let got: Vec<(&str, bool)> = scanned
        .items
        .iter()
        .map(|i| (i.name.as_str(), i.inherent_impl))
        .collect();
    assert_eq!(
        got,
        [("Foo", true), ("Bar", true), ("Foo", false), ("Baz", false)]
    );
}

#[test]
fn a_type_file_holds_one_type_and_its_behaviour() {
    let src = "\
pub struct Suggester {}
impl Suggester {}
struct LevAutomaton;
impl fst::Automaton for LevAutomaton {}
";
    assert_eq!(rules_for("crates/c/src/suggest.rs", src), [Rule::TypeFile]);
}

#[test]
fn two_exported_types_that_nothing_implements_are_still_two_types() {
    let src = "pub struct A {}\npub struct B {}\n";
    assert_eq!(rules_for("crates/c/src/pair.rs", src), [Rule::Types]);
}

#[test]
fn a_file_of_free_functions_is_held_to_no_type_budget() {
    // The type is incidental here — nothing implements it — so the
    // file is what its name says, and twenty functions are its point.
    let mut src = String::from("struct Held;\n");
    for i in 0..20 {
        src.push_str(&format!("fn helper{i}() {{}}\n"));
    }
    assert!(rules_for("crates/c/src/place.rs", &src).is_empty());
}

#[test]
fn helpers_beside_a_type_are_context_until_there_is_a_crowd() {
    let head = "struct Gate;\nimpl Gate {}\n";
    let fns = |n: usize| {
        (0..n)
            .map(|i| format!("fn h{i}() {{}}\n"))
            .collect::<String>()
    };
    assert!(rules_for("crates/c/src/gate.rs", &format!("{head}{}", fns(6))).is_empty());
    assert_eq!(
        rules_for("crates/c/src/gate.rs", &format!("{head}{}", fns(7))),
        [Rule::TypeFile]
    );
}

#[test]
fn a_type_file_past_the_line_budget_becomes_a_directory() {
    let head = "struct Big;\nimpl Big {}\n";
    let blank = |n: usize| "\n".repeat(n);
    let at_budget = format!("{head}{}", blank(MAX_TYPE_FILE_LINES - 2));
    let over = format!("{head}{}", blank(MAX_TYPE_FILE_LINES - 1));
    assert!(rules_for("crates/c/src/big.rs", &at_budget).is_empty());
    assert_eq!(rules_for("crates/c/src/big.rs", &over), [Rule::TypeFile]);
}
