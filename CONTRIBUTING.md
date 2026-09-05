# Contributing to PolterType

Thanks for the interest! This document covers the practical bits;
the architecture lives in [docs/PLAN.md](docs/PLAN.md),
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) (why the pieces are shaped
the way they are) and [docs/DECISIONS.md](docs/DECISIONS.md).

Two short documents worth knowing about before you start:
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) applies to the issue tracker,
pull requests, Discussions and the wiki; and if you find a security
problem, **[SECURITY.md](SECURITY.md) asks you not to open a public
issue** — this app reads keystrokes, so a report about it reaching the
wrong place has real consequences.

## Building locally

**The first build after a fresh clone is slow, on purpose.**
Dependencies are compiled with `opt-level = 3` even in a debug build
(`[profile.dev.package."*"]`), because the Settings window is rendered
on the CPU — unoptimised, `tiny-skia` and `cosmic-text` are the whole
frame budget and the window stutters when you scroll it. Our own crates
stay unoptimised, so backtraces and stepping work where the bugs are,
and it is those crates you recompile while working.

```bash
# Default build (no AI subsystem)
cargo build -p poltertype-app

# Run
cargo run -p poltertype-app

# With the AI subsystem compiled in. This does NOT turn AI on: there is
# no default endpoint, `[ai].enabled` is off, and a non-loopback one
# needs `[ai].allow_remote` as well — so out of the box nothing answers
# and no decision changes. See docs/AI.md.
cargo build -p poltertype-app --features ai

# With AI + the remote HTTP capability compiled in. Without this
# sub-feature no HTTP client is compiled in at all (`cargo tree`
# confirms it); with it, `[[ai.plugins]]` entries call whatever endpoint
# the user configured.
cargo build -p poltertype-app --features ai,poltertype-ai/remote

# Lints (CI runs the same)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask style
cargo test --workspace
```

## `cargo xtask`

The `xtask` subcommand is wired up via a cargo alias in
[`.cargo/config.toml`](.cargo/config.toml) — no `cargo install` step
needed, the alias is loaded automatically from any directory inside
the workspace. If you previously did `cargo install cargo-xtask`
because of older docs, that's an unrelated stub package — uninstall
it with `cargo uninstall cargo-xtask` so it doesn't shadow our alias.

```bash
cargo xtask help            # list available subcommands
cargo xtask wordlists fetch # re-fetch + Hunspell-expand bundled dictionaries
cargo xtask hooks install   # see below
cargo xtask hooks uninstall
cargo xtask style [<path>]  # file-organization + platform rules (below)
cargo xtask assets icon-png <out> [--size N]   # render the app icon (the PolterType mark)
cargo xtask assets icon-ico <out>              # …as a multi-size Windows .ico
```

`poltertype.exe` embeds its own copy of that icon at build time (see
`crates/poltertype-app/build.rs`), so these two commands are for the
*installers* — the Windows one only needs `.ico` for the Add/Remove
Programs entry — and for `docs/icon.png`, the mark in the README
heading (`--size 128`; regenerate it if the geometry ever changes).

## Git hooks (one-time per clone)

```bash
cargo xtask hooks install
```

Wires the versioned hooks under [`.githooks/`](.githooks/):

| Hook | Runs | Why |
|---|---|---|
| `pre-commit` | `cargo fmt --all -- --check`, then clippy twice — the default feature set (what CI runs) and `--all-features` — then `cargo xtask style` | No commits with formatter drift, lint violations in either feature shape, or code in the wrong file. |
| `pre-push` | `cargo build --workspace --all-targets` | No pushes that don't compile. |

Bypass a single run with `git commit --no-verify` / `git push
--no-verify` if you really need to. Uninstall everything with
`cargo xtask hooks uninstall`.

The hooks mirror the gates CI enforces, so failing locally means
failing on GitHub Actions.

**They are fast, and if they are not, something is wrong.** With
everything already built, a commit costs about 4 seconds and a push
about 2, because each of the three configurations above keeps its own
build directory (`target/lint`, `target/lint-all`, `target/`) and
therefore stays warm. Two costs are expected and normal: the first
commit after a fresh clone fills the two lint directories, roughly two
minutes each, and they occupy about 900 MB apiece.

If instead *every* hook run takes minutes on an unchanged tree, the
cause is almost certainly a build script declaring
`cargo:rerun-if-changed` on a path that does not exist — cargo then
treats that script as stale forever and rebuilds the workspace behind
it. Ask cargo directly rather than guessing:

```bash
CARGO_LOG=cargo::core::compiler::fingerprint=info \
    cargo clippy --workspace --all-targets 2>&1 | grep -E "stale:|dirty:"
```

### Linux native deps

Ubuntu / Debian:

```bash
sudo apt-get install \
    pkg-config libdbus-1-dev libudev-dev \
    libxkbcommon-dev libxkbcommon-x11-dev \
    libwayland-dev libx11-dev libxi-dev libxtst-dev libxdo-dev \
    libgtk-3-dev libayatana-appindicator3-dev libasound2-dev
```

Fedora:

```bash
sudo dnf install \
    pkg-config dbus-devel libudev-devel \
    libxkbcommon-devel libxkbcommon-x11-devel \
    wayland-devel libX11-devel libXi-devel libXtst-devel libxdo-devel \
    gtk3-devel libayatana-appindicator-gtk3-devel alsa-lib-devel
```

On **Wayland**, run `scripts/setup-linux.sh` once to grant
`/dev/input/event*` + `/dev/uinput` access. On **X11** you need
nothing at all — no `input` group, no udev rule, no `sudo`. See
[docs/PERMISSIONS.md](docs/PERMISSIONS.md) for the rationale.

### macOS

System Settings → Privacy & Security → Accessibility → enable
`poltertype` (or your `cargo run` debug binary). The app will fail
to install its CGEventTap until that's granted.

### Windows

Just `cargo run`. SmartScreen may complain about the unsigned
binary — releases are still unsigned; code signing is tracked for a
later phase.

## Project layout

```
crates/
  poltertype-app/      binary  — tray + event loop + plumbing + Settings UI
  poltertype-core/    library — engine, settings, layouts, data_dir, audio
  poltertype-input/   library — InputListener / KeyEmitter trait + per-OS
  poltertype-layout/  library — LayoutSwitcher trait + per-OS
  poltertype-detect/  library — Detector / WordRewriter traits + built-ins
  poltertype-update/  library — GitHub-Releases updater: manifest, download,
                                staging, per-OS install. NOT optional — it is
                                in every build, and it is the only network code
  poltertype-popup/   library — suggestion tooltip: focus-free overlay
                                (Wayland layer-shell / X11 override-redirect)
  poltertype-autostart/ library — run at login: LaunchAgent (macOS),
                      HKCU run key (Windows), XDG entry (Linux)
  poltertype-shell/   library — per-OS app-shell quirks: instance-lock
                      identity, Dock activation policy, keycap glyphs,
                      the Linux desktop entry a window's icon comes from
  poltertype-tray/    library — per-OS tray quirks (Linux: keeps the GTK
                                backend's deprecation warning off stderr,
                                and checks the dlopen'd appindicator
                                library is there before it aborts us)
  poltertype-ai/      library — optional AI plug-ins (feature `ai`)
  poltertype-types/   library — shared types (LayoutId, KeyEvent, …)
  poltertype-icon/    library — the brand mark as geometry: RGBA, PNG and
                                .ico, drawn at build time rather than
                                checked in (docs/icon.png is a render of it)
data/                source-of-truth, committed; consumed by build.rs
  layout-mappings/   declarative scancode→char tables (TOML)
  wordlists/         <stem>.txt.gz / -extras.txt / -stop.txt
docs/
  PLAN.md / DECISIONS.md / PERMISSIONS.md / AI.md
  DATA_LAYOUT.md     on-disk data tree + plug-in foundations
  ADDING_A_LANGUAGE.md
  RELEASING.md       the release checklist — read it BEFORE tagging
installers/          per-platform packaging — see "Releasing" below
  wix/main.wxs              WiX 3.x source for the Windows MSI
  windows/build-msi.ps1     wraps candle.exe + light.exe
  macos/Info.plist.in       template for the .app bundle
  macos/build-dmg.sh        universal-binary .app + .dmg via lipo + hdiutil
  linux/poltertype.desktop the AppImage's .desktop entry
  linux/build-appimage.sh   wraps linuxdeploy + appimage plugin (ARCH=x86_64|aarch64)
packaging/           distribution manifests — staged, not published
  aur/               PKGBUILDs: poltertype (source) + poltertype-bin
  winget/            the JustCode.PolterType manifest trio
  homebrew/          the cask for Just-Code-NET/homebrew-tap
  bump.sh            re-point all three at a published release
scripts/
  setup-linux.sh — one-time evdev permission grant
```

`crates/poltertype-core/build.rs` reads from `data/` and writes prepared
assets (FSTs + copied TOMLs + copied stop-word txts) to
`<workspace>/target/dist/data/` on every cargo build. The runtime
finds that tree via `poltertype_core::data_dir::resolve()`. Installer
scripts copy `target/dist/data/` into the install location. See
[docs/DATA_LAYOUT.md](docs/DATA_LAYOUT.md) for the full picture.

## Settings UI

Tray menu **"Settings…"** opens an iced-based GUI with ten panes
(`Pane` in `settings_ui/enums.rs` is the list). **"Edit config.toml…"**
covers what the GUI doesn't expose: creating a wordlist profile,
bulk-editing `[[commands]]`, `[updates].check_interval_hours` (the
General pane has the on/off checkbox but not the interval) and the
`[ai]` switches.

The Settings GUI is the same `poltertype` binary launched with
`--settings`; it runs as a child process so the tray's main-thread
event loop doesn't have to share NSApplication on macOS. When the
window closes the tray reloads settings automatically.

## Adding a new keyboard layout

1. Drop a TOML into `data/layout-mappings/` named after the BCP-47
   tag (`de_de.toml`, `kk_cyrl_kz.toml`, …). Use one of the existing
   files as a template.
2. Add the same stem to `LAYOUTS` in `crates/poltertype-core/build.rs` (so
   build.rs copies it) AND to `BUNDLED_LAYOUT_STEMS` in
   `crates/poltertype-core/src/layouts/consts.rs` (so the runtime considers it).
3. Send a PR. No further Rust changes are required for the engine
   to start considering the new layout — the file is the contract.

If your language has unusual vowels not covered by
`derive_vowels()` in `crates/poltertype-core/src/layouts/helpers.rs`, extend that
function with a special case.

## Translating

Two separate things can be translated, and neither needs any Rust:

* **The settings window** — one TOML file, kept in your own config
  directory rather than in this repository: the set of languages the
  build ships is closed for now, and a catalog of your own works the
  same and is yours to share. The whole guide is
  [docs/TRANSLATING_THE_UI.md](docs/TRANSLATING_THE_UI.md).
* **The README quick-start** — a `README.<lang>.md` beside this file.
  `README.de.md`, `README.es.md`, `README.fr.md` and `README.uk.md` are
  the shape to copy.

A quick-start is deliberately **not** a translation of the whole README.
Translate only the parts that barely change:

1. What the app does — the opening paragraph — plus a line pointing at
   the English README for everything else.
2. The install table, all four installers.
3. The two caveats: unsigned installers, and no Flatpak.
4. The closing line pointing here for building from source.

Then add your language to the link line under the intro in `README.md`,
in alphabetical order of the language's own name.

Three rules that keep a translation honest:

* **No version numbers.** Write `<ver>` exactly as the English table
  does, so a release cannot make your file wrong.
* **Keep the caveats as blunt as the English.** Unsigned installers and
  per-platform gaps are stated plainly on purpose — softening them in
  translation is the one edit that turns a helpful file into a
  misleading one.
* **Translate meaning, not words.** Use the wording the OS itself shows
  in your language for a permission or a warning dialog, rather than a
  literal rendering of the English.

Any language is welcome, and a layout contribution paired with a
translation is welcome twice over. One thing to know before you start:
when a structural change does reach the quick-start — an installer
renamed, a permission step altered — [docs/RELEASING.md](docs/RELEASING.md)
makes updating every `README.<lang>.md` part of the release checklist,
and a translation nobody here can maintain gets deleted rather than left
to rot. Saying you can keep yours current is worth more than one more
language.

## Style & guarantees (hard rules)

* `clippy --workspace --all-targets -- -D warnings` must pass.
* No `unwrap()` / `expect()` outside tests, build scripts, or `main`.
* Never log user-typed text in release builds. The word buffer is
  RAM-only and short-lived.
* The OS hook callback never blocks — events go straight onto a
  `crossbeam-channel`; the engine processes them on a worker thread.
* Platform code lives behind `cfg`-gated modules in `poltertype-input`,
  `poltertype-layout`, `poltertype-update`, `poltertype-popup`,
  `poltertype-tray`, `poltertype-autostart` and `poltertype-shell`.
  The shape is one crate
  per *capability* with per-OS modules inside it — behind a trait and
  a factory where the caller holds a backend (`poltertype-layout`),
  or a plain function where there is nothing to hold
  (`poltertype-tray`, `poltertype-autostart`). Not one crate per
  platform. How seriously we mean it: a one-function GTK quirk got its
  own 64-line crate rather than put the first `#[cfg]` in `main.rs`
  (see `docs/DECISIONS.md`, 2026-07-29).

  Inside such a crate a platform `cfg` — `target_os`, `unix`,
  `windows`, `target_arch` — may appear in exactly **two** places:

  1. on the `mod` or `use` declaration that picks the per-OS module.
     This is the dispatch, and there is one of it per capability;
  2. inside a file or directory already named for that OS
     (`linux.rs`, `macos/`, `windows_impl.rs`), where it can only
     refine a choice the module has already made — `apply/linux.rs`
     compiles its script-building half everywhere so the tests can run
     off-Linux, and says so with a `cfg`.

  Not anywhere else. In particular a `#[cfg]` block **inside a
  function body** that picks a backend is the shape this rule exists
  to stop: the choice is made once, where the module is declared, not
  again in every function that needs it. A `struct` with a per-OS
  field is the same thing wearing a different hat — give each OS its
  own type and let the dispatch choose.

  `poltertype-app` holds **none**, and `poltertype-core` holds none
  either. All of this is checked by `cargo xtask style`, not by good
  intentions. If you find yourself reaching for `#[cfg(target_os)]` in
  the binary, that is the signal a capability crate is missing. Where the difference is
  a *value* rather than an API, prefer a runtime signal over a
  build-time one: the macOS pause-hotkey default and the Wayland
  switch-last default are both chosen from the live backend name, so
  one `config.toml` means the same thing wherever it is read.

## File organization (one kind of thing per file)

A file's **name** says what it is for, and that decides what may be
declared in it. `cargo xtask style` checks this and the pre-commit
hook runs it, so a violation fails the commit rather than the review.

| File | Contents |
|---|---|
| `mod.rs` / `lib.rs` | module docs, `mod` declarations, `pub use` re-exports — wiring only |
| `main.rs` | wiring plus `fn main`; everything it calls lives elsewhere |
| `consts.rs` | constants |
| `enums.rs` | enums (and their small `impl`s) |
| `types.rs` | plain data structs (and their small `impl`s) |
| `traits.rs` | the traits a crate is built around, and their `impl`s |
| `<purpose>.rs` | free functions grouped by purpose (`heuristics.rs`, `helpers.rs`, `files.rs`, …) |
| `<Type in snake_case>.rs` | a struct with substantial behaviour lives in its own file together with its `impl` (e.g. `db.rs`) |
| `tests.rs` | **all** unit tests — never inline `#[cfg(test)] mod tests { … }` blocks in source files |

A file that declares a type **and implements it** is that type's
file, and the rest of this section is about what may keep it company.
A file that only groups free functions is not — a small private struct
holding state for one of them is part of the function, not a tenant.

Four consequences, because these are the ones review kept catching:

* **A constant another file can name lives in `consts.rs`.** A
  file-private one may stay beside the code that reads it — four
  documented gaps above the one function that places a tooltip are
  context, not clutter. Past a handful in one file they have stopped
  being context and become a table, and a table belongs in
  `consts.rs`. A constant that is part of a type's API is an
  associated `const` in its `impl` instead.
* **One type per file, once the file belongs to a type.** A second
  struct or enum beside a type with its own `impl`s means the file is
  a bag — and it is a bag whether or not the second type is `pub`:
  move plain data to `types.rs` / `enums.rs`, a seam to `traits.rs`,
  and a second type with behaviour to its own file. In a file of free
  functions, where no type claims the file, one exported type is still
  the limit.
* **A type's file is not a workshop around the type.** Up to six free
  functions beside it read as its constructors and near helpers; a
  seventh is a second concern that has moved in, and belongs in a
  `<purpose>.rs` sibling. Sometimes the honest fix is the other way
  round — the type goes to `types.rs` / `enums.rs` and the file turns
  out to have been a function file all along.
* **A module is found by its file name.** No `#[path = "…"]` — the
  directory tree must be readable as the module tree, so unit tests
  for `foo.rs` go in `foo/tests.rs`, not in `foo_tests.rs` pointed at
  by an attribute.

Past 400 lines a type file has stopped being one thing, and the
checker says so. Promote it to its own directory module: the struct
with its fields and constructor in one file, and the `impl` split into
one block per concern, one file per block (fields and cross-file
methods become `pub(super)`).
Example: `crates/poltertype-core/src/engine/switcher/`
(`engine.rs` — the struct; `run_loop.rs`, `echo.rs`, `decide.rs`,
`correction.rs`, `commands.rs` — one concern each).

Unit tests always live in a sibling `tests.rs`, declared from the
parent as `#[cfg(test)] mod tests;`. Existing examples to copy from:
`crates/poltertype-core/src/engine/`, `crates/poltertype-core/src/layouts/`,
`crates/poltertype-detect/src/`, `crates/poltertype-app/src/settings_ui/`.

## Comments

Comments answer **why**, never **what** — the code already says what.
A comment that paraphrases the line under it is noise that goes stale
on the next edit.

| Kind | Budget |
|---|---|
| `//!` module header | a short paragraph: what lives here, and the one or two constraints a reader must not break. Longer only for a module whose whole point is a rule (`commands/shell.rs`, `plugins/validate.rs`) |
| `///` on a public item | the contract — units, error conditions, panics — then one paragraph of *why* if a caller can get it wrong. Two paragraphs means the second one belongs in `docs/` |
| `//` inside a body | only where the reason is not visible locally: a workaround, an ordering constraint, an OS quirk |

The test is whether a reader who deletes the comment would then write
the bug. If not, the comment is decoration.

Keep out of the source and put in a document instead:

- **Design rationale and rejected alternatives** →
  [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), or
  [docs/DECISIONS.md](docs/DECISIONS.md) when it is a dated decision.
- **Incident history** ("this broke on hardware in August, here is the
  story") → `docs/DECISIONS.md` and `CHANGELOG.md`. A one-line
  `// keep X, else Y breaks` in the code is the part worth keeping.
- **Feature catalogues** (every pane of a window, every field of a
  config) → the user docs. They drift the moment a field is added.

Link rather than restate: `see docs/ARCHITECTURE.md § Key gate` beats
forty lines of the same argument copied into a module header.

## Commits

Imperative mood, scope prefix when useful (`engine:`, `win:`, `ui:`,
`ai:`). Reference the phase or doc when the change is design-bearing.

## Releasing

> **Read [docs/RELEASING.md](docs/RELEASING.md) first — all of it.**
> In particular step 2: **syncing the docs is a release blocker.** No
> tag ships while `README.md` and `docs/` still describe
> the previous release. Nothing in CI will catch it for you.

Releases are cut by pushing a `v*` tag. CI ([release.yml]) then
builds four installers in parallel and attaches them to a draft
GitHub Release, along with `latest.json` — the manifest the in-app
updater polls, generated from the exact artifacts being uploaded so
the checksums cannot drift out of step with the files:

| Platform | Artifact | Tooling |
|---|---|---|
| Linux (x86_64) | `.AppImage` | `linuxdeploy` + appimage plugin |
| macOS (universal: Intel + Apple Silicon) | `.dmg` | `lipo` + `hdiutil` |
| Windows (x86_64) | `.msi` | WiX Toolset 3 (`candle` + `light`) |
| all three | `latest.json` | generated in [release.yml] |

Publishing the draft is what ships the update to **every existing
user** — the updater resolves `releases/latest`, which skips drafts and
pre-releases. Sanity-check the artifacts before you publish, not after.

The packaging logic lives in [`installers/`](installers/) so it can
also be run locally — useful when adjusting the WiX template or the
DMG layout without round-tripping through GitHub Actions:

```bash
# Linux
cargo build --release --target x86_64-unknown-linux-gnu -p poltertype-app
cargo xtask assets icon-png target/dist/icon-256.png --size 256
VERSION=local ICON_PNG=target/dist/icon-256.png \
    bash installers/linux/build-appimage.sh

# macOS (run on a Mac)
cargo build --release --target x86_64-apple-darwin   -p poltertype-app
cargo build --release --target aarch64-apple-darwin  -p poltertype-app
VERSION=local \
    BIN_X86_64=target/x86_64-apple-darwin/release/poltertype \
    BIN_ARM64=target/aarch64-apple-darwin/release/poltertype \
    bash installers/macos/build-dmg.sh

# Windows
cargo build --release --target x86_64-pc-windows-msvc -p poltertype-app
choco install wixtoolset --no-progress -y   # one-time
$env:VERSION = 'local'
pwsh installers/windows/build-msi.ps1
```

Installers are **unsigned** — we don't yet have an Apple Developer ID
or a Windows EV/OV cert. The release notes call out the Gatekeeper /
SmartScreen workarounds so users know what to click.

[release.yml]: .github/workflows/release.yml

## Reporting bugs / asking for things

GitHub Issues — please attach:

* `poltertype --version`
* OS / DE / session type
* If the engine's behaviour is surprising, the relevant lines from
  `<config-dir>/poltertype/logs/` (the tray's "Open Logs" entry
  takes you there).
