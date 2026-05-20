# Project notes for Claude Code

> Always-loaded context for any session in this repository.
> Authoritative design lives in [docs/PLAN.md](docs/PLAN.md) — read it
> before suggesting architecture changes.

## What this project is

`kb-switcher` is a cross-platform (Windows / macOS / Linux) tray-only
desktop app that auto-detects when the user is typing in the wrong
keyboard layout, switches the layout, and corrects the last word.

Stack:

- **Pure Rust.** No WebView, no Node, no HTML/CSS toolchain.
- **`iced`** for the (rarely-opened) settings window.
- **`tray-icon`** for the system tray.
- **`global-hotkey`** for global hotkeys.
- **`auto-launch`** for autostart on login.
- See [docs/PLAN.md §2](docs/PLAN.md) for the full rationale.

## Workspace layout

| Crate | Purpose |
|---|---|
| `crates/kb-app` | binary: tray, settings UI subprocess, global hotkeys, IPC orchestration |
| `crates/kb-core` | engine, settings, layouts, smart-commands, wordlist profiles, focus tracker, audio |
| `crates/kb-input` | OS keyboard hooks (`InputListener` trait + per-OS) |
| `crates/kb-layout` | OS layout switching (`LayoutSwitcher` trait + per-OS) |
| `crates/kb-detect` | language detector pipeline (heuristic, dictionary, …) |
| `crates/kb-ai` | optional AI/LLM detectors & rewriters (`feature = "ai"`) |
| `crates/kb-types` | shared types (`LayoutId`, `KeyEvent`, …) |
| `xtask` | dev tooling: wordlist fetch + Hunspell expand, git-hook install, icon render, `version bump` / `set` |
| `data/layout-mappings/` | TOML files describing keyboard overlays per layout |
| `data/wordlists/` | bundled FST dictionaries + curated short-stop lists |
| `assets/sound-themes/` | sound packs (folder per theme) |
| `docs/` | PLAN, DECISIONS, DATA_LAYOUT, PERMISSIONS, AI, ADDING_A_LANGUAGE, RELEASING |

## Hard rules

- **Never log user-typed text** in release builds. Even in debug, log
  only key codes / scancodes. Word buffer is RAM-only and short-lived.
- **No network calls** unless explicitly designed. AI subsystem is
  off by default; remote AI requires a second explicit toggle
  (`ai.allow_remote = true`).
- **API keys never in plain text.** Use `keyring` (Win Credential
  Manager / macOS Keychain / Secret Service / KWallet).
- **Platform code is isolated** to `kb-input` and `kb-layout`. No
  `#[cfg(target_os = "...")]` outside those crates.
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
cargo run -p kb-app

# Release build
cargo build --release -p kb-app

# Build with AI subsystem enabled (architecture only in v0.1)
cargo build --release -p kb-app --features ai

# Open the Settings GUI directly (the tray spawns this as a child
# process when the user clicks "Settings…"; useful in dev to skip
# the tray and see the window)
cargo run -p kb-app -- --settings

# Rust checks (CI runs the same set; do these before pushing)
cargo fmt --all
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
- Errors: `thiserror` for libraries, `anyhow` only in `kb-app::main`.
- Commits: imperative mood, scoped prefix when useful (`engine:`,
  `win:`, `ui:`, `ai:`).

## Self-testing on Linux (kb-app)

Claude is **pre-authorised** to run `kb-app` itself when diagnosing
Linux-side issues — no extra confirmation needed. The active login
session on this machine often isn't in the `input` group yet (group
is added by `scripts/setup-linux.sh` but the session has to be
re-opened to pick it up), so always wrap the launch in `sg input -c`:

```bash
sg input -c 'RUST_LOG=kb_input=debug,kb_layout=debug,kb_core=debug \
    cargo run -p kb-app 2>&1 | tee /tmp/kb-switcher.log'
```

Run it in the background (Bash `run_in_background: true`) so the loop
keeps running, give it ~5 seconds of real input, then kill it with
`pkill -f 'target/.*/kb-switcher'` and read `/tmp/kb-switcher.log`.
This authorisation covers `cargo run` / `cargo build` / `cargo test`
for `kb-app` and the kill of the process Claude spawned itself — not
push, force-push, branch deletion, or anything else destructive.

## Decision-making expectations

- Default to **the simplest thing that solves the problem**. Premature
  abstractions and config knobs are worse than a clear conditional.
- Surface trade-offs in PR descriptions, not in code comments.
- When touching OS APIs, link the official doc in the PR.
- The AI/detector pipeline is a deliberate exception — extensibility
  there is a product requirement, not over-engineering.

## Out of scope for v0.1

- **Code signing** — beta installers ship UNSIGNED. Apple
  Developer ID + Windows EV/OV cert tracked for a later phase.
  (Per-platform installers themselves *do* exist — see
  `installers/` and `.github/workflows/release.yml`.)
- **Store submissions** — Microsoft Store, Homebrew Cask, winget,
  AUR. Separate phase per store; users install from the GitHub
  Release page in v0.1.
- **Wayland full support** — best-effort + clear docs only.
  Hotkey capture inside the focused Settings window works; global
  hotkey + listener on Wayland sees what `evdev` can give us.
- **Plug-in marketplace UI** — the loader is live (data-only
  packs in `<data_dir>/plugins/<id>/`), but installation /
  signing / updates flow is queued.
- **`run_shell` smart-command action and multi-token triggers** —
  separate security review.
- **WASM plug-in marketplace.**
- **Telemetry of any kind.**
