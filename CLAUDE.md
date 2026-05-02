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

## Workspace layout (target — see PLAN §4)

| Crate | Purpose |
|---|---|
| `crates/kb-app` | binary: tray, window, IPC orchestration |
| `crates/kb-core` | engine, settings, focus tracker, audio, autostart |
| `crates/kb-input` | OS keyboard hooks (`InputListener` trait + per-OS) |
| `crates/kb-layout` | OS layout switching (`LayoutSwitcher` trait + per-OS) |
| `crates/kb-detect` | language detector pipeline (heuristic, dictionary, …) |
| `crates/kb-ai` | optional AI/LLM detectors & rewriters (`feature = "ai"`) |
| `crates/kb-types` | shared types (`LayoutId`, `KeyEvent`, …) |
| `data/layout-mappings/` | TOML files describing keyboard overlays per layout |
| `assets/sound-themes/` | sound packs (folder per theme) |
| `docs/` | PLAN, ARCHITECTURE, PERMISSIONS, AI, ADDING_A_LANGUAGE |

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

## Common commands (will exist after Phase 1)

```bash
# build & run the tray app
cargo run -p kb-app

# release build
cargo build --release -p kb-app

# build with AI subsystem enabled
cargo build --release -p kb-app --features ai

# Rust checks
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo deny check
```

## Style

- `rustfmt` defaults; `clippy` strict; no `unwrap()` / `expect()`
  outside tests, build scripts, or `main`.
- Errors: `thiserror` for libraries, `anyhow` only in `kb-app::main`.
- Commits: imperative mood, scoped prefix when useful (`engine:`,
  `win:`, `ui:`, `ai:`).

## Decision-making expectations

- Default to **the simplest thing that solves the problem**. Premature
  abstractions and config knobs are worse than a clear conditional.
- Surface trade-offs in PR descriptions, not in code comments.
- When touching OS APIs, link the official doc in the PR.
- The AI/detector pipeline is a deliberate exception — extensibility
  there is a product requirement, not over-engineering.

## Out of scope for v0.1

- Installers, code signing, store submissions (Microsoft Store, brew,
  winget, AUR) — separate phase.
- Wayland full support (best-effort + clear docs only).
- WASM plugin marketplace.
- Telemetry of any kind.
