# Project notes for Claude Code

> Context for the `poltertype` app. Loads whenever a session touches
> files in this repository; workspace-wide rules and the sibling landing
> page live in [../CLAUDE.md](../CLAUDE.md). All paths below are
> relative to this directory.
>
> Authoritative design lives in [docs/PLAN.md](docs/PLAN.md) — read it
> before suggesting architecture changes.

## What this project is

`poltertype` is a cross-platform (Windows / macOS / Linux) tray-only
desktop app that auto-detects when the user is typing in the wrong
keyboard layout, switches the layout, and corrects the last word.

Stack:

- **Pure Rust.** No WebView, no Node, no HTML/CSS toolchain.
- **`iced`** for the (rarely-opened) settings window.
- **`tray-icon`** for the system tray.
- **`global-hotkey`** for global hotkeys.
- **No autostart dependency.** `poltertype-autostart` drives each
  platform's own mechanism directly (see the crate table).
- See [docs/PLAN.md §2](docs/PLAN.md) for the full rationale.

## Workspace layout

| Crate | Purpose |
|---|---|
| `crates/poltertype-app` | binary: tray, settings UI subprocess, global hotkeys, IPC orchestration |
| `crates/poltertype-core` | engine, settings, layouts, smart-commands, wordlist profiles, audio |
| `crates/poltertype-input` | OS keyboard hooks (`InputListener` trait + per-OS), `KeyEmitter`, `KeyGate` (holds keystrokes back during a correction — Linux/evdev only), `FocusTracker` (Windows / Hyprland / X11; on other Wayland the a11y bus answers both caret and focused app — apps with an accessibility bridge only) + `setup` (the permission probe behind the Settings **Setup** pane) |
| `crates/poltertype-layout` | OS layout switching (`LayoutSwitcher` trait + per-OS) |
| `crates/poltertype-detect` | language detector pipeline (heuristic, dictionary, …) |
| `crates/poltertype-update` | GitHub-Releases updater: manifest fetch, checksum-verified download, staging, per-OS install (MSI / DMG / AppImage) |
| `crates/poltertype-popup` | suggestion tooltip: focus-stealing-free overlay (Wayland layer-shell / X11 override-redirect; noop elsewhere) |
| `crates/poltertype-autostart` | run at login: LaunchAgent (macOS), `HKCU` run key (Windows), XDG entry (Linux). No per-OS dependency, `forbid(unsafe_code)`; never calls `launchctl bootout` — see `docs/DECISIONS.md`, 2026-07-30 |
| `crates/poltertype-shell` | per-OS app-shell quirks: `instance_lock_id` (`single-instance` means a path on macOS, a name elsewhere), `keep_out_of_dock`, keycap glyphs, and the two halves of "which application is this window?" on Linux — `window_platform_specific` (the app id iced would otherwise pass as `""`) and `install_desktop_entry` (the `.desktop` + `hicolor` icon a Wayland session takes the window's icon from) |
| `crates/poltertype-tray` | per-OS tray quirks — today only quieting the GTK backend's deprecation warning; the `TrayIcon` itself is still built in the app |
| `crates/poltertype-ai` | optional AI/LLM detectors & rewriters (`feature = "ai"`) |
| `crates/poltertype-types` | shared types (`LayoutId`, `KeyEvent`, …) |
| `crates/poltertype-icon` | the brand mark as geometry: RGBA, PNG and `.ico`. Build-dependency of `poltertype-app` (its `build.rs` embeds the exe's icon resource + `VERSIONINFO`) and of `xtask`; also a runtime dep for the Settings window's icon. No binary asset in the repo — see `docs/DECISIONS.md`, 2026-08-15 |
| `xtask` | dev tooling: `wordlists fetch` (+ Hunspell expand), `hooks install`, `assets icon-png`, `version bump` / `set` |
| `data/layout-mappings/` | TOML files describing keyboard overlays per layout |
| `data/wordlists/` | bundled dictionaries (`<stem>.txt.gz`) + curated `-extras` / `-stop` / `-weak` lists |
| `data/i18n/` | UI translation catalogs (`<lang>.toml`); English lives at the call sites |
| `docs/` | PLAN, DECISIONS, DATA_LAYOUT, PERMISSIONS, AI, ADDING_A_LANGUAGE, TRANSLATING_THE_UI, RELEASING, CODE_SIGNING |

## Hard rules

- **Never log user-typed text** in release builds. Even in debug, log
  only key codes / scancodes. Word buffer is RAM-only and short-lived.
  Enforced since 0.6.3 by `poltertype_types::logsafe::redact_word` —
  every word in a log line or detector reason passes through it and
  renders as `<N chars>`. The only way to see words is a
  `debug_assertions` build with `POLTERTYPE_UNSAFE_LOG_WORDS=1`
  exported (what the self-test recipes use); release builds redact at
  compile time. New logging that touches typed text MUST go through
  this helper.
- **No network calls** unless explicitly designed. Exactly one is:
  the updater (`poltertype-update`) fetches a release manifest from
  GitHub and downloads installers. It sends nothing about the user —
  no body, no query string, no identifier — and `[updates].enabled
  = false` switches it off completely. **This is not telemetry and
  must never become a place to add any.** The AI subsystem is off by
  default; remote AI requires a second explicit toggle
  (`ai.allow_remote = true`).
- **Never install an update while the app is running.** We own a
  global keyboard hook; the binary is replaced on Quit or on an
  explicit "Restart to update", never mid-typing. The background
  worker only downloads, verifies and *stages*.
- **API keys never in plain text.** Use `keyring` (Win Credential
  Manager / macOS Keychain / Secret Service / KWallet).
- **Platform code is isolated** to `poltertype-input`,
  `poltertype-layout`, `poltertype-update`, `poltertype-popup`,
  `poltertype-tray`, `poltertype-autostart` and `poltertype-shell`. No
  `#[cfg(target_os = "...")]` outside those seven crates — which is
  why a one-function GTK quirk got its own crate rather than a
  `#[cfg]` in `main.rs`. **`poltertype-app` and `poltertype-core` hold
  zero**; verify with grep before believing it. Prefer a runtime signal over a build-time one where the choice is a
  value rather than an API: the macOS pause-hotkey default and the
  Wayland switch-last default are both picked off the live backend
  name, which keeps a `config.toml` meaning the same thing wherever it
  is read.
- **Never block the OS hook callback.** Hook handlers must enqueue
  into a `crossbeam-channel` and return immediately. All decision
  logic runs on a worker thread.
- **AI is feature-gated** (`features = ["ai"]`). Default builds must
  compile and run without it.
- **Languages live in data, not code.** Adding a new layout =
  adding a TOML file under `data/layout-mappings/`.

## Common commands

```bash
# Build & run the tray app
cargo run -p poltertype-app

# Release build
cargo build --release -p poltertype-app

# Build with the AI subsystem compiled in. Since 0.10.0 this is a real
# interface with no backend: `[[ai.plugins]]` entries become detectors
# that call whatever endpoint the USER configured. The HTTP client only
# exists with the sub-feature — a stock build has no `reqwest` at all,
# which `cargo tree` will confirm. See docs/AI.md.
cargo build --release -p poltertype-app --features ai
cargo build --release -p poltertype-app --features ai,poltertype-ai/remote

# Open the Settings GUI directly (the tray spawns this as a child
# process when the user clicks "Settings…"; useful in dev to skip
# the tray and see the window)
cargo run -p poltertype-app -- --settings

# Same window, opened on the Setup pane — what the tray's "keyboard
# hooks unavailable" alert spawns. The pane probes the live machine,
# so this is also the quickest way to see what the probe says here.
cargo run -p poltertype-app -- --setup

# Rust checks — run all four before pushing. NOTE: CI does NOT run the
# same set. CI runs fmt + `clippy --locked` (WITHOUT --all-features, so
# the `ai` crate is never linted there) + `test --locked`. `cargo deny`
# is not in CI at all. The pre-commit hook is what covers the gap — so
# if you skip the hook, nothing catches these.
cargo fmt --all
# BOTH clippy runs. CI uses the first (no --all-features), so the
# feature-off shape of an optional crate is only ever checked there —
# which is how the 0.10.0 AI work passed locally and failed on all
# three CI platforms. The pre-commit hook now runs both.
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo deny check

# Dev tooling (cargo xtask)
cargo xtask help                 # full list
cargo xtask wordlists fetch      # re-download + re-expand bundled dicts
cargo xtask hooks install        # wire .githooks/ into this clone
cargo xtask version bump         # release flow — see docs/RELEASING.md
```

## Style

- `rustfmt` defaults; `clippy` strict; no `unwrap()` / `expect()`
  outside tests, build scripts, or `main`.
- Errors: `thiserror` for libraries, `anyhow` only in `poltertype-app::main`.
- Commits: imperative mood, scoped prefix when useful (`engine:`,
  `win:`, `ui:`, `ai:`).
- **One kind of thing per file** (see CONTRIBUTING.md § File
  organization): never mix tests, structs, enums, constants, and free
  functions in one file. Unit tests go in a sibling `tests.rs`
  (`#[cfg(test)] mod tests;`), enums in `enums.rs`, constants in
  `consts.rs`, plain structs in `types.rs`, free functions in
  purpose-named modules; `mod.rs`/`lib.rs` is wiring + re-exports
  only. Examples: `poltertype-core/src/engine/`, `poltertype-core/src/layouts/`,
  `poltertype-detect/src/`, `poltertype-app/src/settings_ui/`.

## Self-testing on Linux (poltertype-app)

Claude is **pre-authorised** to run `poltertype-app` itself when
diagnosing Linux-side issues — no extra confirmation needed. The
session user is in the `input` group, so a plain debug build opens
evdev devices directly (the old `sg input -c` wrapper is gone from
this machine):

```bash
RUST_LOG=poltertype_input=debug,poltertype_layout=debug,poltertype_core=debug \
    cargo run -p poltertype-app 2>&1 | tee /tmp/poltertype.log
```

Run it in the background (Bash `run_in_background: true`) so the loop
keeps running, give it ~5 seconds of real input, then kill the instance
you spawned — `pkill -x poltertype`, never `pkill -f`, which matches the
calling shell — and read `/tmp/poltertype.log`. This authorisation
covers `cargo run` / `cargo build` / `cargo test` for `poltertype-app`
and the kill of the process Claude spawned itself — not push,
force-push, branch deletion, or anything else destructive.

## Decision-making expectations

- Default to **the simplest thing that solves the problem**. Premature
  abstractions and config knobs are worse than a clear conditional.
- Surface trade-offs in PR descriptions, not in code comments.
- When touching OS APIs, link the official doc in the PR.
- The AI/detector pipeline is a deliberate exception — extensibility
  there is a product requirement, not over-engineering.


## Known gaps & deliberate non-goals

The honest-capabilities ledger lives in
**[docs/KNOWN-GAPS.md](docs/KNOWN-GAPS.md)** — what does not work
despite looking like it should, platform by platform, plus what is
deliberately out of scope (unsigned installers, no store submissions,
no telemetry, …). **Read it before promising any capability anywhere**
— README, the website, release notes, issue answers. Re-stamping its
version heading and re-verifying every bullet at every release is a
blocker, not a chore — `docs/RELEASING.md` step 2.
