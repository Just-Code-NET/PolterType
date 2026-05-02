# kb-switcher

Cross-platform automatic keyboard layout switcher.
Lives in the system tray. Detects when you start typing in the wrong
layout, switches it, and fixes the last word — optionally with a sound.

> **Status:** very early scaffolding. See [docs/PLAN.md](docs/PLAN.md)
> for the full design and roadmap.

## Goals

- **Smart** — language detection per word; pluggable AI detectors for
  power users (off by default).
- **Fast** — native Rust, no WebView, no perceptible typing latency.
- **Light** — single binary in the ~10–15 MB range.
- **Quiet** — tray-only, minimal CPU/RAM, no telemetry, no network.
- **Configurable** — autostart, per-language allowlist, per-app
  exceptions, hotkeys, sound themes.
- **Open source** — MIT licensed.

## Platforms

| OS | Status |
|---|---|
| Windows 10 / 11 | planned (primary target for v0.1) |
| macOS 14+ | planned |
| Linux (X11) | planned |
| Linux (Wayland) | best-effort — see [docs/PLAN.md](docs/PLAN.md) |

## Stack

- **Pure Rust** — no WebView, no Node, no HTML stack.
- **`iced`** — settings window UI.
- **`tray-icon`** + **`global-hotkey`** + **`auto-launch`**.
- **Optional AI subsystem** (`feature = "ai"`) — local ONNX or remote
  LLM detectors and word rewriters; off by default.

See [docs/PLAN.md §2](docs/PLAN.md) for the full rationale and the
alternatives considered.

## Building

> Not buildable yet — scaffolding only. Build instructions land in
> Phase 1 (see roadmap).

## License

[MIT](LICENSE)
