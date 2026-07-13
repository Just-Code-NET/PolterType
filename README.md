# PolterType

Cross-platform automatic keyboard layout switcher.
Lives in the system tray. Detects when you start typing in the wrong
layout, switches it, and retypes the last word — like a friendly
poltergeist that haunts your keyboard.

> **Status:** v0.3.0 — out of beta since v0.1.0. Works end-to-end on
> Windows and on Linux (both Wayland and X11). The macOS backend is
> written from Apple's API docs and validated on CI, but hasn't yet
> been runtime-tuned by a hardware-equipped contributor. Installers
> are still **unsigned**. See [docs/PLAN.md](docs/PLAN.md) for the
> full plan and [CHANGELOG.md](CHANGELOG.md) for what's in.

![PolterType settings window — Languages panel](docs/screenshots/settings-window.png)

*The settings window (`poltertype --settings`). Day to day the app is
just a tray icon; you open this window only to tweak languages,
hotkeys, smart commands, wordlists, and per-app exceptions.*

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

| OS              | Status                                                                                                                                                                        |
| --------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Windows 10 / 11 | working                                                                                                                                                                       |
| macOS 11+       | best-effort — written from Apple's docs, CI-validated, not yet runtime-tuned on hardware; needs Accessibility permission                                                      |
| Linux (Wayland) | working; run `scripts/setup-linux.sh` once (evdev + uinput access). Layout switching: Hyprland, KDE Plasma, GSettings (GNOME / Ubuntu Unity / Cinnamon / Budgie / Pantheon / MATE), IBus, Fcitx5. |
| Linux (X11)     | working, and needs **no setup script at all** — XInput2 listener + XTest emitter need no `input`-group membership. Layout switching via the DE backends above, falling back to XKB group locking on bare WMs (i3, openbox, …). |

See [docs/PERMISSIONS.md](docs/PERMISSIONS.md) for the per-OS
permissions story.

## Install

Builds are published as GitHub Releases —
[**Releases page**](../../releases). Each release ships three
artifacts:

| OS                                | File                                          | How to install                                                                                                                                                                                                      |
| --------------------------------- | --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Windows 10 / 11                   | `poltertype-<ver>-x86_64-pc-windows-msvc.msi` | Double-click. Per-user install — no admin rights, no UAC prompt. SmartScreen may show "Windows protected your PC" → **More info** → **Run anyway**.                                                                 |
| macOS 11+ (Intel + Apple Silicon) | `poltertype-<ver>-universal-apple-darwin.dmg` | Open the DMG, drag `poltertype.app` into `/Applications`. First launch: right-click the app → **Open** (or run `xattr -dr com.apple.quarantine /Applications/poltertype.app`). Then grant Accessibility permission. |
| Linux (x86_64)                    | `poltertype-<ver>-x86_64.AppImage`            | `chmod +x` and run. Per-user, no system install. See [docs/PERMISSIONS.md](docs/PERMISSIONS.md) for evdev access on Wayland.                                                                                        |

> Installers are still **unsigned** — that's why Gatekeeper /
> SmartScreen warn on first launch. Code signing comes in a later
> phase.

Building from source is documented in
[CONTRIBUTING.md](CONTRIBUTING.md).

### Staying up to date

You only have to do the above once. From then on PolterType keeps
itself current: it checks GitHub for new releases once a day,
downloads the installer for your platform in the background, verifies
its SHA-256 against the release manifest, and then **waits**. Nothing
is installed while you are typing — the swap happens when you quit the
app, or when you click **⟳ Restart to update** in the tray menu.

This is the only network connection PolterType makes. It is a plain
`GET` of a small JSON file: no account, no identifier, nothing about
you or what you type. Turn it off with a checkbox on the Settings
window's **General** pane, or in `config.toml`:

```toml
[updates]
enabled              = false   # never check, never download
check_interval_hours = 24
```

Two caveats worth stating plainly:

- **The download is checksum-verified, not signed.** The checksum
  comes from the same GitHub release as the installer, so it catches a
  corrupted download or a tampered CDN — but not a compromised GitHub
  account. Signing the manifest is planned (see
  [docs/DECISIONS.md](docs/DECISIONS.md)).
- **Only our own installers self-update.** If you installed from a
  distro package, or you're running a `cargo build` binary, PolterType
  won't overwrite it — those files aren't ours. You'll get a
  notification pointing at the Releases page instead.

## Stack

- Pure Rust — no WebView, no Node.
- `tao` event loop + `tray-icon` + `global-hotkey` + `single-instance`.
- Optional AI subsystem (`feature = "ai"`) — local ONNX or remote LLM
  detectors / word rewriters. Off by default, and **not wired to the
  engine yet**: the crate ships stubs, so no build makes network calls
  regardless of the flags. See [docs/AI.md](docs/AI.md).

See [docs/PLAN.md §2](docs/PLAN.md) for the alternatives considered.

## Hotkeys

Two built-in hotkeys, both rebindable on the **Hotkeys** pane of
the Settings window:

| Default                | Action                                                                                            |
| ---------------------- | ------------------------------------------------------------------------------------------------- |
| `Ctrl+Shift+Space`     | Pause / resume auto-switching.                                                                    |
| `Ctrl+Shift+Backspace` | Force-switch the most recent word — ignores every filter, including the dev-friendly skips below. |

## Smart commands (text triggers)

On top of the two built-in hotkeys, you can define `[[commands]]`
entries — short typed tokens that expand or trigger an action when
the engine sees them at a word boundary. The shape mirrors classic
text expanders (TextExpander, Espanso, AutoHotkey hotstrings):

```toml
[[commands]]
id      = "anrl"
trigger = "anrl"
action  = { type = "type_text", text = "Anatomical Reference List" }

[[commands]]
id      = "to-english"
trigger = "((en))"
action  = { type = "switch_layout", layout = "en-US" }

[[commands]]
id      = "open-config"
trigger = ";cfg"
action  = { type = "open_path", path = "C:/Users/me/AppData/Roaming/poltertype/config.toml" }
apps    = ["Code.exe"]
```

Three v1 actions: `type_text` (snippet expansion), `switch_layout`
(BCP-47 id), `open_path` (file or URL). Optional `apps = [...]`
scopes a command to specific foreground apps using the same
basename match `[exceptions].disabled_apps` already uses. Manage
them on the **Commands** pane in Settings.

## Dev-friendly: stays out of code

If you write code, you _don't_ want a layout switcher meddling with
identifiers. Three layers protect you:

- **Per-app skip list** (`[exceptions].disabled_apps` in
  `config.toml`) — covers VS Code / Cursor / JetBrains family /
  Sublime / Zed / Neovide / Windows Terminal / alacritty / kitty /
  wezterm / PowerShell / cmd by default. Edit the list to add or
  remove. **Windows only for now** — see the caveat below.
- **Per-token identifier guard** — even outside an IDE, the engine
  doesn't auto-switch on tokens that look like identifiers:
  `snake_case`, `camelCase`, `letter+digit`, or anything containing
  `\\` / `;` / `` ` ``. Toggle via
  `engine.suppress_in_identifiers = false` in `config.toml`.
- **Plausibility-keep** — if the word you typed already reads as
  plausible for the current layout (real letters, sane vowel ratio,
  no ridiculous consonant pile-ups), the engine refuses to switch
  even if the alternate scores higher. This is what keeps `kubectl`,
  `terraform`, `nginx`, surnames, and other "real but uncommon"
  vocabulary from getting auto-corrected to Cyrillic noise.

> **Anything that keys off the focused app is Windows-only today.**
> Reading which application has focus is implemented for Windows; on
> macOS and Linux the focus tracker is a no-op. So the per-app skip
> list above, the per-app wordlist profiles below, and the `apps =
> [...]` scoping on smart commands all silently do nothing outside
> Windows. The identifier guard and plausibility-keep are pure
> engine logic and work everywhere.

### Adding your own vocabulary

For specialty words the engine doesn't know yet (project-specific
terms, slang, brand names), the easiest path is the **Wordlists**
pane in the Settings window — pick a layout, type words one per
line, hit Save.

The same files live under `<config-dir>/poltertype/wordlists/`
if you'd rather edit them by hand (the Wordlists pane writes to
exactly these locations). The stem is the BCP-47 id with `-`
replaced by `_` (e.g. `en-US` → `en_us.txt`):

- Windows: `%APPDATA%\opensource\poltertype\config\wordlists\en_us.txt`
- macOS: `~/Library/Application Support/dev.opensource.poltertype/wordlists/en_us.txt`
- Linux: `~/.config/poltertype/wordlists/en_us.txt`

One lowercase word per line; blank lines and `#`-comments ignored.
Adding to the bundled `data/wordlists/*.txt` files in the repo,
on the other hand, requires rebuilding the binary (those bake
into the FST at compile time).

**Per-app profiles** — if `kubectl` should count as a real word
inside VS Code but not in chat, declare a `[[wordlists.profiles]]`
entry in `config.toml` and drop the per-profile overlays under
`<config-dir>/poltertype/wordlists/profiles/<id>/<stem>.txt`.
The engine swaps the active overlay set when the focused app
changes. See [docs/DATA_LAYOUT.md](docs/DATA_LAYOUT.md) for the
full schema.

Wordlist edits apply on next tray restart — the FSTs are built
into the engine's dictionary set at start, not hot-reloaded.

Writing a comment in another language inside an IDE? Hit
`Ctrl+Shift+Backspace` after the word — that hotkey ignores every
filter by design.

## Settings

Two ways to configure:

1. **Tray → "Settings…"** opens a real GUI (`iced 0.13` with the
   lightweight `tiny-skia` renderer). Seven panes: **Languages**,
   **Hotkeys**, **Commands** (text-trigger snippet expanders),
   **Wordlists** (per-layout user-overlay editor with optional
   per-app profile picker), **General**, **Exceptions**, **About**.
2. **Tray → "Edit config.toml…"** opens the raw TOML file in your
   default editor — useful for things the GUI doesn't expose yet
   (creating a new wordlist profile entry, listing
   `[[commands]]` in bulk, …):
   - Windows: `%APPDATA%\opensource\poltertype\config\config.toml`
   - macOS: `~/Library/Application Support/dev.opensource.poltertype/config.toml`
   - Linux: `~/.config/poltertype/config.toml`

Logs land under the OS data dir; "Open Logs Folder…" in the tray
takes you there. After editing the TOML, "Reload Settings" picks
up the change for live-reloadable settings (general flags,
exceptions, hotkey bindings) without a restart. Wordlist /
profile changes still need a tray restart — they're built into
the engine's dictionary set at start.

The GUI runs as a child process (`poltertype --settings`) so
the tray's `tao::EventLoop` and iced's `winit` event loop don't
fight over the macOS main thread. Crashes in the UI never bring
down the engine; see [DECISIONS.md](docs/DECISIONS.md) for the
full rationale.

## Building

```bash
# Default
cargo run -p poltertype-app

# Release
cargo build --release -p poltertype-app

# With the AI subsystem compiled in. Note it is not wired to the
# engine yet — the crate ships stubs and no build makes network
# calls. See docs/AI.md.
cargo build --release -p poltertype-app --features ai
```

[CONTRIBUTING.md](CONTRIBUTING.md) has the per-OS native dep
checklist.

## License

[MIT](LICENSE)
