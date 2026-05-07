# Contributing to kb-switcher

Thanks for the interest! This document covers the practical bits;
the architecture lives in [docs/PLAN.md](docs/PLAN.md) and
[docs/DECISIONS.md](docs/DECISIONS.md).

## Building locally

```bash
# Default build (no AI subsystem)
cargo build -p kb-app

# Run
cargo run -p kb-app

# With the AI subsystem (LocalOnnxDetector + RemoteLlmDetector wiring)
cargo build -p kb-app --features ai

# With AI + actual remote HTTP capability
cargo build -p kb-app --features ai,kb-ai/remote

# Lints (CI runs the same)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## `cargo xtask`

The `xtask` subcommand is wired up via a cargo alias in
[`.cargo/config.toml`](.cargo/config.toml) — no `cargo install` step
needed, the alias is loaded automatically from any directory inside
the workspace. If you previously did `cargo install cargo-xtask`
because of older docs, that's an unrelated stub package — uninstall
it with `cargo uninstall cargo-xtask` so it doesn't shadow our alias.

```bash
cargo xtask help            # list available subcommands
cargo xtask wordlists fetch # re-fetch + Hunspell-expand bundled dictionaries
cargo xtask hooks install   # see below
cargo xtask hooks uninstall
cargo xtask assets icon-png <out> [--size N]   # render the placeholder app icon
```

## Git hooks (one-time per clone)

```bash
cargo xtask hooks install
```

Wires the versioned hooks under [`.githooks/`](.githooks/):

| Hook | Runs | Why |
|---|---|---|
| `pre-commit` | `cargo fmt --all -- --check` | No commits with formatter drift. |
| `pre-push` | `cargo build --workspace --all-targets` | No pushes that don't compile. |

Bypass a single run with `git commit --no-verify` / `git push
--no-verify` if you really need to. Uninstall everything with
`cargo xtask hooks uninstall`.

The hooks mirror the gates CI enforces, so failing locally means
failing on GitHub Actions — better to know in 2 seconds than in
2 minutes after a push.

### Linux native deps

Ubuntu / Debian:

```bash
sudo apt-get install \
    pkg-config libdbus-1-dev libudev-dev \
    libxkbcommon-dev libxkbcommon-x11-dev \
    libwayland-dev libx11-dev libxi-dev libxtst-dev libxdo-dev \
    libgtk-3-dev libayatana-appindicator3-dev libasound2-dev
```

Fedora:

```bash
sudo dnf install \
    pkg-config dbus-devel libudev-devel \
    libxkbcommon-devel libxkbcommon-x11-devel \
    wayland-devel libX11-devel libXi-devel libXtst-devel libxdo-devel \
    gtk3-devel libayatana-appindicator-gtk3-devel alsa-lib-devel
```

Then `scripts/setup-linux.sh` once to grant `/dev/input/event*`
access — see [docs/PERMISSIONS.md](docs/PERMISSIONS.md) for the
rationale.

### macOS

System Settings → Privacy & Security → Accessibility → enable
`kb-switcher` (or your `cargo run` debug binary). The app will fail
to install its CGEventTap until that's granted.

### Windows

Just `cargo run`. SmartScreen may complain about the unsigned
binary; signed releases come in v0.2.

## Project layout

```
crates/
  kb-app/      binary  — tray + event loop + plumbing
  kb-core/    library — engine, settings, layouts, audio
  kb-input/   library — InputListener / KeyEmitter trait + per-OS
  kb-layout/  library — LayoutSwitcher trait + per-OS
  kb-detect/  library — Detector / WordRewriter traits + built-ins
  kb-ai/      library — optional AI plug-ins (feature `ai`)
  kb-types/   library — shared types (LayoutId, KeyEvent, …)
data/
  layout-mappings/  declarative scancode→char tables (TOML)
docs/
  PLAN.md / DECISIONS.md / PERMISSIONS.md / AI.md
installers/         per-platform packaging — see "Releasing" below
  wix/main.wxs              WiX 3.x source for the Windows MSI
  windows/build-msi.ps1     wraps candle.exe + light.exe
  macos/Info.plist.in       template for the .app bundle
  macos/build-dmg.sh        universal-binary .app + .dmg via lipo + hdiutil
  linux/kb-switcher.desktop the AppImage's .desktop entry
  linux/build-appimage.sh   wraps linuxdeploy + appimage plugin
scripts/
  setup-linux.sh — one-time evdev permission grant
```

## Adding a new keyboard layout

1. Drop a TOML into `data/layout-mappings/` named after the BCP-47
   tag (`de_de.toml`, `kk_cyrl_kz.toml`, …). Use one of the existing
   files as a template.
2. Add an `include_str!` entry in
   `crates/kb-core/src/layouts.rs::embedded_layouts()`.
3. Send a PR. No Rust changes are required for the engine to start
   considering the new layout — language-specific code lives in data.

If your language has unusual vowels not covered by
`derive_vowels()` in `crates/kb-core/src/layouts.rs`, extend that
function with a special case.

## Style & guarantees (hard rules)

* `clippy --workspace --all-targets -- -D warnings` must pass.
* No `unwrap()` / `expect()` outside tests, build scripts, or `main`.
* Never log user-typed text in release builds. The word buffer is
  RAM-only and short-lived.
* The OS hook callback never blocks — events go straight onto a
  `crossbeam-channel`; the engine processes them on a worker thread.
* Platform code lives behind `cfg`-gated modules in `kb-input` and
  `kb-layout`. No `#[cfg(target_os = "…")]` outside those crates.

## Commits

Imperative mood, scope prefix when useful (`engine:`, `win:`, `ui:`,
`ai:`). Reference the phase or doc when the change is design-bearing.

## Releasing

Releases are cut by pushing a `v*` tag. CI ([release.yml]) then
builds three installers in parallel and attaches them to a draft
GitHub Release:

| Platform | Artifact | Tooling |
|---|---|---|
| Linux (x86_64) | `.AppImage` | `linuxdeploy` + appimage plugin |
| macOS (universal: Intel + Apple Silicon) | `.dmg` | `lipo` + `hdiutil` |
| Windows (x86_64) | `.msi` | WiX Toolset 3 (`candle` + `light`) |

The packaging logic lives in [`installers/`](installers/) so it can
also be run locally — useful when adjusting the WiX template or the
DMG layout without round-tripping through GitHub Actions:

```bash
# Linux
cargo build --release --target x86_64-unknown-linux-gnu -p kb-app
cargo xtask assets icon-png target/dist/icon-256.png --size 256
VERSION=local ICON_PNG=target/dist/icon-256.png \
    bash installers/linux/build-appimage.sh

# macOS (run on a Mac)
cargo build --release --target x86_64-apple-darwin   -p kb-app
cargo build --release --target aarch64-apple-darwin  -p kb-app
VERSION=local \
    BIN_X86_64=target/x86_64-apple-darwin/release/kb-switcher \
    BIN_ARM64=target/aarch64-apple-darwin/release/kb-switcher \
    bash installers/macos/build-dmg.sh

# Windows
cargo build --release --target x86_64-pc-windows-msvc -p kb-app
choco install wixtoolset --no-progress -y   # one-time
$env:VERSION = 'local'
pwsh installers/windows/build-msi.ps1
```

Beta builds are **unsigned** — we don't yet have an Apple Developer
ID or a Windows EV/OV cert. The release notes call out the
Gatekeeper / SmartScreen workarounds so testers know what to click.

To cut a release:

1. Bump `[workspace.package].version` in the root `Cargo.toml` and
   add a [CHANGELOG.md](CHANGELOG.md) entry.
2. Commit, tag, push:
   ```bash
   git commit -am "release: v0.1.0-alpha.1"
   git tag v0.1.0-alpha.1
   git push origin main --tags
   ```
3. Wait for CI; the Release lands as a **draft** — review, edit
   notes, and publish manually.

[release.yml]: .github/workflows/release.yml

## Reporting bugs / asking for things

GitHub Issues — please attach:

* `kb-switcher --version`
* OS / DE / session type
* If the engine's behaviour is surprising, the relevant lines from
  `<config-dir>/kb-switcher/logs/` (the tray's "Open Logs" entry
  takes you there).
