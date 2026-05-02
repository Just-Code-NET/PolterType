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

### Linux native deps

Ubuntu / Debian:

```bash
sudo apt-get install \
    pkg-config libdbus-1-dev libudev-dev \
    libxkbcommon-dev libxkbcommon-x11-dev \
    libwayland-dev libx11-dev libxi-dev libxtst-dev \
    libgtk-3-dev libayatana-appindicator3-dev libasound2-dev
```

Fedora:

```bash
sudo dnf install \
    pkg-config dbus-devel libudev-devel \
    libxkbcommon-devel libxkbcommon-x11-devel \
    wayland-devel libX11-devel libXi-devel libXtst-devel \
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

## Reporting bugs / asking for things

GitHub Issues — please attach:

* `kb-switcher --version`
* OS / DE / session type
* If the engine's behaviour is surprising, the relevant lines from
  `<config-dir>/kb-switcher/logs/` (the tray's "Open Logs" entry
  takes you there).
