use super::*;
use std::path::{Path, PathBuf};

/// A scratch directory that removes itself. Tests here touch the
/// filesystem by nature — the thing under test is a file copier.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "poltertype-plugins-{}-{tag}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        Self(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: &str) {
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = std::fs::write(path, contents);
}

/// A minimal well-formed pack.
fn make_pack(root: &Path, id: &str) {
    write(
        &root.join("manifest.toml"),
        &format!("id = \"{id}\"\nname = \"Test pack\"\nversion = \"1.0\"\n"),
    );
    write(
        &root.join("layout-mappings/xx_xx.toml"),
        "id = \"xx-XX\"\nname = \"Test\"\nscript = \"Latin\"\n\n[keys]\n0x10 = { plain = \"q\" }\n",
    );
}

/// A minimal well-formed extension: a manifest naming a program, and
/// that program sitting in `bin/` under the name the platform's
/// toolchain would actually have written.
fn make_extension(root: &Path, id: &str, exe_stem: &str) {
    write(
        &root.join("manifest.toml"),
        &format!(
            "id = \"{id}\"\nname = \"Test extension\"\nversion = \"1.0\"\n\
             kind = \"extension\"\n\n[extension]\nexe = \"{exe_stem}\"\n"
        ),
    );
    write(
        &root
            .join("bin")
            .join(format!("{exe_stem}{}", std::env::consts::EXE_SUFFIX)),
        "not really a program",
    );
}

// ── the happy path ───────────────────────────────────────────────────

#[test]
fn installs_an_extension_whose_program_carries_the_platform_suffix() {
    // The manifest says `helper`; on Windows the file in `bin/` is
    // `helper.exe`, because that is what every toolchain writes. The
    // installer refusing that pack would make a portable manifest
    // uninstallable on the one platform that decorates the name — so
    // resolution has to know about the suffix on both doors into
    // "run this program", install as well as discovery.
    let src = Scratch::new("ext-src");
    let data = Scratch::new("ext-data");
    make_extension(src.path(), "testext", "helper");

    let out = install(src.path(), data.path()).expect("extension should install");
    assert_eq!(out.id, "testext");
    assert!(
        out.path
            .join("bin")
            .join(format!("helper{}", std::env::consts::EXE_SUFFIX))
            .is_file(),
        "the program should have been copied into the installed bin/"
    );
}

#[test]
fn installs_a_well_formed_pack() {
    let src = Scratch::new("src");
    let data = Scratch::new("data");
    make_pack(src.path(), "testpack");

    let out = install(src.path(), data.path()).expect("install");
    assert_eq!(out.id, "testpack");
    assert_eq!(out.version, "1.0");
    assert!(!out.replaced);
    assert!(out.path.join("layout-mappings/xx_xx.toml").is_file());
    assert!(out.path.join("manifest.toml").is_file());
}

/// Installing over an existing pack is the update path, and is
/// deliberately the same code — an update that behaved differently
/// would be tested half as often.
#[test]
fn installing_again_replaces_and_reports_it() {
    let src = Scratch::new("src2");
    let data = Scratch::new("data2");
    make_pack(src.path(), "testpack");
    install(src.path(), data.path()).expect("first");

    // A second version, with one file fewer.
    let src2 = Scratch::new("src2b");
    write(
        &src2.path().join("manifest.toml"),
        "id = \"testpack\"\nname = \"Test pack\"\nversion = \"2.0\"\n",
    );
    write(&src2.path().join("wordlists/xx_xx-stop.txt"), "a\nb\n");

    let out = install(src2.path(), data.path()).expect("second");
    assert!(out.replaced);
    assert_eq!(out.version, "2.0");
    assert!(out.path.join("wordlists/xx_xx-stop.txt").is_file());
    assert!(
        !out.path.join("layout-mappings/xx_xx.toml").exists(),
        "the previous pack's files must not survive an update"
    );
}

#[test]
fn lists_and_removes() {
    let src = Scratch::new("src3");
    let data = Scratch::new("data3");
    make_pack(src.path(), "testpack");
    install(src.path(), data.path()).expect("install");

    let listed = list_installed(data.path());
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "testpack");

    assert!(uninstall("testpack", data.path()).expect("uninstall"));
    assert!(list_installed(data.path()).is_empty());
    assert!(
        !uninstall("testpack", data.path()).expect("second uninstall"),
        "removing what is not there is not an error"
    );
}

// ── the allow-list ───────────────────────────────────────────────────

/// A pack is data. Anything that is not gets left behind — and
/// reported, so nobody wonders where their file went.
#[test]
fn executables_and_stray_files_are_not_installed() {
    let src = Scratch::new("src4");
    let data = Scratch::new("data4");
    make_pack(src.path(), "testpack");
    write(&src.path().join("evil.sh"), "#!/bin/sh\necho pwned\n");
    write(&src.path().join("payload.so"), "binary");
    write(&src.path().join(".bashrc"), "echo pwned");
    write(&src.path().join("config.toml"), "schema_version = 1");
    write(&src.path().join("bin/tool"), "binary");
    // Wrong extension inside an allowed directory.
    write(&src.path().join("layout-mappings/notes.txt"), "hi");

    let out = install(src.path(), data.path()).expect("install");
    for unwanted in ["evil.sh", "payload.so", ".bashrc", "config.toml"] {
        assert!(
            !out.path.join(unwanted).exists(),
            "{unwanted} must not be installed"
        );
    }
    assert!(!out.path.join("bin").exists(), "bin/ must not be installed");
    assert!(!out.path.join("layout-mappings/notes.txt").exists());
    assert!(
        out.skipped.len() >= 5,
        "everything skipped should be reported: {:?}",
        out.skipped
    );
}

#[test]
fn documentation_alongside_the_manifest_is_kept() {
    let src = Scratch::new("src5");
    let data = Scratch::new("data5");
    make_pack(src.path(), "testpack");
    write(&src.path().join("README.md"), "# pack");
    write(&src.path().join("LICENSE"), "MIT");

    let out = install(src.path(), data.path()).expect("install");
    assert!(out.path.join("README.md").is_file());
    assert!(out.path.join("LICENSE").is_file());
}

// ── refusals ─────────────────────────────────────────────────────────

#[test]
fn a_pack_without_a_manifest_is_refused() {
    let src = Scratch::new("src6");
    let data = Scratch::new("data6");
    write(&src.path().join("layout-mappings/x.toml"), "id = \"x\"");
    assert!(matches!(
        install(src.path(), data.path()),
        Err(PluginError::MissingManifest { .. })
    ));
}

#[test]
fn a_pack_with_nothing_installable_is_refused() {
    let src = Scratch::new("src7");
    let data = Scratch::new("data7");
    write(
        &src.path().join("manifest.toml"),
        "id = \"empty\"\nname = \"Empty\"\nversion = \"1\"\n",
    );
    write(&src.path().join("readme.rst"), "not allowed");
    // The manifest alone is not content, so this must not install.
    assert!(matches!(
        install(src.path(), data.path()),
        Err(PluginError::Empty)
    ));
}

/// A pack id becomes a directory name. `../../` in one would write
/// outside the plugins directory entirely.
#[test]
fn a_traversing_id_is_refused() {
    for bad in ["../escape", "..", ".", "a/b", "with space", ".hidden", ""] {
        assert!(!is_safe_id(bad), "{bad:?} must be rejected");
    }
    for good in ["uk-extra", "my_pack", "pack.v2", "Pack1"] {
        assert!(is_safe_id(good), "{good:?} should be allowed");
    }
}

#[test]
fn a_traversing_id_in_a_manifest_is_refused_at_install() {
    let src = Scratch::new("src8");
    let data = Scratch::new("data8");
    write(
        &src.path().join("manifest.toml"),
        "id = \"../../escaped\"\nname = \"n\"\nversion = \"1\"\n",
    );
    write(&src.path().join("layout-mappings/x.toml"), "id = \"x\"");
    assert!(matches!(
        install(src.path(), data.path()),
        Err(PluginError::UnsafeId(_))
    ));
    assert!(
        !data.path().join("../../escaped").exists(),
        "nothing may be written outside the plugins directory"
    );
}

#[test]
fn uninstall_refuses_a_traversing_id() {
    let data = Scratch::new("data9");
    assert!(matches!(
        uninstall("../../etc", data.path()),
        Err(PluginError::UnsafeId(_))
    ));
}

/// A symlink named like a content directory must not become a copy of
/// whatever it points at.
#[cfg(unix)]
#[test]
fn symlinks_are_refused_rather_than_followed() {
    let src = Scratch::new("src10");
    let data = Scratch::new("data10");
    let secret = Scratch::new("secret");
    write(&secret.path().join("id_rsa"), "PRIVATE KEY");
    write(
        &src.path().join("manifest.toml"),
        "id = \"sneaky\"\nname = \"n\"\nversion = \"1\"\n",
    );
    let _ = std::os::unix::fs::symlink(secret.path(), src.path().join("layout-mappings"));

    let result = install(src.path(), data.path());
    assert!(
        matches!(result, Err(PluginError::UnsafePath(_))),
        "a symlinked content directory must be refused, got {result:?}"
    );
    assert!(
        !data
            .path()
            .join("plugins/sneaky/layout-mappings/id_rsa")
            .exists(),
        "nothing from the symlink target may be copied"
    );
}

/// A failed install must not leave a staging directory behind, and
/// must not disturb an already-installed pack.
#[test]
fn a_failed_install_leaves_the_previous_pack_intact() {
    let src = Scratch::new("src11");
    let data = Scratch::new("data11");
    make_pack(src.path(), "testpack");
    install(src.path(), data.path()).expect("first install");

    // Now try to install something refused.
    let bad = Scratch::new("bad");
    write(
        &bad.path().join("manifest.toml"),
        "id = \"testpack\"\nname=\"n\"\nversion=\"9\"\n",
    );
    assert!(install(bad.path(), data.path()).is_err());

    let listed = list_installed(data.path());
    assert_eq!(listed.len(), 1, "the good pack must still be listed");
    assert_eq!(listed[0].version, "1.0", "and still be the old version");
    assert!(
        data.path()
            .join("plugins/testpack/layout-mappings/xx_xx.toml")
            .is_file(),
        "its files must be untouched"
    );
}

/// Staging directories are dot-prefixed so the loader and the listing
/// both ignore them; make sure a stray one is not reported as a pack.
#[test]
fn staging_directories_are_not_listed_as_packs() {
    let data = Scratch::new("data12");
    let stray = data.path().join("plugins/.testpack.incoming");
    write(
        &stray.join("manifest.toml"),
        "id = \"testpack\"\nname = \"n\"\nversion = \"1\"\n",
    );
    assert!(list_installed(data.path()).is_empty());
}
