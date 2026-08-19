#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;

use super::*;

/// A throwaway directory named after the test that made it, so
/// concurrent tests never share one.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ptap-discover-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_extension(dir: &Path, id: &str, exe: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join(MANIFEST_NAME),
        format!(
            r#"
id = "{id}"
name = "{id}"
version = "1.0.0"
kind = "extension"

[extension]
exe = "{exe}"
"#
        ),
    )
    .unwrap();
}

fn write_pack(dir: &Path, id: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join(MANIFEST_NAME),
        format!("id = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\n"),
    )
    .unwrap();
}

fn put_binary(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"#!/bin/true\n").unwrap();
}

#[test]
fn a_data_pack_is_not_an_extension() {
    // Not an error either: most plug-ins are packs, and complaining
    // about every one of them would make the log useless.
    let dir = scratch("pack");
    write_pack(&dir, "some-language");
    assert!(load(&dir).unwrap().is_none());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_directory_with_no_manifest_is_ignored_quietly() {
    let dir = scratch("empty");
    assert!(load(&dir).unwrap().is_none());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_installed_extension_is_found_through_its_bin_directory() {
    let dir = scratch("installed");
    write_extension(&dir, "demo", "demo-plugin");
    put_binary(&dir.join(EXTENSION_BIN_DIR).join("demo-plugin"));

    let found = load(&dir).unwrap().expect("should be an extension");
    assert_eq!(found.id, "demo");
    assert!(found.exe.ends_with("bin/demo-plugin"), "{:?}", found.exe);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_checkout_is_found_through_its_cargo_target_directory() {
    // The developer case: nothing has been installed, the binary is
    // wherever Cargo last put it.
    let dir = scratch("checkout");
    write_extension(&dir, "demo", "demo-plugin");
    put_binary(&dir.join("target").join("debug").join("demo-plugin"));

    let found = load(&dir).unwrap().expect("should be an extension");
    assert!(
        found.exe.ends_with("target/debug/demo-plugin"),
        "{:?}",
        found.exe
    );
    fs::remove_dir_all(&dir).unwrap();
}

/// The name a toolchain actually writes for a program called `stem` on
/// this platform: `stem` on Unix, `stem.exe` on Windows.
fn built_as(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

#[test]
fn an_installed_program_with_the_platforms_suffix_is_found() {
    // The regression behind "manifest declares a program but no built
    // copy was found", seen the first time PolterType ran on Windows: a
    // manifest names its program with no extension, so *resolution* has
    // to know about `.exe`. Phrased through `EXE_SUFFIX` rather than a
    // literal, so it asserts the same rule on every platform.
    let dir = scratch("suffix-installed");
    write_extension(&dir, "demo", "demo-plugin");
    put_binary(&dir.join(EXTENSION_BIN_DIR).join(built_as("demo-plugin")));

    let found = load(&dir).unwrap().expect("should be an extension");
    assert_eq!(found.id, "demo");
    assert_eq!(
        found.exe.file_name().and_then(|n| n.to_str()),
        Some(built_as("demo-plugin").as_str()),
        "{:?}",
        found.exe
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_checkout_built_with_the_platforms_suffix_resolves() {
    // The developer half of the same rule: what Cargo left in target/.
    let dir = scratch("suffix-checkout");
    put_binary(&dir.join("target").join("debug").join(built_as("tool")));

    let exe = resolve_exe(&dir, "tool").expect("the built program should resolve");
    assert_eq!(
        exe.file_name().and_then(|n| n.to_str()),
        Some(built_as("tool").as_str()),
        "{exe:?}"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_extension_whose_program_was_never_built_is_refused() {
    let dir = scratch("unbuilt");
    write_extension(&dir, "demo", "demo-plugin");
    assert!(matches!(load(&dir), Err(PluginError::NoExecutable(_))));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_manifest_naming_a_path_is_refused_before_anything_runs() {
    // The same rule as install, enforced again here: discovery is a
    // second door into "run this program", and it must not be a
    // weaker one.
    let dir = scratch("escape");
    write_extension(&dir, "demo", "../../../bin/sh");
    assert!(matches!(load(&dir), Err(PluginError::BadExecutablePath(_))));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_installed_bin_wins_over_a_build_directory() {
    // If both exist, the installed copy is the one the user chose.
    let dir = scratch("both");
    write_extension(&dir, "demo", "demo-plugin");
    put_binary(&dir.join(EXTENSION_BIN_DIR).join("demo-plugin"));
    put_binary(&dir.join("target").join("debug").join("demo-plugin"));

    let found = load(&dir).unwrap().unwrap();
    assert!(found.exe.ends_with("bin/demo-plugin"), "{:?}", found.exe);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn resolve_exe_prefers_the_more_recently_built_profile() {
    let dir = scratch("profiles");
    put_binary(&dir.join("target").join("release").join("tool"));
    // Written second, so it is the newer of the two.
    put_binary(&dir.join("target").join("debug").join("tool"));

    let exe = resolve_exe(&dir, "tool").expect("one of them should resolve");
    assert!(
        exe.ends_with("target/debug/tool"),
        "expected the newer build, got {exe:?}"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn installed_plugins_are_listed_and_development_ones_are_marked() {
    let root = scratch("listing");
    let data_dir = root.join("data");
    let installed = data_dir.join(PLUGINS_DIR).join("installed-one");
    write_extension(&installed, "installed-one", "tool");
    put_binary(&installed.join(EXTENSION_BIN_DIR).join("tool"));

    let dev = root.join("checkout");
    write_extension(&dev, "dev-one", "tool");
    put_binary(&dev.join("target").join("debug").join("tool"));

    let found = extensions_from(&data_dir, std::slice::from_ref(&dev));

    let ids: Vec<&str> = found.iter().map(|e| e.id.as_str()).collect();
    assert!(ids.contains(&"installed-one"), "{ids:?}");
    assert!(ids.contains(&"dev-one"), "{ids:?}");

    let installed_entry = found.iter().find(|e| e.id == "installed-one").unwrap();
    assert!(!installed_entry.development);
    let dev_entry = found.iter().find(|e| e.id == "dev-one").unwrap();
    assert!(
        dev_entry.development,
        "a plug-in that was never installed must be marked as such"
    );

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn an_installed_plugin_is_not_shadowed_by_a_checkout_of_the_same_id() {
    let root = scratch("shadow");
    let data_dir = root.join("data");
    let installed = data_dir.join(PLUGINS_DIR).join("same-id");
    write_extension(&installed, "same-id", "tool");
    put_binary(&installed.join(EXTENSION_BIN_DIR).join("tool"));

    let dev = root.join("checkout");
    write_extension(&dev, "same-id", "tool");
    put_binary(&dev.join("target").join("debug").join("tool"));

    let found = extensions_from(&data_dir, std::slice::from_ref(&dev));

    assert_eq!(found.len(), 1, "{found:?}");
    assert!(!found[0].development, "the installed copy must win");
    fs::remove_dir_all(&root).unwrap();
}
