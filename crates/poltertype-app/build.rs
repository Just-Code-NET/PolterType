//! Windows resource embedding: the app icon and a `VERSIONINFO` block.
//!
//! Everything Windows knows about an executable before running it, it
//! reads out of the file's resources. Ours had none — so the Start
//! menu, Explorer, Alt-Tab and the taskbar all drew the shell's
//! placeholder, and the Properties → Details tab was blank. The
//! installer's `ARPPRODUCTICON` covered exactly one surface,
//! Add/Remove Programs, which is the one nobody looks at.
//!
//! The icon is *rendered here*, not read from a checked-in file:
//! `poltertype-icon` draws the same mark the installers use, so the
//! exe and the MSI cannot disagree about what the app looks like.
//!
//! The `VERSIONINFO` block comes along for the ride because it is the
//! same resource script. It is what fills the Details tab, and what an
//! IT department reads when deciding whether an unsigned binary is the
//! one they meant to approve.
//!
//! Nothing here runs on other platforms: macOS takes its icon from the
//! `.app` bundle's `Resources`, and Linux from the `.desktop` entry.

// Build scripts are explicitly allowed to use unwrap/expect/panic per
// the project's style — a panic here is an honest "build is broken"
// signal, not a runtime hazard.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // The *target*, not the host. A build script runs on the machine
    // doing the building; what decides whether an icon resource means
    // anything is the machine that will run the binary. `cfg!` here
    // would answer the wrong question, and would answer it wrongly
    // only when cross-compiling — the case nobody tests.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let icon = out_dir.join("poltertype.ico");
    poltertype_icon::render_ico(&icon).expect("render the app icon");

    let mut res = winresource::WindowsResource::new();
    res.set_icon(icon.to_str().expect("OUT_DIR is valid UTF-8"));
    // Defaults come from this crate's Cargo metadata, which names the
    // *crate* rather than the product. Every string a user can see
    // says PolterType.
    res.set("ProductName", "PolterType");
    res.set("FileDescription", "PolterType — keyboard layout switcher");
    res.set("OriginalFilename", "poltertype.exe");
    res.set("InternalName", "poltertype");
    res.set("CompanyName", "PolterType contributors");
    res.set("LegalCopyright", "PolterType contributors — MIT licensed");
    // Without a manifest Windows runs us DPI-*unaware*, which
    // virtualises every coordinate the OS hands back to 96 DPI and
    // then has DWM stretch whatever we draw. The suggestion tooltip
    // sizes itself from `GetDpiForMonitor` and positions itself in
    // virtual-screen pixels, so the two halves have to be measured in
    // the same units or it lands wrong on any scaled panel. Per-monitor
    // v2 makes both physical; `PerMonitor` covers pre-1607.
    res.set_manifest(MANIFEST);
    res.compile()
        .expect("embed the Windows resources (needs rc.exe from the Windows SDK)");
}

/// Application manifest. `asInvoker` is the default already, but a
/// manifest that omits the block is a manifest an auditor has to
/// second-guess.
const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2,PerMonitor</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>
"#;
