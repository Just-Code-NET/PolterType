# Changelog

All notable changes to kb-switcher are recorded here. The format is
loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project follows [Semantic Versioning](https://semver.org/).

## [Unreleased] — 0.1.0-beta.11

### Fixed — wordlist edits no longer get silently dropped

Three related ways the Wordlists pane could lose a typed-but-not-
saved edit, all fixed:

* **Footer "Save" didn't save the wordlist.** The bottom-right
  primary-styled "Save" button only wrote `config.toml` —
  wordlist content lived in a separate `text_editor::Content`
  buffer that the per-pane Save (smaller, in the pane footer)
  was responsible for flushing. A user clicking the more
  prominent footer button and then closing the window would
  lose their edit. Footer Save now also flushes any dirty
  wordlist content before writing config.toml.
* **Switching layout / profile / kind dropped unsaved content.**
  Clicking a different layout / profile / kind button used to
  unconditionally re-read the file for the new selection and
  overwrite the editor buffer — silently discarding anything the
  user had typed. The selectors now auto-flush first, with a
  separate "Auto-saved unsaved edit to ..." banner so the user
  understands the side effect.
* **Closing the window dropped unsaved content.** The window's
  X button (or Alt+F4 / Cmd+W) used to take the buffer to the
  grave. Iced's `exit_on_close_request(false)` plus a
  `iced::window::close_requests()` subscription let us intercept
  the close, flush, then close manually.

The actual save logic is now a single `flush_wordlist_to_disk`
helper called by all four paths (per-pane Save, footer Save,
selector switch, window close), so adding new triggers in the
future stays consistent. `WordlistFlushOutcome` carries enough
detail (Nothing / NoLayout / Saved(path) / Failed(msg)) for each
caller to pick banner phrasing that matches what actually
happened — silent for no-op auto-saves, explicit for user-clicked
saves.

### Fixed — wordlist edits via the GUI now apply on window close

Saving a word in the Wordlists pane previously took effect only
after a tray restart, even though the pane's banner said "Saved.
Close this window to apply". The settings-waiter (the worker that
runs when the GUI subprocess exits) reloaded `config.toml` for
the schema parts (`[[commands]]`, `[hotkeys]`, exceptions, profile
defs) but left the engine's dictionary set untouched.

Fix: the close handler now performs three reload steps in
sequence:

1. `config.toml` reload — picks up schema edits (existing).
2. Global wordlist reload — re-reads
   `<config-dir>/kb-switcher/wordlists/<stem>.txt` and atomically
   swaps the engine's dictionary set, same primitive the tray
   "Reload Settings" entry uses.
3. Per-profile cache rebuild + watcher force-reapply — the
   profile cache built at startup is replaced from disk, and a
   new `force_reapply` flag tells the focus-watcher to re-apply
   the currently active profile on its next ~250 ms tick. Without
   this, a user editing a profile's wordlist while focused on a
   matching app would have to alt-tab away and back to see the
   change.

Refactor in `crates/kb-app/src/main.rs`: `profile_dict_cache` now
lives behind `Arc<RwLock<...>>` so the close-handler can rebuild
it without restarting the watcher thread; `spawn_profile_watcher`
takes the cache + force-flag and re-reads on every tick. The
Wordlists pane banner / pane-intro text were updated from
"Restart kb-switcher to apply" to "Close this window to apply" so
the wording matches reality.

### Fixed — manual switch-last hotkey infinite loop

Pressing `Ctrl+Shift+Backspace` (the manual switch-last hotkey)
right after an auto-correction caused an infinite loop: text
accumulating to `wow wow wow…` and the correction sound playing
on a loop until the app was killed.

Root cause: when `apply_correction` sends BACKSPACE keystrokes
via SendInput to delete the typed word, those Backspaces are
flagged INJECTED so the engine itself ignores them. But Win32
`RegisterHotKey` (the primitive `global-hotkey` uses) sees the
*combination* of our injected Backspace + the user's
still-held Ctrl+Shift modifiers as a fresh `Ctrl+Shift+Backspace`
press and fires the hotkey again — running `force_switch_last`
recursively. Same effect from key auto-repeat if the user holds
the chord.

Fix: `EngineCommand::SwitchLastForcefully` now **takes** the
stashed `last_word` atomically (`write().take()`) instead of
cloning it (`read().clone()`). The first fire processes; every
subsequent fire from the same physical hotkey press (or its
echo) finds `None` and exits silently. To re-trigger, the user
must complete another word and let the engine re-stash a new
`last_word`. Pinned by a regression test
(`engine::last_word_consume_tests`).

### Smart commands — text-trigger expansions and shortcuts

Inspired by classic text expanders (TextExpander, Espanso,
AutoHotkey hotstrings): the user types a short token like
`anrl ` (acronym + space), the engine recognises it on the word
boundary, deletes the token + boundary, and runs an action —
typically expanding to a longer phrase.

`config.toml` accepts `[[commands]]` entries:

```toml
[[commands]]
id      = "anrl"
name    = "Anatomical reference list"
trigger = "anrl"
action  = { type = "type_text", text = "Anatomical Reference List" }

[[commands]]
id      = "to-english"
trigger = "((en))"
action  = { type = "switch_layout", layout = "en-US" }

[[commands]]
id      = ";cfg"
trigger = ";cfg"
action  = { type = "open_path", path = "%LOCALAPPDATA%/kb-switcher/config.toml" }
```

Three v1 actions:

* `type_text` — backspace trigger + boundary, emit the literal
  text, re-emit the boundary. So `anrl<space>` → `<expansion><space>`,
  the user's flow continues naturally.
* `switch_layout` — backspace trigger + boundary, switch the OS
  layout to the given BCP-47 id. Same `list_active` pre-flight as
  the corrector — unreachable layouts are rejected loudly.
* `open_path` — backspace trigger + boundary, hand the path to
  `opener::open` (default handler / browser).

Optional `apps = [...]` filter scopes a command to specific
foreground applications using the same case-insensitive basename
match `[exceptions].disabled_apps` already uses.

The trigger lookup runs BEFORE the structural-boundary /
disabled-app / identifier filters: text expansion is direct user
intent, not a guess, so those auto-switch filters don't apply.
That's what makes `=>` snippets work inside an IDE.

A new **Commands** pane in the Settings UI lets users add and
remove commands. Form fields: name, trigger (text input), action
kind (TypeText / SwitchLayout / OpenPath), action param, optional
apps filter. Auto-generates kebab-case ids from the display name;
collisions append `-2`, `-3`, … deterministically.

What v1 deliberately doesn't include:

* `run_shell` — arbitrary command execution. The blast radius
  (a malicious config could mass-exfiltrate) makes this a
  separate security review, queued for later.
* Multi-token triggers (`best regards` → `…`). The buffer resets
  at every word boundary; matching across boundaries needs a
  sliding window we don't have today.
* Case-insensitive / case-preserving expansion. v1 matches
  exactly — pick triggers that don't collide with prose.

### Per-application wordlist profiles

Adds `[wordlists]` to `config.toml`:

```toml
[wordlists]
default_profile = ""

[[wordlists.profiles]]
id     = "code"
name   = "Programming"
apps   = ["Code.exe", "Cursor.exe", "idea64.exe"]

[[wordlists.profiles]]
id     = "writing"
name   = "Long-form prose"
apps   = ["WINWORD.EXE", "obsidian.exe"]
```

Each profile points at its own subdirectory under
`<config-dir>/kb-switcher/wordlists/profiles/<id>/<stem>.txt` (and
`<stem>-stop.txt`). A new background watcher polls
`FocusTracker::focused_exe()` every ~250 ms and atomically swaps
the active dictionary set when the focused app changes — using
the same `DictionaryDetector::replace_dicts` primitive the
"Reload Settings" path already uses.

The Settings UI's **Wordlists** pane now shows a **Profile** row
above the existing Layout / Kind pickers (only when at least one
profile is configured) — pick "Global" or any of your profiles to
edit that profile's overlay files. Profile list management
(add / delete profiles, edit `apps` lists) is queued for a follow-up;
v1 expects users to declare profiles in `config.toml` once, then
edit their wordlists from the GUI.

What v1 deliberately doesn't include:

* Profile inheritance — each profile is its own overlay set, no
  merging. Adds load-time complexity ("which profile wins?")
  without a clear UX win.
* Hot reload — same constraint as the global overlay; profile
  edits apply on tray restart.

### Tooling — `cargo xtask version`

New helper to bump the workspace version in lock-step across
`Cargo.toml`, `CHANGELOG.md` (the `## [Unreleased] — <ver>`
heading), and `Cargo.lock`. Surface:

```bash
cargo xtask version              # print current
cargo xtask version bump         # auto-bump (pre-release counter or patch)
cargo xtask version set X.Y.Z    # exact value
cargo xtask version <subcmd> --dry-run
```

Hand-rolled parser, no `semver` / `regex` deps. Surgical
Cargo.toml edit anchored on `[workspace.package].version` so
dep-pin `version = "..."` entries elsewhere in the file are left
alone. Refuses to write if the file shapes drift — see
`docs/RELEASING.md` for the full release flow.

## [0.1.0-alpha.0 → 0.1.0-beta.6] — pre-release iterations

Pre-release tags up through `v0.1.0-beta.6` (one per merged
batch of work) shipped against this single rolling block while
the project bootstrapped. Per-tag notes weren't kept — the
git log is the authoritative record for which commit landed in
which tag. From `v0.1.0-beta.7` onward, each release gets its
own dated section above.

### Initial scaffolding

The initial scaffolding lands across Phases 0–8 documented in
[docs/PLAN.md](docs/PLAN.md). Highlights:

### Added

* Cargo workspace with seven crates: `kb-app`, `kb-core`,
  `kb-input`, `kb-layout`, `kb-detect`, `kb-ai`, `kb-types`.
* Pure-Rust runtime: `tao` event loop + `tray-icon` + `global-hotkey`
  + `single-instance`. No WebView, no Node.
* SwitcherEngine: scancode-buffer → per-layout render → detector
  pipeline → corrector. Skips events synthesised by our own
  `KeyEmitter` (avoids feedback loops).
* `WordPlausibilityDetector` — vowel-ratio + consonant-cluster
  heuristic. Catches the canonical "wrong-layout" cases for EN ↔ UK.
* Layout mappings in `data/layout-mappings/*.toml`, embedded via
  `include_str!`. EN-US + UK-UA in v0.1.
* Settings stored as TOML at the OS-canonical config path; reload
  from tray notifies the engine without restart.
* File logs via `tracing-appender` (daily rotation) under the OS
  data dir.
* Tray menu: Open Settings (config.toml in default editor) /
  Open Logs Folder / Reload Settings / Pause / About / Quit.
* Global hotkeys: `Ctrl+Shift+Space` (pause), `Ctrl+Shift+Backspace`
  (force-switch the last word).
* AI subsystem scaffold (`kb-ai`, gated by `feature = "ai"`):
  `Detector` + `WordRewriter` plug-in shape, key storage via
  `keyring`, declarative `[[ai.detectors]]` config schema. Concrete
  ONNX/LLM implementations are stubs in v0.1; v0.1.x fills them in.

### Per-OS implementation status

| Platform | Listener | Layout switcher | Emitter |
|---|---|---|---|
| Windows 10 / 11 | `WH_KEYBOARD_LL` (working) | `LoadKeyboardLayout` + `WM_INPUTLANGCHANGEREQUEST` (working) | `SendInput` + `KEYEVENTF_UNICODE` (working) |
| macOS 14+ | `CGEventTap` (best-effort, validated on CI) | Carbon TIS (best-effort) | `CGEventPost` + Unicode string (best-effort) |
| Linux Wayland | `evdev` (best-effort, requires `setup-linux.sh`) | Hyprland / KDE / GSettings (GNOME, Ubuntu Unity, Cinnamon, Budgie, Pantheon, MATE) / IBus / Fcitx5 — probed in that order | `uinput` + Ctrl+Shift+U (best-effort) |
| Linux X11 | stub (v0.1.x) | KDE / GSettings / IBus / Fcitx5 work the same on X11; raw `XkbLockGroup` fallback in v0.1.x | stub (v0.1.x) |

### Documentation

* `docs/PLAN.md` — architecture, roadmap, decisions log.
* `docs/DECISIONS.md` — non-obvious technical choices with reasoning.
* `docs/PERMISSIONS.md` — per-OS access requirements.
* `docs/AI.md` — AI subsystem privacy + plug-in API.

### Real Hunspell-grade dictionaries (~8M inflected forms)

Detection now consults proper per-language dictionaries instead of a
hand-curated 280-word list. Sources (see `data/wordlists/CREDITS.md`):

* **EN**: `dwyl/english-words` — Public Domain — ~370k entries.
* **UK / RU / DE / ES / FR**: LibreOffice Hunspell dictionaries
  (`*.dic` + `*.aff`) — MPL / GPL / etc., per-language.

`xtask/src/hunspell.rs` parses each language's `.aff` rules and
expands every `<stem>/<flags>` entry in the `.dic` into the full
set of inflected surface forms. Coverage per language:

| Lang | Stems  | Surface forms |
|------|-------:|--------------:|
| en   | —      |    370 105    |
| uk   | 350656 |  3 486 848    |
| ru   | 146269 |  1 436 553    |
| de   | 258202 |    789 398    |
| es   |  58221 |    652 463    |
| fr   |  84139 |  2 139 550    |

Storage is a [BurntSushi FST](https://docs.rs/fst) Set built at
compile time from `data/wordlists/<id>.txt` and embedded via
`include_bytes!`. The FST encoding keeps lookup at O(len(word))
with no per-word allocation; the on-disk size grows roughly
linearly with the form count.

User overlay: drop additional words into
`<config-dir>/kb-switcher/wordlists/<id>.txt` to extend any
dictionary with project-specific vocabulary at startup.

Refresh upstream: `cargo xtask wordlists fetch` re-downloads `.dic`
+ `.aff` for each language, re-runs the expander, and writes a
fresh `data/wordlists/<id>.txt`.

### Dev-friendly: keeps quiet in IDEs and on identifiers

Auto-switching skips:

* the foreground app is on `[exceptions].disabled_apps` — defaults
  cover VS Code / Cursor, every JetBrains IDE, Sublime, Zed,
  Neovide, Windows Terminal, alacritty / kitty / wezterm, PowerShell
  / cmd, and friends; case-insensitive basename match.
* the just-finished token looks like a code identifier
  (`snake_case`, `camelCase`, `letter+digit`, or contains code
  punctuation). Acronyms and ordinary capitalised prose are not
  flagged.

Both filters apply to *automatic* decisions only — the manual switch
hotkey `Ctrl+Shift+Backspace` always works, so devs can fix
wrong-layout identifiers or write multi-language comments by
explicitly asking the engine to act.

### Beta installers via GitHub Actions

Pushing a `v*` tag triggers `.github/workflows/release.yml`, which
builds three platform-native installers in parallel and attaches
them to a draft GitHub Release:

* **Windows** — per-user `.msi` via WiX Toolset 3 (no admin needed,
  no UAC prompt). Start menu shortcut, clean uninstall.
* **macOS** — universal-binary `.dmg` (Intel + Apple Silicon merged
  with `lipo`) containing a tray-only `kb-switcher.app` (`LSUIElement`
  set; no Dock icon).
* **Linux** — `.AppImage` (x86_64) built with `linuxdeploy`. Single
  file, no system install.

Beta builds are **unsigned** — Gatekeeper / SmartScreen will warn
on first launch; the release notes call out the per-OS workaround.
Code signing comes in a later phase.

The packaging scripts under `installers/` are also runnable locally;
see [CONTRIBUTING.md §Releasing](CONTRIBUTING.md#releasing).

### Externalised data + lazy load by OS-active

Layout TOMLs and FST wordlists no longer ride inside the binary.
`crates/kb-core/build.rs` writes them to `target/dist/data/`; each
installer copies that tree into the runtime's expected location:

| Platform | Data lives at |
|---|---|
| Windows MSI | `<exe_dir>\data\` |
| macOS .dmg | `kb-switcher.app/Contents/Resources/data/` |
| Linux AppImage | `<mount>/usr/share/kb-switcher/data/` |
| dev (`cargo run`) | `target/dist/data/` |

`kb_core::data_dir::resolve()` finds the live tree at startup. The
app then queries `LayoutSwitcher::list_active()` and loads only the
layouts the OS actually has — a user with `en-US / uk-UA / ru-RU`
saves the FST RAM for the three other bundled languages they'd
never query, and the detector physically can't pick an unreachable
layout (the root cause of the original `http ` bug).

Foundation for the future plug-in / language-pack marketplace —
`<data_dir>/plugins/<pack-id>/` is reserved with the contract
specified in [docs/DATA_LAYOUT.md](docs/DATA_LAYOUT.md). v1's
plug-in surface will be data-only (TOMLs + FSTs); native-code or
network-enabled plug-ins are explicitly out of scope until the
security model has been reviewed.

### Settings UI (iced)

Tray menu **"Settings…"** entry opens a real GUI (iced 0.13 with
the lightweight `tiny-skia` renderer). Six panes:

* **Languages** — checkbox UI over OS-active layouts. Renders the
  *effective* state, so the default (empty allow-list = "use
  every OS layout") shows every box ticked. Un-ticking a box from
  that state materialises the allow-list as "everything except
  this one", preserving the user's intent across save.
* **Hotkeys** — current pause / switch-last bindings + a Rebind
  button per row. Click → "Press a combination…" → the next
  `<modifier>+<key>` combo is captured and written. Lone modifier
  presses are filtered, single-letter combos refused, `Esc`
  cancels. Round-trip through `global-hotkey::HotKey::from_str`
  is unit-tested so the GUI can never produce a combo the next
  tray launch silently drops. `crates/kb-app` now reads bindings
  from `[hotkeys]` in settings (used to be hardcoded).
* **Wordlists** — multiline editor over the per-layout user-overlay
  files in `<config-dir>/kb-switcher/wordlists/<stem>.txt` (Extras)
  and `<stem>-stop.txt` (Stop list). Pick a layout button, pick a
  kind, edit, hit Save — the file is written with a trailing
  newline (matches the bundled convention) and the resolved path
  is shown above the editor so users can verify where the bytes
  land. Changes apply on next tray restart since wordlist FSTs
  are loaded at engine start, not hot-reloaded; the pane spells
  this out so users don't expect live reload.
* **General** — autostart, sound on correction, suppress-in-
  identifiers, idle timeout, plus shortcut buttons to the various
  config / log / wordlist / layout folders.
* **Exceptions** — list-edit for `[exceptions].disabled_apps`.
  One row per entry with a delete `×`, plus an Add field at the
  bottom (Enter or Add-button). Case-insensitive dedup matches
  the engine's runtime comparison.
* **About** — version, repo links, "Reset to defaults" + "Reload
  from disk" escape hatches.

Implementation note: the GUI runs as a child process
(`kb-switcher --settings`) so the tray's `tao::EventLoop` and
iced's `winit` event loop don't fight over the macOS main thread.

### Plug-in loader v1

`<data_dir>/plugins/<pack-id>/` is now enumerated at `LayoutDb`
load. Pack shape: `manifest.toml` + `layout-mappings/*.toml` +
`wordlists/<stem>.fst[+ -stop.txt]`. Precedence chain
`bundled ← plug-ins ← user-overlay` — a user can still override
a plug-in by dropping a TOML with the same id in their config dir.

**v1 surface is data-only** — no native code, no network calls,
no settings injection (see [docs/DATA_LAYOUT.md](docs/DATA_LAYOUT.md)
§ "What plug-ins won't be"). The loader is ~80 LOC, every error
path warns and skips, four unit tests cover happy-path /
missing-manifest / invalid-manifest / user-override.

### Known limitations / v0.1.x targets

* Linux X11 listener / emitter / layout switcher are stubs.
* macOS / Linux backends are written from documentation and only
  validated by `cargo check` on CI; runtime tuning will land as
  contributors with the right hardware report issues.
* Beta builds are unsigned (no Apple Developer ID, no Windows EV /
  OV cert yet) — code signing tracked for a later phase.
* **Hotkey capture on Wayland** — works inside the focused Settings
  window, but Wayland's security model means we don't see global
  key presses while another app has focus. Acceptable for v1 (you'd
  rebind from inside the window anyway), revisited if a use case
  surfaces.
* **Plug-in marketplace UX** — install / sign / update flow is a
  separate phase. The loader is ready; the network + UI plumbing
  has its own security review queued.

[Unreleased]: https://github.com/Just-Code-NET/kb-switcher/compare/v0.1.0-beta.6...HEAD
[0.1.0-alpha.0 → 0.1.0-beta.6]: https://github.com/Just-Code-NET/kb-switcher/releases
