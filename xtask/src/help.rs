//! The `cargo xtask help` listing.

pub(crate) fn print_help() {
    println!("xtask commands:");
    println!("  help                  Show this list.");
    println!("  wordlists fetch       Re-download and re-process the embedded dictionaries.");
    println!("  hooks install         Wire `.githooks/` into this clone (sets core.hooksPath).");
    println!("  hooks uninstall       Unset core.hooksPath (revert to default `.git/hooks/`).");
    println!("  assets icon-png <out> [--size N]");
    println!("                         Render the app icon as a PNG (default size 1024).");
    println!("  assets icon-ico <out>  Render the app icon as a multi-size Windows .ico.");
    println!("                         The exe embeds its own copy at build time; this one");
    println!("                         is for the MSI's Add/Remove Programs entry.");
    println!("  manifest              Sign / verify the release manifest (see `manifest` alone");
    println!("                         for the subcommands). Signing happens on the");
    println!("                         maintainer's machine, never in CI.");
    println!("  style [<path>]        Check the file-organization and platform-split rules of");
    println!("                         CONTRIBUTING.md. A path narrows the report to one crate.");
    println!("  version               Print the current workspace version.");
    println!("  version bump          Bump the workspace version (auto: pre-release counter,");
    println!("                         else patch). Updates Cargo.toml, CHANGELOG.md, Cargo.lock.");
    println!("  version set <X.Y.Z>   Set the workspace version exactly. Same files updated.");
    println!("  version <subcmd> --dry-run   Print what would change without writing.");
}
