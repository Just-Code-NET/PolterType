# kb-switcher

Cross-platform automatic keyboard layout switcher.
Lives in the system tray. Detects when you start typing in the wrong
layout, switches it, and fixes the last word.

> **Status:** v0.1.0-alpha — works end-to-end on Windows; macOS and
> Linux backends are written from API docs and validated on CI but
> haven't yet been runtime-tuned by hardware-equipped contributors.
> See [docs/PLAN.md](docs/PLAN.md) for the full plan and
> [CHANGELOG.md](CHANGELOG.md) for what's in.

## Goals

- **Smart** — language detection per word; pluggable AI detectors for
  power users (off by default).
- **Fast** — pure Rust, no WebView, no perceptible typing latency.
- **Light** — single binary, ~10–15 MB.
- **Quiet** — tray-only, minimal CPU/RAM, **no telemetry, no network**
  (AI subsystem requires a separate explicit toggle).
- **Configurable** — autostart, per-language allowlist, per-app
  exceptions, hotkeys, sound themes.
- **Open source** — MIT licensed.

## Platforms

| OS | Status |
|---|---|
| Windows 10 / 11 | working (primary target for v0.1) |
| macOS 14+ | best-effort; needs Accessibility permission |
| Linux (Wayland) | best-effort; run `scripts/setup-linux.sh` once. Layout switching: Hyprland, KDE Plasma, GSettings (GNOME / Ubuntu Unity / Cinnamon / Budgie / Pantheon / MATE), IBus, Fcitx5. |
| Linux (X11) | layout switching works via the same DE backends; keyboard listener / emitter stubbed (v0.1.x) |

See [docs/PERMISSIONS.md](docs/PERMISSIONS.md) for the per-OS
permissions story.

## Install

Beta builds are published as GitHub Releases —
[**Releases page**](../../releases). Each release ships three
artifacts:

| OS | File | How to install |
|---|---|---|
| Windows 10 / 11 | `kb-switcher-<ver>-x86_64-pc-windows-msvc.msi` | Double-click. Per-user install — no admin rights, no UAC prompt. SmartScreen may show "Windows protected your PC" → **More info** → **Run anyway**. |
| macOS 11+ (Intel + Apple Silicon) | `kb-switcher-<ver>-universal-apple-darwin.dmg` | Open the DMG, drag `kb-switcher.app` into `/Applications`. First launch: right-click the app → **Open** (or run `xattr -dr com.apple.quarantine /Applications/kb-switcher.app`). Then grant Accessibility permission. |
| Linux (x86_64) | `kb-switcher-<ver>-x86_64.AppImage` | `chmod +x` and run. Per-user, no system install. See [docs/PERMISSIONS.md](docs/PERMISSIONS.md) for evdev access on Wayland. |

> Beta builds are **unsigned** — that's why Gatekeeper / SmartScreen
> warn on first launch. Code signing comes in a later phase.

Building from source is documented in
[CONTRIBUTING.md](CONTRIBUTING.md).

## Stack

- Pure Rust — no WebView, no Node.
- `tao` event loop + `tray-icon` + `global-hotkey` + `single-instance`.
- Optional AI subsystem (`feature = "ai"`) — local ONNX or remote LLM
  detectors / word rewriters; off by default.

See [docs/PLAN.md §2](docs/PLAN.md) for the alternatives considered.

## Hotkeys

| Hotkey | Action |
|---|---|
| `Ctrl+Shift+Space` | Pause / resume auto-switching. |
| `Ctrl+Shift+Backspace` | Force-switch the most recent word — ignores every filter, including the dev-friendly skips below. |

## Dev-friendly: stays out of code

If you write code, you *don't* want a layout switcher meddling with
identifiers. Three layers protect you:

* **Per-app skip list** (`[exceptions].disabled_apps` in
  `config.toml`) — covers VS Code / Cursor / JetBrains family /
  Sublime / Zed / Neovide / Windows Terminal / alacritty / kitty /
  wezterm / PowerShell / cmd by default. Edit the list to add or
  remove.
* **Per-token identifier guard** — even outside an IDE, the engine
  doesn't auto-switch on tokens that look like identifiers:
  `snake_case`, `camelCase`, `letter+digit`, or anything containing
  `\\` / `;` / `` ` ``. Toggle via
  `engine.suppress_in_identifiers = false` in `config.toml`.
* **Plausibility-keep** — if the word you typed already reads as
  plausible for the current layout (real letters, sane vowel ratio,
  no ridiculous consonant pile-ups), the engine refuses to switch
  even if the alternate scores higher. This is what keeps `kubectl`,
  `terraform`, `nginx`, surnames, and other "real but uncommon"
  vocabulary from getting auto-corrected to Cyrillic noise.

### Adding your own vocabulary

For specialty words the engine doesn't know yet (project-specific
terms, slang, brand names), drop them into
`<config-dir>/kb-switcher/wordlists/<layout>.txt`:

* Windows: `%APPDATA%\opensource\kb-switcher\config\wordlists\en_us.txt`
* macOS: `~/Library/Application Support/dev.opensource.kb-switcher/wordlists/en_us.txt`
* Linux: `~/.config/kb-switcher/wordlists/en_us.txt`

One lowercase word per line; blank lines and `#`-comments ignored.
Hit "Reload Settings" in the tray and the new words take effect
immediately — no restart needed. Adding to the embedded
`data/wordlists/*.txt` files in the repo, on the other hand, requires
rebuilding the binary (those bake into the FST at compile time).

Writing a comment in another language inside an IDE? Hit
`Ctrl+Shift+Backspace` after the word — that hotkey ignores every
filter by design.

## Settings

There's no GUI in v0.1; the tray "Open Settings (config.toml)…"
entry opens a TOML file in your default editor:

* Windows: `%APPDATA%\opensource\kb-switcher\config\config.toml`
* macOS: `~/Library/Application Support/dev.opensource.kb-switcher/config.toml`
* Linux: `~/.config/kb-switcher/config.toml`

Logs land under the OS data dir; "Open Logs Folder…" in the tray
takes you there. After editing the TOML, "Reload Settings" picks
up the change without restarting.

A full visual UI is on the v0.1.x roadmap — see
[DECISIONS.md, 2026-05-02](docs/DECISIONS.md).

## Building

```bash
# Default
cargo run -p kb-app

# Release
cargo build --release -p kb-app

# With the AI subsystem (architecture only in v0.1)
cargo build --release -p kb-app --features ai
```

[CONTRIBUTING.md](CONTRIBUTING.md) has the per-OS native dep
checklist.

## License

[MIT](LICENSE)
