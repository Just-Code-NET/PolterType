# PolterType — Project Plan

> A living roadmap. Updated as implementation proceeds.
> Created: 2026-05-02. Last updated: 2026-08-05 (v0.12.0).

> **How to read this document.** This is a **plan**, not a description
> of the implementation. Wherever the code has diverged from the
> original intent, the code is the source of truth, not this file. The
> freshest summaries:
>
> * **What has shipped** — `CHANGELOG.md` (0.1.0 "First stable" through
>   0.10.0; most recently a user-supplied LLM interface, focused-app
>   tracking on GNOME/KDE via the a11y bus, UI translation, multi-token
>   smart commands with `run_shell`, and plug-in installation) and §10
>   below, where every item is marked.
> * **Why it is this way** — `DECISIONS.md`; several decisions below
>   have since been revisited (most notably "the full GUI is deferred",
>   even though it shipped back in 0.1.0-beta).
> * **What does not exist despite being described below** —
>   `../CLAUDE.md`, the "Known gaps" section: `focused_exe()` is
>   complete on Windows / Hyprland / X11, partial on other Wayland
>   (AT-SPI sees only apps with an accessibility bridge, which
>   excludes most terminals) and absent on macOS; AT-SPI *keystroke
>   listening* is decided against with measurements, and `libei` does
>   not exist. The AI subsystem ships an *interface* and no backend
>   at all as of 0.10.0 — a socket the user points at their own model.
>   Never call it "AI-powered": out of the box nothing answers. The
>   guided
>   onboarding window does exist as of 0.7.0 — but its macOS half has
>   never run on a Mac.
>
> Sections 2–4 in places describe the original intent (dependencies
> that were never adopted; a tray menu that turned out differently).
> Check against the code.

---

## 0. Key decision log

| Date | Decision | Rationale |
|---|---|---|
| 2026-05-02 | Initial take: Tauri 2 + Svelte 5. | Fast start, tray/autostart out of the box. |
| 2026-05-02 | **Pivot: pure Rust, no WebView. UI — `iced` + `tray-icon`.** | The user wants "lower-level and lighter". Smaller binary, no HTML stack, a more cohesive Rust codebase. |
| 2026-05-02 | **Lay out the AI pipeline as a separate subsystem.** | The user plans custom tricks with ML/LLM models. |
| 2026-05-02 | Bundle ID: `dev.opensource.poltertype`. | Fixed by the user. |
| 2026-05-02 | UI MVP: EN + UK. The architecture is multilingual, including "exotic" languages. | Fixed by the user. |
| 2026-05-02 | Sounds: CC0 placeholders → own sounds later. The "sound theme" interface is flexible from the start. | Fixed by the user. |
| 2026-05-02 | Default log level: `info`. | Fixed by the user. |
| 2026-05-02 | v0.1 release channel: GitHub Releases only. | Fixed by the user. |
| 2026-05-02 | **Wayland — in v0.1, as the primary Linux target.** X11 — fallback. | Modern distributions (GNOME/KDE) default to Wayland; the user wants to focus on it. |
| 2026-05-02 | Phase 1 does not enable the `iced` window, only tray + event loop. iced comes in Phase 4. | Less risk in the skeleton; iced is not needed until there is something to show. |
| 2026-05-07 | **Revised:** the full `iced` window ships already in 0.1, not in "Phase 8 / v0.2". Seven panels, a separate `--settings` process. | Event-loop behaviour became clear earlier than expected; a separate process removes the main-thread conflict on macOS. |
| 2026-05-07 | **Revised:** data (layouts, dictionaries) moved out of the binary into `<data_dir>/`, instead of `include_str!`/`include_bytes!`. | Lazy loading keyed on active layouts; user overlays and plugin packs without rebuilding. |
| 2026-05-21 | **v0.1.0 — out of beta** ("First stable"). | The Wayland path (Hyprland + keyd) works reliably on the maintainer's daily machine. |
| 2026-07-11 | **Revised: X11 is not a "fallback" but a first-class path.** The only Linux session type that works **with zero permissions** (no `input` group, no `sudo`, no `setup-linux.sh`). | XInput2 + XTest are available to any client that has opened the display. Ironically — the lowest barrier to entry on all of Linux. |
| 2026-07-11 | **Rename `kb-switcher` → PolterType** — binary, crates, app id, config directory, env var. The old config is adopted automatically on first launch. | The working title had run its course; the migration keeps existing users' settings. |
| 2026-07-11 | **Correction pipeline v2**: switch the layout first, then delete (not the other way around). | Details in `DECISIONS.md` (entry of 2026-07-11): removes the race with the echo from our own emitter. |
| 2026-07-13 | **Linux `FocusTracker`**: Hyprland IPC + X11 EWMH, identity = executable basename via `/proc`, 150 ms TTL cache. GNOME/KDE Wayland stay noop (no compositor-agnostic query). | Closes the "quietest hole" from §3.9 on the two Linux paths that can answer honestly. Details in `DECISIONS.md` (2026-07-13). |
| 2026-07-13 | **Hook-failure alert instead of a silent tray**: menu entry → setup guide, tooltip suffix, one-shot notification. A "Run setup" button that invokes `sudo` itself was rejected. | The listener returned a descriptive error since day one; the tray just never showed it. Self-`sudo` is the scary pattern the docs warn against. |
| 2026-07-31 | **Update manifests are signed by a human, not by CI** (ed25519, key never in Actions). Verification ships one release ahead of enforcement. | An Actions secret is readable by exactly the attacker a signature defends against. Details in `DECISIONS.md` (2026-07-31). |
| 2026-07-31 | **The hook-failure alert opens a Setup pane, not a browser tab.** Four states, including "set up, but this session predates it". Still no self-`sudo` — the button copies the command. | A document cannot tell the user which of the two Linux failure states they are in; a probe can. |
| 2026-07-31 | **No Flatpak, decided rather than left open.** `uinput` is not grantable short of `--device=all`, and no portal exists. | Deciding once with evidence costs less than answering the question every few months. Details in `DECISIONS.md` (2026-07-31). |
| 2026-07-24 | **Spelling suggestions**: dictionary-driven tooltip for mistyped same-layout words (`poltertype-popup` crate, `[suggestions]`, on by default). Below-threshold layout verdicts surface as the leading tooltip entry instead of being dropped. | Extends the correction promise to plain typos with the data we already bundle. Details in `DECISIONS.md` (2026-07-24). |

---

## 1. Product vision

**PolterType** is a cross-platform (Windows / macOS / Linux) background
application that automatically switches the keyboard layout when the
user has started typing a word in the "wrong" layout and, where
possible, corrects the word already typed (optionally with a sound
confirmation).

Target qualities:

- **Smart** — language detection from lexical/orthographic signals,
  later with optional ML/LLM plug-ins. A false switch is enemy #1.
- **Fast** — native code, zero perceptible input latency.
- **Light** — minimal binary, no WebView, no Node runtime.
- **Invisible** — lives in the system tray, minimal CPU/RAM, zero telemetry.
- **Flexible** — configurable language whitelist/blacklist, hotkeys,
  exceptions, autocorrect on/off, optional AI plugins.
- **Open source** — MIT license, repo on GitHub.

Inspiration: Punto Switcher, xneur, Caramba Switcher. We are building
something more modern, safer, opt-in, with no dark patterns.

---

## 2. Technology stack

### 2.1 Why pure Rust + `iced` (no WebView)

Alternatives considered:

| Option | Pros | Cons |
|---|---|---|
| **Rust + `iced`** ✅ | One runtime (Rust), ~10–15 MB binary, no HTML stack. Declarative Elm-style UI. MIT/Apache-2.0. | Less "native" look than OS widgets; requires hand-rolled composition with tray-icon/global-hotkey. |
| Rust + `egui` (eframe) | Fastest prototyping, even smaller binary. | Immediate-mode UI looks more "developer tool"; theming is limited. |
| Rust + Slint | Declarative DSL, very modern look. | Licensing: GPLv3 / Royalty-Free / Commercial — incompatible with a plain-MIT binary. |
| Rust + GTK4 (`gtk4-rs`) | Native look on Linux. | Heavy dependencies on Win/macOS, complicated builds. |
| Tauri 2 (Rust + WebView) | Convenient UI in Svelte/React. | WebView weight, two runtimes, a web stack in the repo (Node, Vite, Tailwind…). The user declined. |
| C++ (Qt) | Maturity. | LGPL/commercial hassle, heavy runtime, another language next to the Rust core. |
| C++ (native OS APIs per OS) | Best performance and native look. | 3× the codebase, high maintenance cost. Not justified for a tray app. |
| Flutter Desktop / Electron / .NET MAUI | Big runtimes, web stacks or .NET. | Does not fit "low-level and light". |

**Conclusion:** `Rust + iced` is the best trade-off of "light, modern,
one runtime, MIT". We integrate by hand with `tray-icon`,
`global-hotkey`, `auto-launch`. If during implementation the
settings-window UX requirements turn out to be too much for `iced`,
the planned fallback is `egui` (same weight class, even simpler
integration).

### 2.2 Key dependencies (preliminary list)

| Crate | Purpose |
|---|---|
| `iced` 0.13+ | settings window UI |
| `tray-icon` | system tray (Win/Mac/Linux) |
| `global-hotkey` | global hotkeys |
| ~~`auto-launch`~~ | run at login — **dropped in 0.6.2**. It sat in the manifest unused for the project's whole life while the Settings checkbox quietly did nothing. Replaced by `poltertype-autostart`, which drives each platform's own mechanism directly (LaunchAgent / run key / XDG entry) and needs no dependency at all. |
| `single-instance` | forbid a second process |
| `tao` *(optional)* | shared event loop for tray + hotkeys + iced |
| `tokio` | async runtime, channels, timers |
| `serde` / `serde_json` / `toml` | settings serialization |
| `directories` | OS-specific paths (`~/.config`, `%APPDATA%`, ...) |
| `keyring` | secure storage for optional API keys |
| `tracing` + `tracing-subscriber` + `tracing-appender` | logging |
| `parking_lot` | fast mutexes |
| `crossbeam-channel` | lock-free queues between the hook thread and the engine |
| `lingua` *or* a home-grown n-gram | basic per-word language detection |
| `unicode-normalization` | NFC/NFD for correct comparison |
| `rodio` | WAV/OGG sound playback |
| `notify-rust` | system notifications (optional) |
| **Win:** `windows` (official Microsoft bindings) | `WH_KEYBOARD_LL`, `LoadKeyboardLayoutW`, `SendInput`, `GetForegroundWindow`. |
| **macOS:** `core-graphics`, `core-foundation`, `objc2`, `objc2-app-kit` | `CGEventTap`, TIS API, `NSWorkspace`. |
| **Linux:** `x11rb`, `xkbcommon`, `evdev`, `ashpd` *(XDG portals)*, `libei` *(later)* | XInput2, XKB, Wayland compatibility. |
| **AI subsystem (optional):** `ort` *(ONNX Runtime)* or `candle-core` | local models. |
| **AI subsystem (optional):** `reqwest` + `eventsource-stream` | remote APIs (LLM). |

### 2.3 Things we can live without, but nice to have

- The `xtask` pattern for auxiliary build scripts instead of a `Makefile`.
- `cargo-deny` to check dependency licenses (important for an MIT project).
- `cargo-dist` or a hand-written release.yml for cross-building artifacts.

---

## 3. Architecture

```
┌───────────────────────────────────────────────────────────┐
│  Main thread (event loop: tao)                            │
│  ┌────────────┐ ┌─────────────┐ ┌──────────────────────┐  │
│  │ TrayIcon   │ │GlobalHotkey │ │  iced Settings Window │  │
│  └────────────┘ └─────────────┘ │  (opened on demand)   │  │
│                                  └──────────────────────┘  │
└───────────────┬───────────────────────────────────────────┘
                │ (cmd channel)
┌───────────────▼───────────────────────────────────────────┐
│  CoreService (async, tokio)                                │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────────┐   │
│  │SettingsStore │ │ AudioPlayer  │ │ FocusTracker     │   │
│  └──────────────┘ └──────────────┘ └──────────────────┘   │
│                                                            │
│  ┌──────────────────────────────────────────────────────┐ │
│  │              SwitcherEngine  (state machine)          │ │
│  │   buffer ─► detector pipeline ─► decision policy ─►   │ │
│  │   ─► corrector                                        │ │
│  └──────┬─────────────────────────────────┬──────────────┘ │
│         │                                 │                │
│  ┌──────▼─────────┐               ┌──────▼─────────┐       │
│  │ InputListener  │               │ LayoutSwitcher │       │
│  │ (per-OS trait) │               │ (per-OS trait) │       │
│  └────────────────┘               └────────────────┘       │
└───────────────────────────────────────────────────────────┘

Detector pipeline (modular, see §3.4):
  HeuristicDetector  →  DictionaryDetector  →  [LocalMlDetector]
                                            →  [RemoteLlmDetector]
```

### 3.1 InputListener (global keyboard hook)

```rust
pub trait InputListener: Send + 'static {
    fn start(&mut self, events: crossbeam_channel::Sender<KeyEvent>) -> Result<()>;
    fn stop(&mut self);
}
```

Implementations:

- **Windows** — `SetWindowsHookExW(WH_KEYBOARD_LL, ...)` on a dedicated
  thread with a `GetMessageW` loop. Always return `CallNextHookEx`
  (we block nothing). Word correction goes through `SendInput`.
- **macOS** — `CGEventTapCreate(kCGSessionEventTap, ..., listenOnly)`,
  a tap on the CFRunLoop of a dedicated thread. Requires the
  Accessibility permission — a first-launch onboarding is a must.
- **Linux Wayland (primary target)** — Wayland by design offers no
  global keylogging API; the realistic paths are:
  1. **evdev** via `/dev/input/event*` (the `evdev` crate). Requires
     the user to be a member of the `input` group + udev rules. Set up
     at install time by the `setup-linux.sh` script (a single `sudo`
     invocation, with a clear UI warning). Works in all Wayland
     compositors (GNOME, KDE, Hyprland, Sway).
  2. **AT-SPI** via the `atspi` crate as a fallback when the user
     declined the `input` group and has the accessibility bus enabled
     (GNOME — by default, KDE — optionally). Less reliable and slower,
     but no `sudo`.
  3. **`libei`** (the `reis` crate) — for synthesizing key presses
     (word correction) through the `org.freedesktop.portal.RemoteDesktop`/
     `InputCapture` portal on KDE 6.0+ / GNOME 46+. This is a separate
     path for send-keys, not listen-keys.

  Fallback strategy: at startup, detect `XDG_SESSION_TYPE` and the
  effective permissions; if nothing is available — the tray still runs
  but shows a "keyboard hooks unavailable, see Setup" banner.
- **Linux X11** — `XInput2 RawKeyPress` via `x11rb`. Non-blocking.
  Kept as a fallback for X11 sessions.

### 3.2 LayoutSwitcher (layout switching)

| OS | API |
|---|---|
| Windows | `LoadKeyboardLayoutW` + `PostMessageW(HWND_BROADCAST, WM_INPUTLANGCHANGEREQUEST, ...)` or `ActivateKeyboardLayout`. |
| macOS | `TISCreateInputSourceList` → `TISSelectInputSource`. |
| Linux Wayland | Probe in order: Hyprland (`hyprctl`), KDE (`qdbus`), GSettings (`gsettings org.gnome.desktop.input-sources` — GNOME/Unity/Cinnamon/Budgie/Pantheon/MATE), IBus (`ibus engine`), Fcitx5 (`fcitx5-remote`). Every probe is a real CLI/schema check, not just an env guess. |
| Linux X11 | `XkbLockGroup` via `x11rb` (fast) or a `setxkbmap -layout ...` fallback. |

### 3.3 SwitcherEngine (logic)

States:

1. `Idle` — no active word.
2. `Buffering` — the user is typing a word; we collect `(scancode,
   vk, shift_state, current_layout, timestamp)`.
3. `Decide` — a separator fired (Space, Enter, Tab, punctuation).
   - Convert the buffer to text in every candidate layout
     (overlay maps: EN↔UK, EN↔RU, EN↔DE, …).
   - Run every variant through the **detector pipeline** (§3.4).
   - Compare confidence; if an alternative layout scores significantly
     higher (configurable threshold) — and the corresponding language
     is among the user's active layouts — accept the decision.
4. `Correct` — with the user's consent (an option):
   - `len(buffer)` × `BackSpace` via `SendInput` / `CGEventPost` /
     `XTestFakeKeyEvent`.
   - Switch the layout.
   - Send the corrected text (via unicode input or a keydown/up
     sequence according to the map).
   - Play the sound.
5. **Hotkey Pause/Resume**, **Manual switch-last** (Ctrl+Shift+Backspace,
   Punto-style): converts the previous word manually.

Triggers that reset the buffer:

- Window focus change.
- The pause hotkey.
- The user switched the layout themselves.
- Buffer-editing input (Ctrl+Z, Ctrl+A, mouse).
- Idle timeout (e.g. 2 s).

### 3.4 Detector pipeline (ready for many languages and AI)

**This is the main seam where flexibility plugs in.** One trait with
several implementations, executed sequentially or in parallel.

```rust
pub struct DetectionInput<'a> {
    pub raw_buffer: &'a [KeyStroke],
    pub current_layout: LayoutId,
    pub candidate_layouts: &'a [LayoutId],
    pub recent_context: &'a str,        // previous N words
    pub focused_app: Option<&'a AppId>, // for context
}

pub struct DetectionVerdict {
    pub best_layout: LayoutId,
    pub confidence: f32,           // 0.0–1.0
    pub reason: VerdictReason,     // for the "why we switched" UI
}

pub trait Detector: Send + Sync {
    fn name(&self) -> &'static str;
    fn detect(&self, input: &DetectionInput<'_>) -> Option<DetectionVerdict>;
}
```

Implementations:

> ⚠️ The trait signature above is from the original design. **In the
> code it is different:** `fn judge(&self, ctx: &DetectionContext<'_>)
> -> Verdict`, where `Verdict` is three-valued (`NoOpinion` /
> `Keep { reason }` / `Switch`). It is `Keep` that lets the dictionary
> say "this is a real word, don't touch it" — the main safeguard
> against false positives.

| Detector | Purpose | Status |
|---|---|---|
| `WordPlausibilityDetector` (planned as `HeuristicDetector`) | fast rules: does the word look plausible for the current layout (letters, vowel ratio, consonant clusters). | ✅ in 0.1 |
| `DictionaryDetector` | an FST dictionary over Hunspell-expanded lists. `lingua-rs` and n-grams were **not used** — dropped in favour of FST. | ✅ in 0.1 |
| `ContextDetector` | takes the previous N words into account (Markov model). | ❌ missing (was planned for v0.2 — not done) |
| `LocalOnnxDetector` | an ONNX model, offline. | ❌ removed in 0.10.0 — no in-process model; "local" now means `LlmDetector` pointed at a server on your own machine (Ollama, llama.cpp) |
| `LlmDetector` (absorbed the planned `RemoteLlmDetector`) | one detector speaking `openai-chat` / `anthropic-messages` / `ollama-generate` to an endpoint the **user** names; loopback vs remote decided in `locality`, remote additionally gated by `[ai].allow_remote`. | ✅ real since 0.10.0; compiled into the installers since 0.12.0; builds nothing unless `[[ai.plugins]]` is configured |

Pipeline policy (example):

```text
1. HeuristicDetector — if confidence > 0.95, accept.
2. DictionaryDetector — runs for all candidate layouts;
   accept if delta(best, current) > threshold.
3. (opt.) LocalMlDetector — invoked only when (1)+(2) produced
   confidence < threshold and enabling it is allowed.
4. (opt.) RemoteLlmDetector — entirely outside the default; enabled in
   settings for "hard" fields (research, multi-lingual writing).
```

**Multilingual support:**

- A layout is described by a `LayoutId` (BCP-47-like, e.g. `uk-UA`,
  `en-US`, `de-DE`, `kk-Cyrl-KZ`, `hy-AM`).
- The overlay map is described by data in
  `src/layout/mappings/<id>.toml` (not by Rust code), so adding a new
  language = adding a file.
- Detectors are written language-agnostic; language knowledge lives in
  the data.
- In the settings the user sees two columns: "available in the system"
  and "active for PolterType".

### 3.5 Settings storage

`TOML` via `serde` (user-readable). Paths come from
`ProjectDirs::from("dev", "opensource", "poltertype")`, so they are
qualifier-scoped rather than bare:

- Win: `%APPDATA%\opensource\poltertype\config\config.toml`
- macOS: `~/Library/Application Support/dev.opensource.poltertype/config.toml`
- Linux: `~/.config/poltertype/config.toml`

Structure — **this mirrors the shipped defaults**; keep it that way,
it is the only schema listing in `docs/`:

```toml
schema_version = 1

[general]
autostart = true           # honoured on all three platforms since
                           # 0.6.2 (LaunchAgent / HKCU run key / XDG
                           # autostart entry)
sound_on_correct = true
show_notifications = false
ui_language = "system"     # or "en", "uk"
ui_theme = "system"        # Settings window theme: "system", "light", "dark"
log_level = "info"

[languages]
active   = ["en-US", "uk-UA"]
ignored  = []

[engine]
min_word_length = 3
confidence_threshold = 0.55
ignore_in_password_fields = true
idle_timeout_ms = 2000
suppress_in_identifiers = true   # skip snake_case / camelCase / letter+digit
suppress_for_all_caps = true     # skip URL, HTTP, API, ССЫЛКА…

[exceptions]
# EMPTY by default, deliberately — see DECISIONS.md, "Reversed: no
# default app skip-list". Shipping a list of editors and terminals
# made the app look dead in exactly the windows developers type in.
disabled_apps    = []
word_whitelist   = ["nginx", "kubectl", "github"]

[hotkeys]
pause_toggle        = "Ctrl+Shift+Space"   # macOS: Ctrl+Shift+P
manual_switch_last  = "Ctrl+Shift+Backspace"   # Wayland: Ctrl+Shift+F9

# Text-trigger expansions. Not exposed as a table above — repeated
# [[commands]] entries; see §3.9.
[[commands]]
id      = "to-english"
trigger = "((en))"
action  = { type = "switch_layout", layout = "en-US" }

# Per-app wordlist overlays, keyed off the focused app (so: Windows,
# Hyprland and X11 only — inert on macOS and GNOME/KDE Wayland).
[wordlists]
default_profile = ""       # profile used when no `apps` list matches;
                           # "" means global overlay files only

[[wordlists.profiles]]
id   = "code"
apps = ["Code.exe", "kitty"]

[sounds]
theme = "default"          # preset folder
volume = 0.6

# Spelling-suggestion tooltip for mistyped (same-layout) words —
# added 2026-07-24. Purely local: candidates come from the bundled
# dictionaries (a second, surface-form FST per language), ranked by a
# keyboard-aware edit distance. The tooltip never takes keyboard
# focus; entries are applied by click or by accept_modifiers+digit.
[suggestions]
enabled              = true
max_suggestions      = 5            # clamped to 1..=9 (one digit key each)
tooltip_timeout_secs = 30           # clamped to 3..=600 at read time
accept_modifiers     = "Ctrl+Shift" # + digit 1..9; "" disables keyboard accept

# The app's ONLY network access, and it is ON by default. Added in
# v0.4.0. See §6, DECISIONS.md and docs/PERMISSIONS.md § Network.
[updates]
enabled              = true
check_interval_hours = 24   # floor of 1 is enforced in code; 0 means hourly, NOT off

[ai]
enabled      = false
allow_remote = false   # a second switch: even enabled, no network without it

# Read since 0.8.0: each entry becomes a Detector appended to the
# pipeline. Since 0.10.0 that detector is real (docs/AI.md) and since
# 0.12.0 it ships in the installers; with no entries — the default —
# nothing is built and nothing is called.
[[ai.plugins]]
type        = "remote-llm"
id          = "claude"
provider    = "anthropic"
model       = "claude-sonnet-4"
api_key_ref = "keyring:anthropic"   # never the key itself — refused if it is
```

API keys are **not stored** in `config.toml` — only a reference to an
entry in the system keychain via `keyring`.

### 3.6 Tray UX

The icon shows the current layout (a two-letter code, EN/UK/...) as a
generated PNG/ICO (optionally composed natively via `tiny-skia`).

The menu — **as it settled in the code** (the original sketch with a
quick-switch submenu and a "Today: N corrections" counter was **not**
built):

- ⚠ Keyboard hooks unavailable — Setup Guide… *(only when the hooks
  failed to start; opens `docs/PERMISSIONS.md`)*
- ⚙ Settings…
- 📝 Edit config.toml…
- 🪵 Open Logs Folder…
- 📖 Open User Wordlists Folder…
- ⌨ Open User Layouts Folder…
- 🔄 Reload Settings
- ⏸ Pause auto-switch
- ⟳ Check for updates… *(absent when `[updates].enabled = false`;
  becomes "⟳ Restart to update — vX.Y.Z" once a version is staged)*
- ℹ About …
- ❌ Quit

### 3.7 Sounds (sound themes)

- **By default sounds are synthesized** — `AudioPlayer` generates a
  tone on the fly (a different pitch per event). This keeps the binary
  small and avoids per-platform decoder hassle. There is **no**
  `assets/` directory in the repository, and no bundled `default/`
  theme either.
- User theme: `<config-dir>/sound-themes/<theme>/<event>.ogg`.
  Events are `correct`, `pause`, `resume` (not `{correct,pause,switch,
  error}` as originally planned).
- If a theme file is missing, we silently fall back to the synthesized
  tone — no crash.

### 3.8 AI / ML subsystem (opt-in)

**Design goal:** add AI models as a separate, isolated subsystem that
is disabled by default. Zero impact on the core path until the user
explicitly turns it on.

The architecture rests on two seams:

#### A. Detector plugins (already described in §3.4)

A `Box<dyn Detector>` is appended to the pipeline. Implementations:

- `LocalOnnxDetector { model_path, runtime_threads }` — inference via
  `ort`/`tract`/`candle`. Works fully offline.
- `RemoteLlmDetector { provider, model, api_key_ref, max_latency_ms }` —
  an HTTP request with the context and the candidates; providers:
  `openai`, `anthropic`, `ollama` (local, but "remote" by schema),
  `custom-openai-compatible`.

#### B. Word rewriter (new custom "tricks")

A separate seam "after" the detector — transforms even a correctly
typed word when the AI knows the user meant something else. Intended
uses: "auto-capitalization", "acronym expansion", "slang → formal
replacement".

```rust
pub struct RewriteRequest<'a> {
    pub original: &'a str,
    pub layout: LayoutId,
    pub recent_context: &'a str,
}

pub enum RewriteVerdict {
    Keep,
    Replace { text: String, reason: String, requires_confirmation: bool },
}

pub trait WordRewriter: Send + Sync {
    fn name(&self) -> &'static str;
    fn rewrite(&self, req: &RewriteRequest<'_>) -> RewriteVerdict;
}
```

The rewriter always **confirms the operation** through a flow
mirroring text correction. Disabled by default.

#### C. Example config

```toml
[ai]
enabled = true
allow_remote = false   # a separate checkbox: "allow AI network calls"

# NOTE: `[[ai.plugins]]`, not `[[ai.detectors]]` — the implemented name.
# `default_pipeline` was in the original design and does not exist:
# plug-ins are appended to the built-in detectors, never reordered
# against them.
[[ai.plugins]]
type = "local-onnx"
id   = "fasttext-lid-176"
model_path = "models/lid.176.onnx"
threads = 1
weight = 1.0
min_text_length = 4

[[ai.plugins]]
type = "remote-llm"
id   = "anthropic-haiku"
provider = "anthropic"
model = "claude-haiku-4-5-20251001"
api_key_ref = "keyring:anthropic"   # the key lives in the system keychain
max_latency_ms = 600
trigger_when_dictionary_below = 0.5
weight = 0.7

[[ai.rewriters]]
type = "remote-llm"
id   = "smart-capitalize"
provider = "openai"
model = "gpt-4o-mini"
api_key_ref = "keyring:openai"
prompt_template = "rewriters/smart_capitalize.tmpl"
require_confirmation = true
```

#### D. Privacy guarantees

> As of v0.12.0 the subsystem is real and ships in the installers
> (docs/AI.md): with an endpoint configured it asks one question per
> newly-seen ambiguous word — candidates and a fixed instruction, no
> sentence, no document, no layout ids. With nothing configured — the
> default — no detectors are built and no request is ever made, so
> everything below is enforced behaviour, not aspiration. The
> tray-tooltip indicator and the call counter **do not exist** — the
> tooltip shows only the name, the layout and "(paused)".

- AI is disabled by default.
- A separate `allow_remote` toggle — even with `enabled=true` the
  network must stay blocked until the user explicitly switches it on.
  (Today the flag is parsed, but no code reads it.)
- Every remote call must be reflected in the tray tooltip — an
  "AI: on/off, remote: yes/no" indicator and an "N AI calls today"
  counter — **not implemented**.
- API keys — via `keyring`, never in plain text. (The helper is
  written; nothing calls it yet.)
- Cache LLM responses by hash(input) — never send identical words
  twice.

#### E. Dynamic plugin loading (distant plan)

Initially all detectors/rewriters are compiled into the binary. If a
third-party marketplace is ever needed — `wasmtime` for WASM plugins.
Not in the MVP.

### 3.9 FocusTracker (application context)

- Win: foreground-process query (`GetForegroundWindow` →
  `QueryFullProcessImageNameW`) — **✅ implemented**.
- Linux Hyprland: `activewindow` over the IPC socket — **✅
  implemented** (2026-07-13).
- Linux X11: `_NET_ACTIVE_WINDOW` + `_NET_WM_PID` (EWMH) — **✅
  implemented** (2026-07-13).
- macOS: `NSWorkspace.didActivateApplicationNotification` — **❌ no**.
- Linux GNOME/KDE Wayland — **❌ no**: there is no compositor-agnostic
  active-window query, by design; needs per-DE backends (KWin script /
  GNOME shell extension).

> **Remaining hole (macOS + GNOME/KDE Wayland).**
> `create_focus_tracker()` returns a `NoopFocusTracker` there, and its
> `focused_exe()` always returns `None`. So everything keyed off the
> active application **silently does nothing** on those targets:
> `[exceptions].disabled_apps`, per-app dictionary profiles, and
> `apps = [...]` on smart commands. There will be no error — simply
> nothing will happen. Do not promise these features there (in
> particular on the landing page) until the trackers exist.
>
> Design notes for the Linux backends (identity = executable basename
> via `/proc/<pid>/exe`, 150 ms TTL cache, why XWayland is not used as
> a fallback) — `DECISIONS.md`, entry of 2026-07-13.

The tracker answers "executable basename of the focused window"
(`focused_exe()`, the planned `AppId { exe_name, window_title }` was
trimmed to just the name) for:

- per-app exceptions;
- per-app wordlist profiles and smart-command scoping;
- log metadata.

Buffer reset on focus switch (listed in §3.3) ended up driven by
pointer clicks and idle timeouts instead of focus transitions — the
engine only *pulls* focus state at word boundaries.

---

## 4. Repository structure

The actual structure as of v0.2.0 (the original sketch diverged from
it in several places: `assets/` and a root `tests/` do not exist,
modules are split into "one entity — one file" directories, and
`CONTRIBUTING.md` lives at the root, not in `docs/`):

```
poltertype/                      # (the Claude config is not here — it's
│                                #  at the workspace root: ../.claude/)
├── .cargo/config.toml           # the `cargo xtask` alias
├── .github/workflows/{ci.yml,release.yml}
├── .githooks/                   # pre-commit / pre-push (installed by xtask)
├── docs/
│   ├── PLAN.md                  # this file
│   ├── DECISIONS.md             # architecture decision log
│   ├── DATA_LAYOUT.md           # the on-disk data tree + plugins
│   ├── PERMISSIONS.md           # macOS Accessibility, Linux evdev/X11
│   ├── AI.md                    # state and design of the AI subsystem
│   ├── ADDING_A_LANGUAGE.md
│   └── RELEASING.md
├── crates/
│   ├── poltertype-app/          # binary: tray, Settings UI (separate process)
│   │   └── src/{main.rs, tray.rs, detectors.rs, updater.rs, bridges.rs,
│   │            hotkeys.rs, user_dirs.rs, settings_ui/, settings_proc/, icon_render/}
│   ├── poltertype-core/         # engine, settings, layouts, commands, audio
│   │   └── src/{engine/, settings/, layouts/, commands/, wordlist_profiles/, audio/, data_dir/}
│   │       └── build.rs         # prepares target/dist/data out of data/
│   ├── poltertype-input/        # InputListener + KeyEmitter + FocusTracker
│   │   └── src/{windows/, macos/, linux/{wayland,x11}/, focus/}
│   ├── poltertype-layout/       # LayoutSwitcher + per-OS backends
│   │   └── src/{windows/, macos/, linux/{hyprland,kde,gsettings,ibus,fcitx,x11}/}
│   ├── poltertype-detect/       # Detector pipeline
│   │   └── src/{traits.rs, plausibility.rs, dictionary.rs, enums.rs}
│   ├── poltertype-update/       # GitHub-Releases updater (v0.4.0+); the
│   │   │                        # only network code in a stock source build
│   │   └── src/{check.rs, manifest.rs, download.rs, staging.rs, version.rs, apply/}
│   ├── poltertype-ai/           # feature `ai`; real LlmDetector since 0.10.0,
│   │   │                        # compiled into the installers since 0.12.0
│   │   └── src/{factory.rs, detector.rs, wire.rs, transport.rs, locality.rs, cache.rs, keys.rs, ...}
│   └── poltertype-types/        # shared types (LayoutId, KeyEvent, ...)
├── data/                        # source of truth, consumed by build.rs
│   ├── layout-mappings/         # TOML overlays (en_us.toml, uk_ua.toml, ...)
│   └── wordlists/               # <stem>.txt.gz + -extras/-stop/-weak
├── installers/{wix,windows,macos,linux}/
├── scripts/setup-linux.sh
├── xtask/                       # wordlists fetch, hooks install, icon, version
├── Cargo.toml                   # workspace
├── CHANGELOG.md
├── CONTRIBUTING.md
├── CLAUDE.md
├── LICENSE                      # MIT
└── README.md
```

There are no integration tests in a root `tests/` — unit tests live in
sibling `tests.rs` files inside each module (see CONTRIBUTING.md, the
section on file organization).

A workspace of several crates gives us:

- Clean isolation of OS code behind `#[cfg(...)]`, confined to
  `poltertype-input` / `poltertype-layout`.
- The AI crate behind `feature = "ai"` — not compiled by default.
- The option to extract `poltertype-detect` as a standalone library if
  third-party use ever comes up.

---

## 5. Claude Code integration

### 5.1 `CLAUDE.md` (root)

Always in context:

- Architecture rules (where platform code lives, how to add a new language).
- Dev commands (`cargo run -p poltertype-app`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo fmt --all`).
- Code style (rustfmt, clippy strict).
- Safety constraints (never log user text in release builds).
- The release procedure.

### 5.2 `.claude/settings.json`

- Permissions for safe tools (`Read`, `Grep`, `Glob`, `Edit`, `Write`,
  `Bash(cargo *)`, `Bash(rustup *)`, `Bash(git status / diff / log / ...)`).
- Forbid accidental push/force operations.

### 5.3 Possible subagents (later)

- `platform-windows-expert` / `platform-macos-expert` /
  `platform-linux-expert`.
- `layout-mapping` — help with adding new layouts.
- `ai-integrations` — for work on AI plugins.

---

## 6. Security & privacy

- **One network call exists, and it is the updater.** `poltertype-update`
  fetches a release manifest from GitHub, downloads the installer for
  this platform, and verifies its SHA-256. It sends no body, no query
  string and no identifier — GitHub sees an IP and a User-Agent, the
  same as any HTTP server would. It is **not** telemetry and is not a
  place to add any. `[updates].enabled = false` switches it off
  entirely. The trust boundary *was* the GitHub account that publishes
  releases; since 0.7.0 `Manifest.signature` is a real detached ed25519
  signature, verified before any URL in the manifest is read. It is not
  yet mandatory — see `docs/DECISIONS.md`, 2026-07-13 and 2026-07-31.
- **An update is never installed while the app runs.** We hold a global
  keyboard hook; the swap happens on Quit or on an explicit "Restart to
  update".
- Beyond the updater: no network. AI is disabled; remote AI requires two
  toggles (`enabled` + `allow_remote`).
- We do not store text. Only a short-lived word buffer in RAM, cleared
  after a decision/timeout.
- API keys — via `keyring` (Win Credential Manager / macOS Keychain /
  GNOME Secret Service / KWallet).
- Password fields:
  - Win: skip when the focused field has `ES_PASSWORD`.
  - macOS: `AXSecureTextField`.
  - Linux: heuristics + a "disable in …" option.
- Logging: default level `info`. Release builds never log buffer
  contents, only metadata (length, language-from, language-to).
- Signing the release binaries — a separate phase.

---

## 7. Testing

Levels:

1. **Unit (Rust):**
   - `poltertype-detect::heuristic` — tables of test words and expected
     decisions.
   - `poltertype-layout::mappings` — full map symmetry (EN→UK→EN = identity).
2. **Integration:**
   - Inject synthetic `KeyEvent`s into `SwitcherEngine`, verify the
     decisions without OS hooks.
   - Property-based tests (`proptest`) for random-input fuzzing of the
     engine.
3. **E2E (manual matrix):**
   - Win 11, macOS 14+ (Intel+ARM), Ubuntu 24.04 (X11 + Wayland),
     Fedora 40 (Wayland).
4. **CI:**
   - cargo fmt / clippy / test on the {windows-latest, macos-latest,
     ubuntu-latest} matrix.
   - cargo-deny for licenses.

---

## 8. Risks & unknowns

| Risk | Impact | Mitigation |
|---|---|---|
| Wayland offers no global keylogging API. | A blocker for the primary use case on Wayland. | The evdev + `input` group + udev rule path (enabled by `setup-linux.sh`). AT-SPI fallback. Tray onboarding explains what is happening and why. |
| `setup-linux.sh` scares people with a `sudo` prompt. | Onboarding drop-off. | An honest explanatory banner in the tray + a link to the script's source. Alternatively — a guide for doing it manually. |
| The macOS Accessibility prompt scares people. | Onboarding drop-off. | First launch: a clean guide with a GIF. |
| Detector false positives. | The single most annoying thing. | A high threshold + an Undo hotkey + statistics of "cancelled" switches for tuning. |
| Antivirus/SmartScreen on Windows. | Users won't launch an unsigned build. | First release — a warning in the README. |
| Programs that grab global hooks themselves (games). | Conflict. | A per-app disable list. |
| Performance on old machines. | Input stutter. | The hook callback only enqueues; processing happens on a separate thread. |
| AI dependencies (ONNX runtime) bloat the binary. | Large MBs. | `feature = "ai"`; the AI build is a separate `poltertype-ai` artifact. |
| Remote LLM APIs: 200–800 ms latency. | Does not fit inline correction. | Call only in a "background rewrite" mode (after the fact) with confirmation. |
| Headless Linux audio. | Crash at startup. | `rodio` initializes lazily, falls back to no-op. |

---

## 9. Settled decisions (formerly "open questions")

1. **UI framework:** `iced` (pure Rust). Fallback — `egui`.
2. **Bundle ID:** `dev.opensource.poltertype`.
3. **UI languages v0.1:** EN + UK; the architecture is multilingual
   (i18n via `fluent-rs` or `rust-i18n`, `.ftl` files in `assets/i18n/`).
4. **Sounds v0.1:** CC0 placeholders; theme format — folders.
5. **Default log level:** `info`.
6. **v0.1 release channel:** GitHub Releases only.

---

## 10. Roadmap

> **Status as of v0.6.2 (2026-07-30).** Phases 0–8 are, in
> their core parts, complete and shipped across releases 0.1.0 →
> 0.6.2 — Phase 8's auto-updater landed in 0.4.0, 0.5.0 added
> the spelling-suggestions tooltip (surface FSTs, `poltertype-popup`,
> AT-SPI caret anchoring — see `DECISIONS.md` 2026-07-24), 0.6.0
> made corrections survive a user who keeps typing through them
> (`EVIOCGRAB`, `DECISIONS.md` 2026-07-29), 0.6.1 fixed where
> that tooltip actually lands, and 0.6.2 was the first release run on
> a real Mac — which is also where Phase 5 stopped being theory; below,
> what remains open is marked. Items that are **not** done are deliberately left as
> `[ ]` — that is the current work list. The wording of some items had
> drifted behind the code (e.g. `HeuristicDetector` in Phase 3 is
> actually called `WordPlausibilityDetector`); it is corrected here.

### Phase 0 — Skeleton ✅

- [x] Create the project, `git init`.
- [x] PLAN.md, README, LICENSE (MIT), .gitignore, .gitattributes,
      .editorconfig, CLAUDE.md, `.claude/`.
- [x] CONTRIBUTING.md (at the repository root, not in `docs/`).

### Phase 1 — Rust skeleton bootstrap ✅

- [x] Cargo workspace with 8 crates (`poltertype-update` joined in 0.4.0).
- [x] `poltertype-app`: `tao` event loop + `tray-icon`, a generated
      placeholder icon.
- [x] `single-instance`, `tracing` initialization.
- [x] CI: `cargo fmt/clippy/check` on three OSes.
- [x] A basic `cargo-deny` configuration.

### Phase 2 — Platform adapters ✅

- [x] `poltertype-input`: trait + Windows LL hook.
- [x] `poltertype-layout`: trait + the Windows implementation.
- [x] macOS / Linux are **no longer** stubs — see Phases 5 and 6.
- [x] `docs/PERMISSIONS.md`.

### Phase 3 — SwitcherEngine MVP ✅

- [x] `poltertype-types`: shared types (LayoutId, KeyEvent, ...).
- [x] `poltertype-detect`: `WordPlausibilityDetector` +
      `DictionaryDetector`. The dictionary is an FST over
      Hunspell-expanded lists (not `lingua`, which was dropped).
- [x] `poltertype-core`: WordBuffer, DecisionPolicy, Corrector,
      AudioPlayer.
- [x] The EN↔UK map in `data/layout-mappings/` (fifteen are bundled
      today: EN·UK·RU·DE·ES·FR·PL·CS·EL·HE·TR·BG·IT·PT-PT·PT-BR).
- [x] Pause / switch-last hotkeys.
- [x] Settings: saving/loading `config.toml`.

### Phase 4 — Settings UX ✅

The original plan (see `docs/DECISIONS.md`, the entry
`2026-05-02 — Phase 4: deferred full GUI`) deferred the full window.
**That decision was later reversed**: the `iced` GUI shipped already
in 0.1.0-beta and today has seven panels (Languages, Hotkeys,
Commands, Wordlists, General, Exceptions, About). It launches as a
separate `poltertype --settings` process.

- [x] Tray menu: "Edit config.toml…" via `opener`.
- [x] Tray menu: "Open Logs Folder…".
- [x] Tray menu: "Reload Settings".
- [x] File-backed logs via `tracing-appender`.
- [x] Engine: candidate-layout filtering by `[languages]`.
- [x] The full GUI (`iced`) — shipped earlier than planned.

### Phase 5 — macOS

- [x] `CGEventTap` (listener) — runtime-tuned on macOS 15 (Intel) in
      0.6.2 by an outside contributor. What CI could never catch: the
      tap runloop has to run in `kCFRunLoopDefaultMode` (the source
      never fires in `kCFRunLoopCommonModes` as the *run* mode), and
      emitted events must be stamped via `kCGEventSourceUserData` or
      they echo back as user input.
- [x] `TISSelectInputSource` (layout switching) — every TIS call now
      goes through the main dispatch queue; HIToolbox asserts it and
      killed the process with SIGILL otherwise. See `DECISIONS.md`.
- [x] **Run at login** — per-user LaunchAgent (`poltertype-autostart`).
- [x] **Accessibility onboarding** — the Settings window's **Setup**
      pane (0.7.0), opened by the tray's hook-failure alert. Reports
      Accessibility and Input Monitoring separately, raises the
      system's own prompt for each, and deep-links into the matching
      System Settings pane. Never yet run on a Mac; the screenshots /
      GIFs of the toggles remain undone.
- [ ] **`NSWorkspace` focus tracking** — not implemented, so the
      `FocusTracker` on macOS is a no-op (see Phase 6 and §3.9).
- [ ] **Keystroke hold-back.** The key gate works on Linux/evdev and
      on macOS (validated on Intel; opt-in via `POLTERTYPE_HOLD_KEYS=1`
      because of the post-correction latency); on Windows it is
      implemented but unvalidated, so a keystroke can still land
      inside a correction there.
- [ ] **Apple Silicon.** Validation so far is Intel-only.

### Phase 6 — Linux

- [x] **Wayland evdev listener** via `evdev`; `setup-linux.sh` adds
      the user to the `input` group + udev rules (`/dev/input/event*`
      and `/dev/uinput`).
- [x] Layout switcher: Hyprland → KDE → GSettings (GNOME family) →
      IBus → Fcitx5 → X11 XKB, each as a separate backend behind the
      `Trait`.
- [x] Send-keys via `uinput` (paired with evdev).
- [x] X11: XInput2 listener + XTest emitter + XKB switcher
      (`XkbLatchLockState`). Requires zero permissions (no
      `input` group, no `sudo`). The XKB switcher is probed **last** —
      where a DE drives the session, its backend keeps the layout
      indicator in sync, and locking the group underneath it would
      leave the indicator lying.
- [x] **`FocusTracker` for Linux** — landed 2026-07-13 (post-0.2.2):
      Hyprland IPC + X11 EWMH backends, `/proc`-resolved executable
      basename, 150 ms TTL cache. GNOME/KDE Wayland remain noop —
      see §3.9.
- [x] **Hook-failure alert in the tray** — landed 2026-07-13
      (post-0.2.2): "Setup Guide" menu entry + tooltip warning +
      one-shot notification when the listener fails to start. The
      originally sketched "Run setup" button (tray invoking `sudo`
      itself) was **rejected** — see `DECISIONS.md`; that rejection
      still stands, and the Setup pane (0.7.0) copies the command to
      the clipboard rather than running it.
- [x] ~~**Wayland AT-SPI fallback listener** via `atspi`~~ —
      **decided against 2026-08-01, with measurements.**
      `RegisterKeystrokeListener` returns false on a wlroots session
      and delivers nothing even with injected keys, because
      `at-spi2-registryd` can only relay what the compositor hands it
      and only mutter does. Where it *would* work (X11) we already
      have a listener that needs no permissions, so it adds nothing
      there. See `DECISIONS.md`, 2026-08-01.
- [x] **Portal send-keys** — done in 0.10.0, and not via `libei`.
      The portal exposes `NotifyKeyboardKeycode` as a plain D-Bus
      method, which is exactly what a correction needs; going through
      `ConnectToEIS` and the libei protocol would have meant a new
      protocol implementation and a heavy dependency to send twenty
      keystrokes, and would still need the same session negotiation.
      `zbus` was already in the tree.
      **Never executed** — there is no RemoteDesktop backend on the
      maintainer's machine. Tried only when `uinput` cannot be opened,
      so nobody who already ran `setup-linux.sh` sees a consent
      dialog. See `linux/portal/`.
- [ ] macOS (Apple Silicon): still nobody. And since 0.7.0 changed the
      macOS input path — `FlagsChanged`, modifier release — even the
      Intel confirmation predates what now ships. The only unconfirmed
      item.
- [x] The tray shows the language, the menu works.
- [x] The settings window: seven panels (more than planned), settings
      persist.
- [x] The AI subsystem is real: the two original stub backends were
      replaced in 0.10.0 by a single `LlmDetector` speaking three wire
      formats to a user-named endpoint, and since 0.12.0 it ships
      compiled into the installers — off until configured (see
      Phase 7 and docs/AI.md).
- [x] Screenshots in the README — added 2026-07-13.
