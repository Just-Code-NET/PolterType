# Changelog

All notable changes to PolterType are recorded here. The format is
loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project follows [Semantic Versioning](https://semver.org/).

## [0.7.0] — updates get signed, ARM64 Linux gets a build, and the setup guide starts checking your machine

> **macOS users, before you update.** This release changes the macOS
> input path: modifier presses now reach the engine (they never did),
> and a correction releases the modifiers you are holding before it
> types. Together they fix a correction fired under a held ⌘ going out
> as ⌘⌫ — "delete to start of line". The code is reviewed, unit-tested
> where the logic is portable, and compiled by CI, but **it has not
> been run on a Mac by anyone**. If typing looks wrong afterwards,
> please say so in
> [#3](https://github.com/Just-Code-NET/PolterType/issues/3) — that
> issue is how this gets confirmed.

### Security

- **The release manifest is signed, and the updater checks it.** Until
  now the only thing standing between a user and a hostile update was
  a SHA-256 that shipped in the same GitHub release as the installer —
  so whoever could publish one could publish both. `latest.json` now
  carries a detached ed25519 signature, verified against a public key
  compiled into the binary, the moment the manifest is parsed and
  before any URL in it is read. The private key is **not** a CI secret
  and never touches a runner: signing is a manual step the maintainer
  performs on the draft release (`cargo xtask manifest sign`), which is
  the only version of this that a compromised GitHub account cannot
  forge.
  **Not yet mandatory.** A *wrong* signature is refused from this
  release on, but a *missing* one is still accepted — otherwise every
  user would be stranded on the last unsigned manifest. Enforcement is
  a one-constant flip in a later release, and only then does anything
  user-facing get to say "signed updates".

### Added

- **A Setup pane that checks this machine instead of linking to a
  document.** When the keyboard hooks fail to start, the tray alert now
  opens the Settings window on a new **Setup** pane rather than a
  markdown file in a browser. It probes the running system and says
  what is actually missing, per OS: on Wayland, whether key events can
  be read and whether corrections can be typed, as two separate
  answers, because they are two separate permissions and the half-
  granted case (detection works, nothing gets fixed) is the confusing
  one. On macOS, Accessibility and Input Monitoring separately, with
  buttons that ask the system for each and deep links into the right
  System Settings pane. On X11 and Windows it says there is nothing to
  grant — most people arrive expecting the worst. *Check again*
  re-probes, and answers even when nothing changed.
  It also catches the trap that wastes an evening: `usermod -aG input`
  updates the group database and cannot touch a login session that
  already exists, so everything looks configured and nothing works.
  That state gets its own answer — log out, don't re-run the script.
  Nothing on the pane changes the system. The Linux script needs
  `sudo`, so the button copies the command for the user to read and run
  themselves.
- **An honest banner when layout switching is unavailable.** Hooks
  working and no switcher backend is its own failure: PolterType spots
  the wrong-layout word and rewrites it into the same wrong layout, so
  it looks like the correction is broken rather than missing.
- **The suggestion tooltip anchors to the caret on GNOME and KDE
  Wayland.** Those sessions have no compositor-agnostic active-window
  query, so the focus tracker was a plain no-op there and the tooltip
  fell back to the bottom of the screen. But AT-SPI is a session-bus
  service and answers on any compositor — the caret watcher had simply
  never been built on that path. It is now: `focused_exe()` still
  returns `None` (nothing keyed off the focused app starts guessing),
  while the tooltip gets the *best* anchor in the chain instead of the
  worst.

### Documented

- **Packaging manifests for AUR, winget and Homebrew**, staged in
  `packaging/` with the publish step for each written down. Nothing is
  live yet — and the README install table stays silent until each one
  is. `packaging/bump.sh <version>` re-points all three at a published
  release by hashing the bytes GitHub actually serves. Two decisions
  worth knowing: the AUR packages install the udev rule but will not
  add anyone to the `input` group (that is the user's account, not
  ours), and the Homebrew cask does **not** strip macOS quarantine —
  removing that check silently for an unsigned app that reads every
  keystroke is not a convenience we get to hand out.
- **The tooltip was never broken on KDE.** KWin has implemented
  `zwlr_layer_shell_v1` for years; verified against KWin 6.7.3, where
  the surface configures and maps exactly as on Hyprland. GNOME
  Wayland is not a no-op either — Mutter has no layer-shell, but the
  X11 override-redirect fallback maps through XWayland. Five places
  claimed otherwise, including the error message users would see. The
  backend has always *probed* rather than matched desktop names; the
  prose was a hand-maintained list that went stale silently. The real
  remaining gap is a Wayland session with neither layer-shell nor
  XWayland, plus macOS and Windows.
- **No Flatpak, decided with evidence rather than left open.** The
  emitter writes to `/dev/uinput`, which no Flatpak permission grants
  short of `--device=all` — `device=input` deliberately excludes it —
  and there is no portal. Layout switching additionally needs host
  binaries a sandbox does not have. Reasoning, sources and the
  conditions for revisiting are in `docs/DECISIONS.md`; the README
  says so plainly so nobody has to ask twice.

- **aarch64 Linux builds.** `poltertype-<ver>-aarch64.AppImage` ships
  alongside the x86_64 one, built natively on an ARM64 runner rather
  than cross-compiled. Raspberry Pi 5, Asahi and ARM laptops/servers
  had nothing to download and nothing for the in-app updater to offer
  them; both now work. Deliberately the *only* architecture added —
  every installer is a support surface, so armv7 and ARM Windows stay
  out until there is hardware and demand behind them.

### Fixed

- **macOS: a suggestion accepted with its chord still held no longer
  retypes the word under those modifiers.** `release_modifiers` was a
  default no-op on macOS, and — worse — every event we posted inherited
  the *live hardware* modifier flags from its `HIDSystemState` source,
  so with ⌘ down our backspaces went out as ⌘⌫ ("delete to start of
  line"). The emitter now clears the flags on everything it posts and
  sends a `FlagsChanged` release for each modifier the engine believes
  is down. Caps Lock is deliberately left alone — it is a latch, not a
  held key.
- **Windows: the same no-op, the same bug.** `release_modifiers` now
  sends key-ups for both sides of each held modifier.
- **macOS: modifier presses reach the engine at all.** The event tap
  subscribed to `KeyDown`/`KeyUp` only, but macOS reports a modifier
  moving as `FlagsChanged` — so the modifier arms of the keycode table
  had been unreachable since they were written, and the engine only
  learned what was held when an ordinary key arrived. It now sees the
  same discrete modifier stream as the Windows and Linux backends,
  which is what makes the fix above fire at the right moment. Keys with
  no SC Set-1 equivalent (Fn, media) are dropped rather than falling
  through the identity mapping into the word buffer's "navigation — end
  the word" range.

### Internal

- The macOS backend is a directory rather than one file, and the part
  most likely to be wrong — the Apple→SC Set-1 keycode table and the
  `FlagsChanged` direction rules — carries no Apple dependency, so its
  tests run on Linux and Windows CI too. Everything Mac-only stays
  compile-checked by CI's `macos-latest` job.

## [0.6.3] — the key gate grows a safety catch, corrections learn ñ, and the logs forget your words

### Security

- **Typed words no longer appear in logs — at any level, in any
  build.** The decision diagnostics embedded the word they judged
  ("current \`…\` is a dictionary word", `original=… corrected=…`),
  and the correction summary logged both words at INFO — the default
  level — so a release build with default settings wrote typed words
  into the on-disk log, contradicting the README's privacy promise.
  Every such site now renders words as `<N chars>`. Developers can
  see the words in a **debug build only** by setting
  `POLTERTYPE_UNSAFE_LOG_WORDS=1`; release builds redact
  unconditionally, at compile time.

### Fixed

- **Linux: the key gate can no longer freeze the whole session's
  input.** The gate's "is our emitter proxied by a remapper?" probe
  ran once, at startup — racing keyd's own asynchronous grab of the
  freshly created device. Winning that race armed the gate on a stack
  where it must stand down, and the first correction then grabbed the
  remapper's virtual keyboard: every input path — the user's keys and
  our own corrections alike — funnelled into PolterType, and the
  session's input died until a reboot. The gate now re-verifies the
  emitter before **every** hold and shuts itself off for the rest of
  the run the moment the emitter turns busy. The emitter also records
  its device node at creation, so the never-grab-our-own-device
  exclusion works by identity rather than name comparison.
- **Wrong-layout words containing a cross-layout letter now get
  corrected.** Typing `mañana` or `español` with the US layout active
  renders as `ma;ana` / `espa;ol` (`ñ` sits on the US `;` key) — and
  both detectors used to freeze exactly this case: the plausibility
  scorer ignored the `;` entirely (`espa;ol` scored a perfect en-US
  fit and vetoed the switch), and the dictionary detector looked up
  the letters-only skeleton, where over-inclusive bulk entries like
  `maana` / `seor` produced a phantom "current is a real word" veto.
  Interior punctuation (apostrophes and hyphens exempt) now crushes a
  rendering's plausibility and demotes skeleton dictionary hits from
  a veto to a tiebreaker, so the es–en pair the landing page demos
  works end-to-end.
- **Hyprland: the "Spanish" keymap now resolves to `es-ES`.** The
  pretty-name table covered every bundled language except Spanish, so
  the moment a correction switched the system to the Spanish layout
  the engine stopped recognising its own current layout — renders
  came back empty and every subsequent word got a phantom
  re-correction.
- **Hyprland: the "Russian" keymap no longer resolves to `en-US`.**
  The `us` shorthand in the same table matched as a substring, and
  "R**us**sian" (as well as "Belar**us**ian") contains it — so a user
  switching to the Russian layout had the engine convinced they were
  typing English. Found by the new all-bundled-languages round-trip
  test.

## [0.6.2] — PolterType runs on a Mac, and starts when you sign in

PolterType runs on a Mac for the first time, and the "Start
automatically when I sign in" checkbox does something for the first
time — on every platform.

### Fixed — macOS

The macOS backend shipped in v0.5.0 had only ever been compiled by CI.
The fixes below come from an outside contributor running it on real
hardware (macOS 15, Intel), where it turned out to crash on launch and
never process a keystroke.

- **The app wouldn't launch from Finder at all.** The single-instance
  lock was created under the process working directory, which is the
  read-only system volume for GUI launches; startup aborted with
  "Read-only file system". The lock now lives in the per-user config
  directory.
- **SIGILL seconds after launch.** HIToolbox asserts the main dispatch
  queue inside Text Input Services on modern macOS; calling TIS from
  the layout-poller / engine threads killed the process. All TIS calls
  are now routed through the main dispatch queue.
- **The keyboard tap delivered nothing.** The tap thread ran its
  CFRunLoop in `kCFRunLoopCommonModes` as the run mode; the tap source
  never fired. It now runs in the default mode.
- **Every second word was skipped.** Emitted backspaces / retyped text
  echoed back through the event tap untagged and poisoned the word
  buffer after each correction. All posted events are now stamped so
  the listener recognises them as injected.
- **Shift / Caps Lock state was invisible to the engine.**
  `CGEventFlagAlphaShift` now folds into the shift bit, matching the
  X11 backend; the keycode table gained full modifier / navigation /
  F-row mappings so caret-moving keys end a word the way they do on
  Windows and Linux.
- **Russian / Ukrainian layouts weren't detected** on systems with the
  PC ("Win") input-source variants — `RussianWin` / `UkrainianWin`
  (and `ABC`, the modern US id) are now mapped, and layout switching
  matches sources by their mapped BCP-47 id.
- **The app icon no longer lingers in the Dock.** tao applies the
  Regular activation policy by default, overriding `LSUIElement`; the
  tray app now runs as an Accessory process.
- **The pause hotkey no longer steals the system layout switcher.**
  `Ctrl+Shift+Space` is macOS's own "select previous input source", so
  the default there is now `Ctrl+Shift+P`. Applied only while you are
  on the default; an explicit binding is honoured as written.

### Added

- **"Start automatically when I sign in" now does something.** The
  setting has existed since the first release, defaulting to *on*,
  while no code ever read it — the app had never started at login on
  any platform. It now registers a per-user LaunchAgent on macOS, an
  `HKCU` run-key value on Windows and an XDG autostart entry on Linux.
  Unticking it removes the entry.
- **The Accessibility permission prompt.** When the event tap can't
  attach, macOS is now asked to show the system prompt instead of the
  app failing silently into a dead tray icon. Note macOS also requires
  **Input Monitoring** for key delivery; it prompts for that when the
  tap is created.
- Settings UI shows the platform's modifier glyphs (⌃⌥⇧⌘) on macOS.

### Fixed

- **A correction could be declined on a busy machine.** The intrusion
  probe bounded itself by wall-clock while being driven by its own
  sleeps, so under load the deadline expired before the run of silence
  that authorises a repair could accumulate — and the engine left the
  text alone when it should have fixed it. It now counts samples
  instead, which is the same bound without the race. This also fixes
  an intermittent CI failure on macOS.

### Removed

- `auto-launch`, which had been declared since the first commit and
  used by nothing.

## [0.6.1] — the suggestion tooltip stops chasing your mouse

### Fixed — the suggestion tooltip lands where you are typing

Two bugs put the tooltip somewhere other than the text being corrected.

- **The tooltip followed the mouse, not the caret.** When the focused
  app exposed no AT-SPI caret, the anchor fell back to the pointer
  position, on the theory that the user had just clicked into the text
  they were editing. Nothing checked that the pointer was still *at*
  that click, so an idle mouse parked mid-screen pulled the tooltip to
  the middle of the display while the caret sat in a chat box at the
  bottom edge. The pointer anchor is gone: without a caret the tooltip
  now hangs above the bottom edge of the focused window, which is the
  neighbourhood of the chat inputs and shell prompts this feature is
  for.
- **The first tooltip of every session was placed blind.** On Wayland
  the popup thread parks on its command channel between popups and
  reads nothing from the compositor, so the outputs' names, sizes and
  scales — which arrive as events, not with the globals — had not been
  received when the first popup was built. That popup got no screen
  bounds to clamp against and no named output, leaving the choice of
  monitor to the compositor while the coordinates had been computed for
  a different one. The popup thread now refreshes its output state
  before every show, which also picks up hotplugs and mode changes that
  happened while it was parked.

### Fixed — no more appindicator deprecation notice on Linux

Every start printed `libayatana-appindicator is deprecated. Please use
libayatana-appindicator-glib in newly written code.` to stderr. The
notice is aimed at whoever links the library — `tray-icon`, through
`libappindicator` — and there is nothing a user can do about it. It now
goes to PolterType's own log at debug level instead of the journal, so
it stays available to us without being noise for you. New
`poltertype-tray` crate, which exists so the binary crate can keep
holding no platform-conditional code at all.

## [0.6.0] — PolterType stops tripping over your typing

### Fixed — corrections no longer scramble the word you type next

Typing `зтзь ` (i.e. `pnpm ` on the wrong layout) and carrying straight
on with the next word could leave `ipnpm ` on screen — the `i` in front
of the correction rather than behind it, and `pinpm ` / `pnpmi ` when it
landed further in. The keystroke was reaching the application *inside*
the correction's own burst, where the compositor interleaves it with our
injected keys and no amount of counting afterwards can place it.

- **The blind settle sleep before a replay is gone.** Both Linux
  emitters paused 30 ms right before typing the corrected word, to let
  the compositor finish propagating the new keyboard layout. That pause
  sat between our last look at the key stream and our first emitted
  key — precisely the window a keystroke slipped into. The engine owns
  that wait now, measured from the actual layout switch and taken before
  the deletion, where it costs nothing (the absorb gate has usually
  covered it several times over already).
- **The engine looks at the key stream later, and once more.** The probe
  that catches keys racing the deletion now allows for the trip from
  device to listener thread, so keys pressed during the burst are seen
  while they can still be placed.
- **A keystroke that gets in anyway is repaired.** After emitting, the
  engine checks whether anything landed inside its own burst and, if the
  user has since paused, erases what it typed — intruder included — and
  retypes it all in the order it was typed. The repair waits for that
  pause on purpose and is budgeted: a correction must never end up
  chasing a still-typing user down the line, so if no pause comes it
  leaves the text alone and stops vouching for the screen instead.

### Added — corrections hold your keystrokes instead of racing them

Repairing a scrambled correction is treating the symptom. On
Linux/Wayland PolterType now **holds the keyboard back** for the length
of a correction burst (`EVIOCGRAB`) and types out whatever you pressed
meanwhile itself, in the order you pressed it. Typing a whole command
straight through in the wrong layout — `зтзь ш кгт ` at a real typing
cadence — went from 4 wrong results in 6 to none.

- **It stands down where it would do harm.** Behind an input remapper
  (keyd and friends) the only grabbable source of your keystrokes also
  carries PolterType's own, so grabbing it would block the correction
  itself. PolterType detects that at startup and quietly keeps the
  detect-and-repair behaviour instead — `docs/PERMISSIONS.md` has the
  keyd one-liner if you want the stronger path. `POLTERTYPE_HOLD_KEYS=0`
  turns it off entirely.
- **It cannot leave your keyboard dead.** The thread that owns the
  devices drops the hold after 1.2 s no matter what the engine is
  doing, and a crashed process releases it by construction. Backspace,
  arrows and Esc pressed during a burst are typed out too rather than
  swallowed.

### Fixed — accepting a suggestion with the keyboard actually replaces the word

`Ctrl+Meta+<digit>` (and the default `Ctrl+Shift+<digit>`) did nothing
visible. The accept itself was working the whole time — the replacement
was simply typed while the chord's own modifiers were still held, so
every key of it arrived at the application as a shortcut rather than a
character. Corrections now wait for the chord to come up before typing,
and ask the emitter to release what is held; the manual switch-last
hotkey had the same flaw and is fixed by the same change.

The digit itself is erased along with the word now, too. Chords are
matched off the key stream rather than grabbed — registering nine
global hotkeys would take those combinations away from every
application — so the digit reaches the document on its way past, and
was being left behind in the replaced text.

### Fixed — the input thread no longer goes blind every 2 seconds

The device rescan that picks up hot-plugged keyboards re-opened every
node under `/dev/input` and read its capabilities — 70–140 ms on the
same thread that reads your keystrokes, every 2 seconds. Events piled
up in kernel buffers and arrived late in a burst, right where the
correction logic is at its most timing-sensitive. It now opens only
devices that are genuinely new. A keyboard unplugged and plugged back
into the same port is also picked up again, which the first version of
this fix would have missed.

## [0.5.0] — PolterType starts fixing your typos, not just your layout

### Added — spelling suggestions for plain typos

Until now PolterType only helped when the *layout* was wrong. Typos
typed in the *right* layout — `слоао`, `hwllo` — got nothing. Now,
when a completed word isn't a dictionary word (and isn't something
the engine would auto-correct), a small tooltip appears near the
focused window with up to 5 nearby dictionary words. Click one, or
press `Ctrl+Shift+<digit>`, and the word is replaced in place —
including any separators and next-word keystrokes you'd already
typed.

The details that make it behave:

- **Candidates come from the bundled dictionaries** via a new
  surface-form FST per language, so `п'ять` is suggested *with* its
  apostrophe. Ranking is keyboard-aware: substituting a key with its
  physical neighbour ranks higher than a random edit, and adjacent
  transpositions count as a single slip.
- **Low-confidence layout verdicts join the list.** When the detector
  saw a cross-layout word but stayed below the confidence threshold,
  that candidate now leads the tooltip (badged with the layout)
  instead of being dropped — you make the call the engine wasn't sure
  enough to make.
- **The tooltip appears next to the text you're typing.** It anchors
  to the real caret via accessibility (AT-SPI) in apps that expose
  it, falling back to the pointer, then the focused window.
  PolterType raises the session's `org.a11y.Status.IsEnabled` flag so
  application a11y bridges wake up — apps already running before
  PolterType's first launch join in after they restart. Placement
  picks whichever side of the caret has room (above first, then
  below/right/left) and never covers the line being typed. Clicks on
  the tooltip are carefully disambiguated from clicks that move the
  caret — mid-text insertions replace exactly the mistyped word and
  leave the surrounding text alone.
- **The tooltip never takes keyboard focus** (Wayland layer-shell on
  Hyprland/Sway, override-redirect on X11) and hides itself after 30
  seconds, on Esc / click elsewhere / caret movement, or the moment
  it can no longer act on the word. GNOME/KDE Wayland, macOS and
  Windows have no overlay backend yet — the feature quietly stays
  engine-side there.
- **"Add to dictionary" lives in the tooltip.** The last row adds the
  flagged word to your wordlist overlay
  (`<config-dir>/poltertype/wordlists/<stem>.txt`) with one click or
  digit — jargon, names and project vocabulary stop being flagged
  immediately and permanently. No tooltip appears at all for words
  typed right after a click / arrow keys / Esc: the typed keys may
  be a fragment of a longer word on screen, and a suggestion
  computed on a fragment would corrupt it if accepted.
- **Local, silent, off-switchable.** No network, nothing typed ever
  reaches a log, and `[suggestions] enabled = false` (or the new
  Settings → Suggestions pane) turns the whole thing off. Defaults:
  on, 5 suggestions, 30 s, `Ctrl+Shift` + digit (the modifier half is
  configurable — e.g. `accept_modifiers = "Ctrl+Meta"`).

### Fixed

- The Windows MSI never shipped `uk_ua-weak.txt`, so the weak-word
  deferral (the `туче` → `next` case from 0.4.x) silently didn't work
  on Windows installs. The WiX manifest lists each data file
  explicitly and the weak list was forgotten; it is packaged now.

## [0.4.2] — PolterType stops muting itself in your editor

### Fixed — PolterType no longer arrives with your editor pre-muted

`[exceptions].disabled_apps` shipped with a ~50-entry default skip-list
— VS Code, Cursor, the JetBrains IDEs, Sublime, Zed, kitty, alacritty,
konsole, PowerShell, tmux and more. On Linux it had never done anything:
`focused_exe()` returned `None` there, so the list could not match. The
Hyprland/X11 focus tracker added in 0.3.0 made it real, and PolterType
abruptly went silent in exactly the windows developers type in — no
error, no notification, nothing above `DEBUG` in the log. It reads, from
the outside, as "layout switching is broken".

**The default list is now empty.** PolterType corrects everywhere until
you tell it not to. What keeps the corrector out of your code is
unchanged and never depended on knowing which app has focus: the
identifier guard (`engine.suppress_in_identifiers`), plausibility-keep,
`min_word_length`, and the dictionary confidence threshold.

The skip-list itself still works and is still honoured — it is now
opt-in. Add apps in `config.toml` or on the Settings → Exceptions pane.

**Existing installs are migrated on first launch.** Shipping an empty
default in the binary would have fixed nothing for anyone who already
ran an older build: those 69 entries are written into *their*
`config.toml`, and the app reads the file, not the default. So this
release clears the list out of the file — but only when it is still
the shipped default, entry for entry. Take one app out of the list, or
add one of your own, and PolterType treats it as yours and never
touches it. The migration is logged at `INFO` when it fires.

## [0.4.1] — the three installers finally agree on how to spell a version

### Fixed — the Windows installer is named like the other two

The MSI shipped as `poltertype-v0.4.0-x86_64-pc-windows-msvc.msi` while
the AppImage and the DMG dropped the tag's `v`
(`poltertype-0.4.0-…`). The build script was passing the raw git tag
into the file name instead of the stripped version — long-standing, and
harmless to the updater (it matches artifacts by pattern), but it made
the three downloads on a release page look like they came from
different projects, and it is a trap for anyone scripting a download by
filename. The README already documented the `v`-less form; now the
build agrees with it.

Pre-release tags keep their suffix in the file name
(`poltertype-0.5.0-rc.1-…msi`), so a release candidate can never
collide with the final release it precedes.

## [0.4.0] — PolterType keeps itself up to date

### Added — PolterType keeps itself up to date

Until now, updating meant noticing that a release had happened and
re-running an installer by hand. Since the installers are unsigned and
there is no store to push through, that meant most people simply stayed
on whichever build they first installed — including for security fixes.

PolterType now updates itself. Once a day it fetches a small manifest
from GitHub Releases, and when a newer version is out it downloads the
installer for your platform in the background and verifies its SHA-256
against the manifest. Then it stops and waits.

**Nothing is ever installed while you're typing.** The app holds a
global keyboard hook; swapping its binary mid-sentence is the one thing
it must not do. The staged update installs when you quit the app, or
when you click the new **⟳ Restart to update — v0.4.0** entry in the
tray menu. A notification tells you once when a version is ready; the
same tray entry doubles as a manual **Check for updates…** when nothing
is staged.

All three platforms self-update: the MSI is reinstalled per-user (no
UAC), the AppImage is swapped in place, and the macOS `.app` bundle is
replaced from the DMG. An install that *isn't* ours — a distro package,
a `cargo build` binary — is never overwritten; you get a notification
pointing at the Releases page instead.

### Changed — the app now makes exactly one network call

This is a real change to what PolterType is, so it is stated plainly
rather than buried: previous versions never opened a socket, and this
one does.

The update check is a plain `GET` of a static JSON file on `github.com`.
There is no request body, no query string, no account and no identifier.
GitHub learns what any web server learns — your IP, and a User-Agent
naming the version you're running. Nothing about you, your layouts, your
configuration or a single character you type is transmitted, ever. This
is not telemetry, and PolterType still has none of any kind.

If you want a build that never touches the network at all, that is one
checkbox — **General → Updates** in the Settings window, or:

```toml
[updates]
enabled              = false
check_interval_hours = 24      # clamped to a 1-hour floor
```

Existing `config.toml` files don't need editing: the section defaults in.

### Known limits, stated up front

- **The download is verified, not signed.** The SHA-256 comes from the
  same release as the installer, so it catches a corrupted download or a
  tampered CDN — but not a compromised GitHub account. Signing the
  manifest with a key held off GitHub is the real fix and is planned;
  the manifest already carries a reserved `signature` field.
- **The macOS updater has not been run on a Mac.** macOS is a CI-only
  target for this project. The `.app`-swap path follows Apple's docs and
  the Windows and Linux paths are exercised, but treat macOS
  self-updating as unproven for now.

## [0.3.1] — the Settings window wears the brand, in light and dark

### Changed — the Settings window wears the brand now

The Settings window used to render in iced's stock theme — grey,
unbranded, and unrelated to how the product presents itself anywhere
else. It now shares a visual language with poltertype.com: the same
design tokens (brand indigo, ink/muted text, "ecto" green for
success, "garble" pink for danger), the GhostMark keycap-ghost logo
in the sidebar (drawn as vectors — no image assets, no SVG renderer
dependency), pane content grouped into hairline cards, and hotkeys
rendered as physical keycap chips, the same way the site draws
hotkey chords.

Both light and dark variants ship. The default follows the OS
setting; a new **Appearance** picker on the General pane pins it to
Light or Dark explicitly, persisted as `[general].ui_theme` in
`config.toml` (`"system"` / `"light"` / `"dark"`; unknown values
fall back to `"system"`).

Smaller UX fixes in the same pass: the About links (site, repo,
issue tracker) are real buttons that open the browser instead of
non-clickable text, the About pane shows the resolved config path,
and per-pane status banners use the shared success/danger colours
instead of hardcoded RGB values.

Two things had to be fixed for the above to hold up. Following the
OS appearance now works beyond GNOME/KDE: the detection iced ships
mis-parses the XDG desktop portal's reply and falls back to "light"
on Hyprland-class desktops, so the window now asks the portal (and
the GNOME gsettings key) itself. And switching themes at runtime
exposed rendering bugs in iced 0.13's CPU compositor — the window
blinked between the new palette and a stale old-theme frame while
the mouse moved — which the window now sidesteps by forcing a full
repaint on every UI change (imperceptible; it only redraws on input
events anyway).

## [0.3.0] — per-app features land on Linux, and the tray admits hook failures

### Added — focus tracking on Linux (Hyprland + X11)

Per-app features stop being Windows-only. The focus tracker — the
component that answers "which app is the user typing into?" — now has
two Linux backends: Hyprland (over the compositor's IPC socket, the
same transport the layout switcher uses) and X11 (EWMH
`_NET_ACTIVE_WINDOW`). Both report the focused process's executable
basename, exactly like the Windows tracker, so `[exceptions].disabled_apps`,
per-app wordlist profiles, and `apps = [...]` scoping on smart
commands now work on those setups with the same config values you'd
write on Windows (minus the `.exe`). GNOME and KDE on Wayland still
have no active-window query — the tracker stays a no-op there.

### Added — the tray now tells you when keyboard hooks are unavailable

Previously, when the keyboard listener failed to start — most commonly
a Wayland session without `input`-group access — the tray came up
looking perfectly healthy while the app silently did nothing. Now the
failure is surfaced three ways: a "⚠ Keyboard hooks unavailable —
Setup Guide…" entry at the top of the tray menu (opens the permissions
guide in your browser), a warning suffix on the tray tooltip, and a
one-time system notification at startup explaining what happened and
where the fix is.

### Added — a settings-window screenshot in the README

### Fixed — one more "Poltertype" → "PolterType" in the settings window

The Languages panel's helper text was still spelling the product name
in lowercase.

## [0.2.2] — the name is spelled "PolterType" everywhere it is shown

### Changed — the displayed product name is now "PolterType"

Everywhere the app wrote its own name for a human to read, it used the
spelling `Poltertype`. The brand is **PolterType**. The settings window
title, the tray tooltip, the system notifications, the About entry in
the tray menu, `--version` and `--help`, the README files seeded into
the user's layouts and wordlists folders, and the installer metadata —
the Linux `.desktop` entry, the macOS bundle name, and the Windows
product name shown in Add/Remove Programs — all agree on it now.

This is a display-only change: nothing moves on disk and no setting is
lost. The app id stays `dev.opensource.poltertype` and the config and
data directories stay `poltertype`, because those are identifiers, not
the brand. The Windows installer's product folder and registry key are
derived from the product name and so change case with it, which is
harmless — both the filesystem and the registry are case-insensitive
there, and the upgrade is keyed on the MSI upgrade code regardless.

## [0.2.1] — "Settings…" survives an in-place update

### Fixed — "Settings…" did nothing after the app was updated in place

Replacing the binary while the tray kept running — an in-place package
upgrade, or a `cargo build` during development — made the **Settings…**
tray entry a silent no-op, permanently, until the app was restarted.

The cause is how Linux reports a running process's own path: once the
binary behind `/proc/self/exe` is unlinked, the kernel keeps resolving
the link but appends a literal ` (deleted)` to it, and
`std::env::current_exe()` hands that string back verbatim. The tray
spawns the Settings GUI as a copy of itself, so it was trying to
execute a file called `poltertype (deleted)`, getting `ENOENT`, and
giving up with nothing but a `warn!` line in a log the user has no
reason to look at.

The tray now recognises that path shape and launches the binary that
actually sits there — the freshly installed one. When there is nothing
left to launch (the app was uninstalled or the build directory wiped),
it says so with a system notification instead of failing silently.

## [0.2.0] — PolterType rename, Linux X11 support, Hyprland layout fix

The rename lands in full — binary, crates, config directory and
data-dir env var all become PolterType, with an existing `kb-switcher`
configuration adopted automatically on first launch — together with
X11 support and a Hyprland fix for corrections that fired in one
direction only on input-remapper setups.

### Added — Linux X11 support

X11 sessions are now fully supported, and unlike Wayland they need
**no setup at all**: no `input` group, no udev rule, no `sudo`, no
`setup-linux.sh`. Everything the app needs is available to any client
that can open the display.

* **Listener** — `XInput2` raw key events selected on the root window.
* **Emitter** — `XTestFakeInput`, both for replaying the corrected word
  as scancodes and (for smart-commands) for typing arbitrary Unicode by
  temporarily binding a keysym to a spare keycode.
* **Layout switching** — XKB group locking (`XkbLatchLockState`), for
  bare window managers (i3, openbox, a hand-rolled `.xinitrc`) where no
  desktop environment owns the layout. On an X11 session that *does*
  run GNOME / KDE / IBus / Fcitx, those backends still win, so their
  tray indicator stays in sync with the keyboard.

Session detection also stopped relying on `XDG_SESSION_TYPE` alone —
plenty of bare-WM setups never set it, which is exactly the crowd this
backend is for. It now falls back to the display sockets, and correctly
picks the Wayland path under XWayland, where the compositor owns input.

### Fixed — a GTK-only machine no longer claims the GNOME layout backend

The `org.gnome.desktop.input-sources` schema ships with GTK, so it is
installed on many machines running no GNOME-family desktop at all.
The probe accepted it on the strength of the schema alone, then read
back an empty input-source list — leaving a switcher with nothing to
switch between, and shadowing the backend that would have worked. It
now requires the schema to list at least one input source.

### Changed — project renamed: kb-switcher → PolterType

The working title `kb-switcher` is retired. Everything brand-visible
moves to the new name: the binary (`poltertype`), the crates
(`poltertype-*`), the app id (`dev.opensource.poltertype`), the macOS
bundle id (`org.poltertype.app`), the config directory
(`~/.config/poltertype/` and OS equivalents), the data-dir override
env var (`POLTERTYPE_DATA_DIR`, was `KB_SWITCHER_DATA_DIR`), and the
installer/product names. On first launch the app adopts an existing
`kb-switcher` config directory automatically: `config.toml` plus the
wordlist / layout overlays are copied into the new location (nothing
in the new directory is ever overwritten, and the old directory is
left in place as a backup).

### Fixed — build script baked the checkout path into itself

`crates/poltertype-core/build.rs` (and `xtask`) resolved the repo
root with the compile-time `env!("CARGO_MANIFEST_DIR")` macro, which
freezes the absolute path of the checkout that compiled them. After
moving or renaming the working copy, the cached build script kept
reading wordlists and layout mappings from the old path — silently
producing empty dictionaries and stale mappings in
`target/dist/data`, which disabled layout detection entirely in dev
builds. Both now read `CARGO_MANIFEST_DIR` from the environment at
run time.

### Fixed — Hyprland: stop trusting our own emitter for the current layout

One direction of correction could silently die while the other kept
working (typically "uk→en fires, en→uk never does"). Root cause:
`current()` read the active keymap of the keyboard Hyprland flags
`main:` — but Hyprland re-elects `main` when devices appear, and
right after our uinput emitter registers, the emitter itself is
often promoted. Its keymap only tracks our own `switchxkblayout all`
calls, never the user's per-device Alt+Shift toggle (which lands on
the keyd/remapper virtual keyboard the physical keystream flows
through). After the first correction plus one manual toggle, the
engine's idea of "current layout" was permanently wrong for one
direction: a Ukrainian word typed under en-US mapped to "already
valid Ukrainian" and was vetoed. The guard that was supposed to skip
the emitter compared the raw device name (`poltertype virtual
keyboard`) against Hyprland's dash-normalised output
(`poltertype-virtual-keyboard`) and never matched. `current()` now
normalises names before comparing, never considers the emitter, and
prefers an input-remapper virtual keyboard (keyd / kanata / kmonad)
over `main:` — when a remapper is present, its device is the one
whose keymap reflects what the user is actually typing.

## [0.1.1] — ALL-CAPS suppression + trailing-space fix on Wayland

Two follow-up fixes for the most common "the corrector glitched on me"
reports against 0.1.0 — both Linux/Wayland symptoms, both pure-Rust
core / listener changes.

### Fixed — ALL-CAPS abbreviations are no longer "corrected"

Typing a word entirely in uppercase (`URL`, `HTTP`, `API`, `ССЫЛКА`,
…) by holding Shift or via Caps Lock is almost always deliberate —
an abbreviation or a shouted word — not someone "in the wrong
layout". The auto-switch detector would occasionally take the bait
on these tokens (an ALL-CAPS string often happens to render as
something letter-like in the other layout) and replace the
abbreviation with gibberish. The engine now skips auto-switching for
buffers where every cased letter is uppercase and there are at least
two of them. Mixed-case (`Hello`, `iPhone`, `IPv4`) and single
capital letters (sentence starts, `I` / `Я`) are unaffected; the
manual switch-last hotkey (`Ctrl+Shift+Backspace`) still works on
ALL-CAPS buffers for the rare case where the user really did want to
flip layouts. Controlled by `[engine].suppress_for_all_caps`
(default: on).

On Linux/Wayland the listener folds Caps Lock into the effective
shift bit, so both held-Shift and Caps-Lock-on variants are caught.
On Windows / macOS only the held-Shift variant is caught for now —
folding Caps Lock into the modifier on those backends is a separate
per-OS listener change.

### Fixed — corrector no longer eats the trailing space on Wayland

The long-standing report "corrected words run together — the space
gets cut" turned out to be a held-key bug, not a coalescing one. The
boundary key (almost always Space) that triggers the correction is
still physically held down when our uinput replay reaches it: the
user just pressed Space, the engine reacted within ~10 ms, but human
fingers don't release that fast. Injecting a *press* for an already-
down key is a no-op at the compositor — global key state is already
"down", so no character is produced. The replay now emits a release
for the boundary scancode before its press, clearing the held state
regardless of whether the user is still holding the key (a harmless
no-op if they already let go). The following press is then a real
down-edge and reliably produces the trailing space / newline.

## [0.1.0] — First stable

First stable release — drops the `-beta` pre-release suffix. No new
features beyond the fixes below; this version marks the Linux/Wayland
path as working well enough on the maintainer's daily-driver setup
(Hyprland + keyd) to leave beta.

### Fixed — never re-press Enter/Tab during a correction

Auto-correction re-emits the boundary key after the corrected word.
When that boundary was Enter, the correction pressed Enter a second
time — in a terminal that ran a spurious command (e.g. typing
`podman start --all`, hitting Enter, and having a stray `і` typed and
executed at the next prompt); in a chat app it would send a message.
The engine now treats Enter / Return / Tab as submission boundaries
and never auto-corrects on them. The manual switch-last hotkey is
unaffected.

### Fixed — clipboard paste no longer gets "corrected"

Pasting text with `Ctrl+V` (or `Ctrl+Shift+V` / `Shift+Insert`) could
trigger an auto-correction of the pasted word. A paste isn't typing
and must never be retyped into another layout, but on Wayland the
compositor / input remapper (keyd & friends) can replay the inserted
text through a virtual keyboard, where it is indistinguishable from
human keystrokes. The engine now opens a short window after any paste
shortcut during which it declines to auto-correct, so pasted content
is left exactly as-is. The next genuinely-typed word is unaffected.

## [0.1.0-beta.16] — Wayland keystream hotkeys + evdev reconnect

### Added — Wayland hotkeys handled off the key stream

On the Wayland/evdev backend the OS-level `global-hotkey` grab never
sees native input — it can only bind through Xwayland, which Hyprland
and friends don't route real keystrokes into. So the pause and
switch-last hotkeys silently did nothing on a pure Wayland session.
The evdev listener already observes every key, so the engine now
matches the hotkey chords straight off that stream instead. Detection
is rising-edge (one fire per physical press, autorepeat ignored) and
requires an exact modifier match, so `Ctrl+Shift+Space` never fires on
`Ctrl+Shift+Alt+Space`. The two paths are mutually exclusive per
backend, so there's no double-fire on Windows/X11.

The default switch-last binding (`Ctrl+Shift+Backspace`) is also
rebound to a safe key (`Ctrl+Shift+F9`) on the keystream path: there
the Backspace also reaches the focused app, where `Ctrl+Backspace`
means "delete the previous word" and would corrupt the very text being
corrected. An explicit custom binding is always honoured as-is.

### Fixed — evdev listener no longer floods the log when a keyboard disconnects

Powering off a Bluetooth keyboard (or unplugging a USB one) left its
evdev fd returning `ENODEV` on every poll, and the listener re-polled
it hundreds of times a second — warning on each, flooding the log
forever. A disconnected device is now dropped from the poll set on the
first `ENODEV`. The listener also re-enumerates `/dev/input` every two
seconds, so a reconnected keyboard is picked back up automatically
instead of staying dead until the app restarts.

## [0.1.0-beta.15] — Linux/Wayland auto-switch on Hyprland + keyd

### Fixed — Linux/Wayland auto-switch on Hyprland + input-remapper setups

The auto-switch + corrector pipeline did not actually work on a
Wayland session running Hyprland with `keyd` (a common tiling-WM
setup): the tray icon appeared but no layout was detected, nothing
was corrected, and early attempts spiralled into a backspace/space
loop that locked typing for seconds. Several distinct bugs:

* **evdev listener deadlocked.** `Device::fetch_events` is blocking
  by default; the single-thread fan-in loop stalled on the first
  quiet device and never reached the keyboard `keyd` actually emits
  through. The evdev FDs are now set non-blocking.
* **Layout switch hit the wrong device.** `hyprctl switchxkblayout
  main-keyboard` only flips one keyboard; with `keyd` the real input
  flows through its virtual keyboard, which kept the old layout and
  re-typed the original Latin glyphs. We now switch `all` devices.
* **Active-layout query read the wrong device.** `current()` took the
  first `active keymap` line (a stale power/sleep button), so the
  engine misjudged the active layout and the tray ignored manual
  Alt+Shift switches. It now reads the keyboard Hyprland flags
  `main`, skipping our own uinput emitter.
* **Corrector typed Unicode escape codes.** The Wayland emitter drove
  the GTK `Ctrl+Shift+U <hex>` compose sequence, which most
  terminals / Wayland-native apps render literally. The corrector now
  replays the original scancodes after the layout flip (a new
  `KeyEmitter::send_keys`), so the compositor's xkb mapping produces
  the right glyphs. Windows/macOS keep their native Unicode path.
* **Self-correction feedback loop.** Replayed events come back through
  the listener without an `injected` marker (the remapper strips it),
  so the engine re-corrected its own output indefinitely. A short
  post-correction lockout window suppresses the echo.
* **Dropped keystrokes in replays.** Packing press+release into one
  uinput frame let libinput coalesce it into a zero-duration tap
  (most visibly the trailing space between corrected words). Events
  are now emitted one per frame with a small inter-event delay.
* **Shift / Caps state was ignored.** The evdev listener left
  modifiers empty, so corrections always came out lowercase. It now
  tracks Shift/Ctrl/Alt/Super/CapsLock from the event stream.

`scripts/setup-linux.sh` also re-triggers udev with `--action=change`
and force-fixes `/dev/uinput` ownership so the permissions apply
without a reboot.

### Added — "weak" dictionary list for rare-but-valid Hunspell forms

Hunspell expands every Ukrainian stem into all of its grammatical
surface forms — including ones modern speakers basically never type
standalone, like vocative-case nouns ("туче!" — "O cloud!" from
`туча`). When such a form happened to also be the cross-layout
rendering of a common English word, the dict detector saw a real
Ukrainian word in the buffer and refused to switch — leaving the
user stuck on gibberish. The motivating case: typing `next` under
uk-UA produced `туче`, which is technically valid → `Keep` → no
correction.

New per-layout `<stem>-weak.txt` data file marks these "valid but
basically never the intent" entries. The `DictionaryDetector` now
treats a current-side weak hit as a deferred signal: it walks the
alt-layout renderings first and switches to any of them that's
itself a strong dict hit. If no alt is in dict, the weak word still
keeps (the weak list never blocks a switch by itself, only opens
the door to one). Strong (non-weak) entries are unaffected — they
continue to win outright.

* New file: `data/wordlists/uk_ua-weak.txt`, seeded with `туче`.
  Conservative on purpose — adding a common word here would
  auto-switch users typing it intentionally.
* Same loader contract as the existing `<stem>-stop.txt` /
  `<stem>-extras.txt` files: bundled list at compile time, optional
  user overlay at `<config-dir>/poltertype/wordlists/<stem>-weak.txt`
  picked up by "Reload Settings" without a rebuild.
* `DictionaryDetector::is_weak()` exposed for diagnostic UI / future
  detectors.

### Fixed — short English acronyms typed in the wrong layout now switch

Two-letter English acronyms (`AI`, `ML`, `UI`, `UX`, `DB`, `QA`, `CD`,
`CI`, `MD`, …) typed under uk-UA used to render as Cyrillic-uppercase
noise (`ФШ`, `ЬД`, `ГШ`, …) and stay there — neither detector had any
signal to switch on:

* `DictionaryDetector` deliberately skips the embedded FST for ≤2-letter
  buffers (the bulk `dwyl/english-words` corpus ships short noise like
  `ws`, `ax`, `oe` that would block legitimate Cyrillic switches), so a
  curated 2-letter acronym sitting only in the FST was invisible.
* `WordPlausibilityDetector` ignores buffers shorter than 3 letters by
  design.

`build.rs` now mirrors the ≤2-letter slice of `<stem>-extras.txt` into
the dist `<stem>-stop.txt` at compile time. Extras is our own curated
list — no noise — so its short subset is safe to trust in the short
regime. Existing user-side `<stem>-stop.txt` overlays still merge in
on top, and the `dwyl` short noise is unchanged (still FST-only, still
invisible to the short-token lookup). For en-US this lights up `ai`,
`ml`, `ui`, `ux`, `db`, `qa`, `cd`, `ci`, `md`, `fe`, `fp`, `gz`,
`qr`, `mp`, `bz`, `xz`, `ks`, `ln`, `rc`, `ay`.

### Changed — unified Save / Reload in the Settings window

The Wordlists pane used to ship its own Save and Reload buttons
below the editor, separate from the footer Save and Reload that
covered the rest of the settings. Two pairs of nearly-identical
buttons made the UI confusing — users (reasonably) expected the
more prominent footer Save to write everything, including the
wordlist edit in front of them, and were surprised when it
didn't.

Both per-pane buttons are now removed. The footer pair now
covers everything:

* **Footer Save** — writes `config.toml` AND flushes any unsaved
  wordlist content (using the same `flush_wordlist_to_disk`
  helper as the auto-save-on-switch path).
* **Footer Reload** — re-reads `config.toml` AND re-reads the
  currently-displayed wordlist file from disk, discarding any
  unsaved editor content (intentional — same semantics as the
  old per-pane Reload).

The Wordlists pane keeps its dirty indicator ("● unsaved
changes") and per-pane status banner so the user still sees
"auto-saved unsaved edit to ..." messages from layout / profile /
kind switches. Just one click target for the save itself.

### Changed — Settings window default size

Bumped from 720×540 to 820×640 so the Commands and Wordlists
panes render their full forms (and lists, where applicable)
without scrolling on a stock 1080p screen. Still small enough to
feel like a settings dialog, not a main window.

### Added — system notification on auto-switch

When the engine auto-corrects (changes the OS layout and re-types
the last word) it can now show a brief system notification —
`"poltertype: Switched to English (United States)"` — that auto-
dismisses after ~2 seconds. Off by default (preserves the existing
"quiet by default" contract); toggle on the General pane in the
Settings window. The body text uses the layout's friendly `name`
field (from `data/layout-mappings/<stem>.toml`) when known, and
falls back to the raw BCP-47 id.

Implementation notes:

* Cross-platform via `notify-rust` — Windows 10+ Toast,
  NSUserNotification on macOS, Desktop Notifications spec via
  DBus on Linux. Matches platforms supported elsewhere in
  PolterType.
* Fired only on `SwitcherEvent::Corrected` — auto-switch and
  manual switch-last hotkey both produce that event, so the
  user sees notifications for both. NOT fired on
  `LayoutChanged` (which also covers external layout changes
  like Win+Space; those are already explicit user actions and
  don't need a notification of their own).
* Spawned on a dedicated thread so the platform's notification
  call (DBus round-trip on Linux, Toast XML on Windows) never
  adds latency to the tray's event loop.
* Notification text never contains the typed word — only the
  destination layout's name. Matches the project's hard rule
  in `CLAUDE.md` about not logging user-typed text.
* Failures (no notification daemon, Focus Assist suppressing
  toasts, sandbox quirks) are logged at warn level and
  swallowed; the auto-switch itself already happened, so the
  notification is best-effort UX sugar on top.

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
   `<config-dir>/poltertype/wordlists/<stem>.txt` and atomically
   swaps the engine's dictionary set, same primitive the tray
   "Reload Settings" entry uses.
3. Per-profile cache rebuild + watcher force-reapply — the
   profile cache built at startup is replaced from disk, and a
   new `force_reapply` flag tells the focus-watcher to re-apply
   the currently active profile on its next ~250 ms tick. Without
   this, a user editing a profile's wordlist while focused on a
   matching app would have to alt-tab away and back to see the
   change.

Refactor in `crates/poltertype-app/src/main.rs`: `profile_dict_cache` now
lives behind `Arc<RwLock<...>>` so the close-handler can rebuild
it without restarting the watcher thread; `spawn_profile_watcher`
takes the cache + force-flag and re-reads on every tick. The
Wordlists pane banner / pane-intro text were updated from
"Restart PolterType to apply" to "Close this window to apply" so
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
action  = { type = "open_path", path = "%LOCALAPPDATA%/poltertype/config.toml" }
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
`<config-dir>/poltertype/wordlists/profiles/<id>/<stem>.txt` (and
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

* Cargo workspace with seven crates: `poltertype-app`, `poltertype-core`,
  `poltertype-input`, `poltertype-layout`, `poltertype-detect`, `poltertype-ai`, `poltertype-types`.
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
* AI subsystem scaffold (`poltertype-ai`, gated by `feature = "ai"`):
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
`<config-dir>/poltertype/wordlists/<id>.txt` to extend any
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
  with `lipo`) containing a tray-only `poltertype.app` (`LSUIElement`
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
`crates/poltertype-core/build.rs` writes them to `target/dist/data/`; each
installer copies that tree into the runtime's expected location:

| Platform | Data lives at |
|---|---|
| Windows MSI | `<exe_dir>\data\` |
| macOS .dmg | `poltertype.app/Contents/Resources/data/` |
| Linux AppImage | `<mount>/usr/share/poltertype/data/` |
| dev (`cargo run`) | `target/dist/data/` |

`poltertype_core::data_dir::resolve()` finds the live tree at startup. The
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
  tray launch silently drops. `crates/poltertype-app` now reads bindings
  from `[hotkeys]` in settings (used to be hardcoded).
* **Wordlists** — multiline editor over the per-layout user-overlay
  files in `<config-dir>/poltertype/wordlists/<stem>.txt` (Extras)
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
(`poltertype --settings`) so the tray's `tao::EventLoop` and
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

[0.3.1]: https://github.com/Just-Code-NET/PolterType/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/Just-Code-NET/PolterType/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/Just-Code-NET/PolterType/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/Just-Code-NET/PolterType/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/Just-Code-NET/PolterType/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/Just-Code-NET/PolterType/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/Just-Code-NET/PolterType/compare/v0.1.0-beta.16...v0.1.0
[0.1.0-alpha.0 → 0.1.0-beta.6]: https://github.com/Just-Code-NET/PolterType/releases
