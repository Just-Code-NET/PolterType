# <img src="docs/icon.png" width="32" height="32" align="absmiddle" alt=""> PolterType

Cross-platform automatic keyboard layout switcher. Lives in the system
tray, notices when you start typing in the wrong layout, switches it
and retypes the last word — like a friendly poltergeist that haunts
your keyboard.

> [poltertype.com](https://poltertype.com) ·
> [Wiki](../../wiki) — setup guides, configuration cookbook, FAQ
>
> 🌐 [Deutsch](README.de.md) · [Español](README.es.md) ·
> [Français](README.fr.md) · [Українська](README.uk.md)

![Typing "ma;ana" with the wrong layout active: PolterType switches the layout and retypes it as "mañana", and the rest of the sentence comes out right as typed](docs/screenshots/demo.gif)

*Live capture, unedited timing: `ñ` sits on the US `;` key, so Spanish
typed on an English layout comes out as `ma;ana` — PolterType fixes the
word the moment it ends and switches the layout with it, so `por la
tarde` lands correctly as typed.*

> **Status: v0.23.0**, out of beta since v0.1.0. Windows, macOS and
> Linux (Wayland and X11, on the desktops in the table below) all
> correct words end to end, and the spelling-suggestions tooltip
> renders on all three. Installers are **unsigned**, so first launch
> meets an OS warning. One macOS caveat stands: a correction typed
> under a *held* modifier — the ⌘⌫ case — has still had no report from
> real hardware
> ([#3](https://github.com/Just-Code-NET/PolterType/issues/3)).
> [docs/KNOWN-GAPS.md](docs/KNOWN-GAPS.md) says what every claim here
> was measured on and what is only reasoned;
> [CHANGELOG.md](CHANGELOG.md) has the per-release detail.

![PolterType settings window — Languages panel](docs/screenshots/settings-window.png)

*The settings window (`poltertype --settings`). Day to day the app is
just a tray icon; you open this window only to tweak languages,
hotkeys, smart commands, wordlists, and per-app exceptions.*

## Goals

- **Smart** — language detection per word; pluggable AI detectors for
  power users (off by default).
- **Fast** — pure Rust, no WebView, no perceptible typing latency.
- **Light to run** — single binary, ~10–15 MB, a tray icon's worth of
  CPU and RAM. **The download is not light** — the installers are
  55–65 MB, nearly all of it the fifteen bundled dictionaries — but
  only the languages your OS has enabled are ever read into memory.
- **Quiet** — tray-only, **zero telemetry**. Exactly one network call
  exists, the update check (§ [Staying up to
  date](#staying-up-to-date)); it sends nothing about you and one
  checkbox turns it off. The AI subsystem is off by default and needs
  a second explicit toggle to reach the network at all.
- **Configurable** — autostart, per-language allowlist, per-app
  exceptions, hotkeys, sound themes.
- **Open source** — MIT licensed.

## Platforms

| OS              | Status                                                                                                                                                                        |
| --------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Windows 10 / 11 | working                                                                                                                                                                       |
| macOS 11+       | working — validated on macOS 15 (Intel) and, at 0.19.0, on macOS 26 (Apple Silicon, M1 Pro), self-update included ([#3](https://github.com/Just-Code-NET/PolterType/issues/3)). Needs Accessibility **and** Input Monitoring permission; the app prompts on first launch. **On Intel, take 0.14.4 or newer** — earlier DMGs shipped an unsigned x86_64 slice, and macOS will not let unsigned code hold Accessibility ([#28](https://github.com/Just-Code-NET/PolterType/issues/28)) |
| Linux (Wayland) | working on **Hyprland, KDE Plasma, GNOME and sway**; run `scripts/setup-linux.sh` once (evdev + uinput access). Other wlroots compositors — labwc, and Budgie's and Xfce's Wayland sessions — expose no way to change the keyboard layout, so PolterType reports that it cannot switch there rather than pretending. |
| Linux (X11)     | working, and needs **no setup script at all** — XInput2 listener + XTest emitter need no `input`-group membership. Measured on Xfce, LXQt, Cinnamon, i3, icewm, openbox and fluxbox. **MATE is the exception**: its settings daemon owns the keyboard layout and reports it inconsistently, so PolterType stands down there. |

Per-OS permissions are in [docs/PERMISSIONS.md](docs/PERMISSIONS.md).

**Typing straight on through a correction.** On Linux a correction
holds your keystrokes back while it types and replays them behind
itself, so the next word doesn't scramble the result — except behind
an input remapper such as keyd, where PolterType stands down and falls
back to detect-and-repair. Windows (0.11.0) and macOS (0.13.0) have
the same mechanism, both validated on real hardware and both **off by
default** (`POLTERTYPE_HOLD_KEYS=1`): it works, and it costs a
noticeable delay after every correction, which is not a trade worth
making on everybody's behalf
([#7](https://github.com/Just-Code-NET/PolterType/issues/7),
[#8](https://github.com/Just-Code-NET/PolterType/issues/8)).

## Install

Builds are published as GitHub Releases —
[**Releases page**](../../releases). Each release ships four
installers (plus `latest.json`, the manifest the in-app updater
polls — you never download that one by hand):

| OS                                | File                                          | How to install                                                                                                                                                                                                      |
| --------------------------------- | --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Windows 10 / 11                   | `poltertype-<ver>-x86_64-pc-windows-msvc.msi` | Double-click. Per-user install — no admin rights, no UAC prompt. SmartScreen may show "Windows protected your PC" → **More info** → **Run anyway**.                                                                 |
| macOS 11+ (Intel + Apple Silicon) | `poltertype-<ver>-universal-apple-darwin.dmg` | Open the DMG, drag `poltertype.app` into `/Applications`. First launch: right-click the app → **Open** (or run `xattr -dr com.apple.quarantine /Applications/poltertype.app`). Then grant **Accessibility** and **Input Monitoring** — macOS prompts for both on first run. Expect to grant them again after an update: without a Developer ID the app is identified by the hash of its own bytes, so a new version is, to macOS, different software. |
| Linux (x86_64)                    | `poltertype-<ver>-x86_64.AppImage`            | `chmod +x` and run. Per-user, no system install. See [docs/PERMISSIONS.md](docs/PERMISSIONS.md) for evdev access on Wayland, and for NixOS, where an AppImage needs `programs.appimage.binfmt` to run at all.                                                                                        |
| Linux (aarch64)                   | `poltertype-<ver>-aarch64.AppImage`           | Same, for ARM64 — Raspberry Pi 5, Asahi, ARM laptops and servers. Built natively, not cross-compiled.                                                                                                               |

> The installers are **unsigned** — that is why Gatekeeper and
> SmartScreen warn on first launch; the plan is in
> [docs/CODE_SIGNING.md](docs/CODE_SIGNING.md).

> **No Flatpak, and there won't be one.** PolterType types by writing
> to `/dev/uinput`, which no Flatpak permission grants short of
> `--device=all` — the whole device tree — and there is no portal for
> it. Layout switching also needs host binaries (`hyprctl`,
> `gsettings`, `gdbus`, `qdbus`, `ibus`) that a sandbox does not have. The
> reasoning, sources and the conditions under which we'd revisit are
> in [docs/DECISIONS.md](docs/DECISIONS.md) (2026-07-31). Use the
> AppImage, or a native package.

Building from source is documented in
[CONTRIBUTING.md](CONTRIBUTING.md).

### Staying up to date

You only have to install once. From then on PolterType keeps itself
current — **on by default.** It checks GitHub for new releases once a
day (first check ~a minute after startup), downloads the installer for
your platform in the background, verifies its SHA-256 against the
release manifest, and then **waits**: nothing is installed while you
are typing. The swap happens when you quit the app, or when you click
**⟳ Restart to update** in the tray menu.

This is the **only** part of PolterType that touches the network — a
`GET` of a small JSON manifest, and, only when there really is a new
version, a `GET` of the installer. No account, no identifier, nothing
about you and nothing about what you type. GitHub sees what any
download reveals: your IP and a User-Agent naming the running version
(`PolterType/0.23.0 (updater)`). The exact manifest URL is printed on
the Settings window's **General** pane, so you never have to take our
word for it.

Turn it off with the checkbox on that same pane, or in `config.toml`:

```toml
[updates]
enabled              = true    # ← the default. false = never check, never download
check_interval_hours = 24      # floor is 1; 0 does NOT mean "off", it means hourly
```

Switching it off also deletes anything already downloaded.

Two caveats worth stating plainly:

- **The manifest is signed, and since v0.17.2 an unsigned one is
  refused.** Every download is verified against the SHA-256 in the
  release manifest, which catches a corrupted transfer or a tampered
  CDN — but not a compromised GitHub account, since the checksum lives
  in the same release as the installer. The manifest itself carries a
  detached ed25519 signature made with a key that is *not* in CI,
  checked against a public key compiled into the binary before any URL
  in the manifest is read. The **installers** are a different matter
  and are still unsigned
  ([docs/CODE_SIGNING.md](docs/CODE_SIGNING.md)), so a first launch
  still meets an OS warning.
- **Only our own installers self-update.** A distro package or a
  `cargo build` binary isn't ours to overwrite, so you get a
  notification pointing at the Releases page instead — same on an
  architecture we don't publish for (x86_64 and aarch64 Linux, x86_64
  Windows and universal macOS are the whole list).

## Stack

- Pure Rust — no WebView, no Node.
- `tao` event loop + `tray-icon` + `global-hotkey` + `single-instance`.
- `ureq` + `rustls` + `sha2` for the updater, plus `ed25519-dalek` to
  check the release manifest's signature (verification only; the app
  holds no secret).
- The AI subsystem (`feature = "ai"`, in the official installers since
  v0.12.0) — one detector that speaks `openai-chat`,
  `anthropic-messages` or `ollama-generate` to an endpoint **you**
  configure. No model, no vendor SDK, no default endpoint;
  `[ai].enabled` is off by default and a non-loopback endpoint needs a
  second toggle (`[ai].allow_remote = true`). Its HTTP client
  (`reqwest` + `rustls`) is the second and last place a TLS stack is
  linked in, and stays dormant until an `[[ai.plugins]]` entry names
  an endpoint. A stock source build (`cargo build`, no flags) contains
  none of it. See [docs/AI.md](docs/AI.md).

AI was involved in the development process. Code quality and the
quality of the finished product are held to a demanding standard:
every line of code is reviewed.

See [docs/PLAN.md §2](docs/PLAN.md) for the alternatives considered.

## Hotkeys

Two built-in hotkeys, both rebindable on the **Hotkeys** pane of
the Settings window:

| Default                | Action                                                                                            |
| ---------------------- | ------------------------------------------------------------------------------------------------- |
| `Ctrl+Shift+Space`     | Pause / resume auto-switching.                                                                    |
| `Ctrl+Shift+Backspace` | Force-switch a word — the one you are still typing, or the most recent finished one if you have moved past it. Ignores every filter, including the dev-friendly skips below, and works while auto-switching is paused. On a word PolterType *just corrected* it undoes that correction, and learns the word so it won't happen again — unless the correction rested on a real word of the other language, in which case putting it back is a one-off rather than a new dictionary entry. Press it again and the word moves on to the next layout, wrapping: with two layouts that is simply back again, so a press made in error costs one more press. Only that first undo teaches anything. |

> **On macOS the pause default is `Ctrl+Shift+P`.** `Ctrl+Space` and
> `Ctrl+Shift+Space` are macOS's own "previous / next input source"
> shortcuts, so claiming them globally would take your layout
> switching away — the very thing PolterType is there to complement.

> **On Wayland the force-switch default is `Ctrl+Shift+F9`.** There we
> read keys from the evdev keystream, so the chord reaches the focused
> app too — and `Ctrl+Backspace` deletes the very word you asked to
> fix, so PolterType substitutes a key no app acts on.

Both substitutions apply only while you are on the default; bind
whatever you like and it is honoured as written, destructive or not.

### Modifier-only chords

Either hotkey can be bound to modifiers alone — the gesture people
arrive with from Punto Switcher and Caramba:

| Binding                    | Gesture                                     |
| -------------------------- | ------------------------------------------- |
| `Shift+Shift`              | Tap Shift twice.                            |
| `Ctrl+Shift`, `Alt+Shift`… | Hold two modifiers together, then let go.   |

They fire when the keys come **back up**, and only if nothing else was
pressed while they were down — so `Ctrl+C`, `Ctrl+Shift+V` and typing
capitals are left alone. A tap also has to be a tap: hold the keys for
more than half a second and nothing happens.

To bind one, click **Rebind** and make the gesture. Two shapes are
deliberately not offered: a *single* modifier, because mouse buttons
are invisible to PolterType on Windows and macOS and a lone-Shift
binding would fire on every Shift+click; and Caps Lock, because
binding it would flip the lock as well as fire. Modifier chords are
never consumed either — the keys still reach the focused application,
which is harmless for a bare modifier and is why this shape behaves
the same everywhere.

## Spelling suggestions

Wrong-layout words get auto-corrected; plain typos get *suggested*.
When a word you just finished isn't in the dictionary for the
language you're typing, a small tooltip appears near the focused
window with up to 5 nearby dictionary words — click one, or press
`Ctrl+Shift+<digit>`, and the word is replaced in place. When the
engine saw a possible wrong-layout word but wasn't confident enough
to auto-switch, that candidate leads the list with a layout badge, so
the borderline cases become your one-click call.

Everything is local: candidates come from the bundled dictionaries
(plus your own wordlist overlays), ranked by a keyboard-aware edit
distance that knows `hwllo` is a slipped finger away from `hello`.
The last row of every tooltip is **Add to dictionary** — one click
teaches PolterType your jargon for good, and other forms of the word
come with it — add `mañana`, and `mañanas` stops being flagged too.
The tooltip never steals keyboard focus and disappears after 30 seconds or
the moment you type past it. Tune or disable it on the **Suggestions**
pane (`[suggestions]` in `config.toml`).

Typing past it is easy to do — so the offer is not lost with it. The
last eight words a tooltip offered and you typed past wait under **Add
a missed word…** in the tray menu, and picking one adds it exactly as
the tooltip row would. That list lives in memory only: it is never
written to a file, never logged, and it goes when PolterType does.

The tooltip only appears for words PolterType *keeps*. When it
corrects one it shouldn't have, the teaching gesture is
`Ctrl+Shift+Backspace`: it puts the word back as you typed it and adds
it to your dictionary. Undoing a correction that was *right* teaches
nothing — otherwise trying the gesture out would file the other
language's word, typed on the wrong keyboard, as vocabulary. For words
you already know it will get wrong, `word_whitelist` in `[exceptions]`
takes ones it must never touch.

> **Where it renders.** Linux: Wayland layer-shell on Hyprland, Sway
> and KDE Plasma, an override-redirect window on X11 — which also
> covers GNOME Wayland, through XWayland. Windows since 0.11.0 and
> macOS since 0.15.0, both as a non-activating panel that cannot take
> the keyboard away from what you are typing in.
>
> **Where it lands** depends on what the focused app will tell us.
> Apps that expose a caret get the tooltip directly above it;
> everything else gets it just above the window's bottom edge, the
> neighbourhood of chat boxes and shell prompts. It is never placed by
> your mouse pointer. Which apps land in which half we can't promise:
> a program that draws its own caret — most browsers, most terminals —
> has none for the OS to report, and on Linux only apps with a live
> accessibility bridge do. A reported caret is checked before it is
> trusted, too (on macOS, Chrome and Terminal report one nowhere near
> the text), and failing that check costs the window its caret, not
> its tooltip.

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

## Languages

Fifteen layouts ship with the app, each with a full dictionary:

**English (US) · Ukrainian · Russian · German · Spanish · French ·
Polish · Czech · Greek · Hebrew · Turkish · Bulgarian · Italian ·
Portuguese (PT) · Portuguese (BR)**

PolterType only loads the ones your OS actually has enabled, so
bundling fifteen costs a two-keyboard user nothing at runtime.

**On Windows the bundled table is only a starting point.** A Windows
layout is named after a *language*, but a language is not a keyboard —
Bulgarian alone ships in three genuinely different arrangements. Since
v0.14.3 PolterType asks Windows what each of your installed keyboards
actually produces and uses that answer, so variants and even custom
layouts we have never heard of come out right. Linux and macOS read
the bundled tables, and a TOML of your own in
`<config-dir>/poltertype/layouts/` still outranks everything,
everywhere.

Two are worth a footnote rather than a surprise:

- **Polish** gets no Polish↔English correction, and can't. The layout
  essentially every Polish user has enabled is the "programmer's" one
  — US QWERTY with the diacritics on AltGr, which PolterType doesn't
  track — so under it Polish and English produce identical characters
  and there is no mistake to detect. The Polish dictionary still does
  real work: it stops Polish prose being dragged toward whatever other
  layout you have active, and Polish↔Cyrillic works normally.
- **Hebrew** ships dictionary stems rather than every inflected form,
  because expanding its clitic prefixes yields 60 million of them.
  Hebrew shares its script with nothing else bundled, so detection
  leans on that and the dictionary refines it.

**Yours isn't here?** Adding a language is one TOML file and one
wordlist — no Rust required. The walkthrough is
[docs/ADDING_A_LANGUAGE.md](docs/ADDING_A_LANGUAGE.md), and you can
try it on your own machine without rebuilding anything by dropping the
files into `<config-dir>/poltertype/layouts/` and
`<config-dir>/poltertype/wordlists/`.

## Dev-friendly: stays out of code

If you write code, you _don't_ want a layout switcher meddling with
identifiers. Three guards protect you, and they are pure engine logic
— they work in every app, on every OS:

- **Per-token identifier guard** — the engine doesn't auto-switch on
  tokens that look like identifiers: `snake_case`, `camelCase`,
  `letter+digit`, or anything containing `\\` / `;` / `` ` ``. Toggle
  via `engine.suppress_in_identifiers = false` in `config.toml`.
- **Plausibility-keep** — if the word you typed already reads as
  plausible for the current layout (real letters, sane vowel ratio,
  no ridiculous consonant pile-ups), the engine refuses to switch
  even if the alternate scores higher. This is what keeps `kubectl`,
  `terraform`, `nginx`, surnames, and other "real but uncommon"
  vocabulary from getting auto-corrected to Cyrillic noise.
  A dot-separated token — `games.just-code.net`, `config.toml` — is
  judged one segment at a time and is only as plausible as its worst
  segment, so a hostname reads as plausible exactly when all of its
  parts do.
- **Structural boundaries** — a word that starts or ends with `:` `/`
  `\` `@` `=` `#` `&` is part of a URL, a path, an email address or an
  expression, so the engine stays out of it entirely: `http:`, `/tmp`,
  `@nickname`, `C:\Users` alike. (Sentence punctuation like `.` and
  `,` is deliberately *not* on that list.)

If that isn't enough for a particular app, silence it there
explicitly:

- **Per-app skip list** — `[exceptions].disabled_apps` in
  `config.toml`, matched case-insensitively against the focused
  process's executable basename (`Code.exe`, `code`, `kitty`, …).
  **Empty by default**: PolterType corrects everywhere until you tell
  it not to — a shipped list of editors and terminals only made the
  app look dead in exactly the windows developers type in. Add your
  own, or manage them on the **Exceptions** pane in Settings.

> **The skip list needs a focus tracker, and that isn't equally good
> everywhere.** Reading which application has focus is complete on
> Windows, macOS (since 0.15.0), Hyprland and X11. On other Wayland
> sessions (GNOME, KDE) PolterType asks the accessibility bus instead,
> which only answers for applications that expose an accessibility
> bridge. Most terminals don't, so the skip list, per-app wordlist
> profiles and `apps = [...]` scoping may simply not fire there.

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

**Per-app profiles** — if `kubectl` should count as a real word
inside VS Code but not in chat, declare a `[[wordlists.profiles]]`
entry in `config.toml` and drop the per-profile overlays under
`<config-dir>/poltertype/wordlists/profiles/<id>/<stem>.txt`.
The engine swaps the active overlay set when the focused app
changes. See [docs/DATA_LAYOUT.md](docs/DATA_LAYOUT.md) for the
full schema.

Wordlist edits apply **without a restart**: closing the Settings
window rebuilds the dictionaries, and "Reload Settings" in the tray
does it for hand-edited files. Only the *bundled*
`data/wordlists/*.txt` need a rebuild — those bake into the FST at
compile time.

Writing a comment in another language inside an IDE? Hit
`Ctrl+Shift+Backspace` after the word — that hotkey ignores every
filter by design. (On Wayland: `Ctrl+Shift+F9`, see above.)

## Settings

Two ways to configure:

1. **Tray → "Settings…"** opens a real GUI (`iced 0.13` with the
   lightweight `tiny-skia` renderer). Ten panes: **Setup**,
   **Languages**, **Hotkeys**, **Commands**, **Wordlists**,
   **General**, **Exceptions**, **Suggestions**, **Plugins**,
   **About**.
2. **Tray → "Edit config.toml…"** opens the raw TOML file in your
   default editor — useful for what the GUI doesn't expose yet
   (creating a wordlist profile entry, listing `[[commands]]` in
   bulk, …):
   - Windows: `%APPDATA%\opensource\poltertype\config\config.toml`
   - macOS: `~/Library/Application Support/dev.opensource.poltertype/config.toml`
   - Linux: `~/.config/poltertype/config.toml`

**"Start automatically when I sign in"** (in **General**, on by
default) registers PolterType with the OS: a LaunchAgent on macOS, a
per-user run key on Windows, a systemd user service on Linux (an XDG
autostart entry where there is no user manager). `config.toml` is the
source of truth — deleting the entry by hand only lasts until the next
launch, so untick the setting instead.

Logs land under the OS data dir; "Open Logs Folder…" in the tray takes
you there. Edits apply **without a restart**: closing the Settings
window picks everything up on its way out, and "Reload Settings" in
the tray does the same for hand-edited TOML — general flags,
exceptions, hotkey bindings, wordlists and profile overlays alike.

The tray also carries **Pause auto-switch**, **Open User Wordlists
Folder…**, **Open User Layouts Folder…**, and — unless you turned
updates off — **Check for updates…**, which becomes **⟳ Restart to
update** once a new version is staged. If the keyboard hooks fail to
start, a **⚠ Setup Guide…** entry appears at the top pointing at
[docs/PERMISSIONS.md](docs/PERMISSIONS.md).

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

# With the AI subsystem compiled in — the configuration the official
# installers ship since v0.12.0. `[[ai.plugins]]` entries become real
# detectors calling an endpoint YOU configure; with none configured
# the subsystem builds nothing and opens no socket. (The updater is
# separate and is in every build.) See docs/AI.md.
cargo build --release -p poltertype-app --features ai,poltertype-ai/remote
```

[CONTRIBUTING.md](CONTRIBUTING.md) has the per-OS native dep
checklist.

## License

[MIT](LICENSE)
