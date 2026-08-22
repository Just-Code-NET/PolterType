# Known gaps (as of v0.17.7)

Things a reader of the docs might reasonably assume work, but don't.
Check here before promising any of them (especially on the website).
**Re-stamp this heading and re-verify every bullet at every release —
`docs/RELEASING.md` step 2 makes that a blocker, not a chore.**

Say which platform a pass actually covered rather than re-stamping
silently: a heading that claims more than was checked is worse than a
stale one, because nobody can tell which bullets it means. It also went
three releases without a stamp (0.14.3 → 0.17.2), which is what the
sentence above exists to prevent.

**What the [Unreleased] pass has checked so far.** Nothing
platform-specific, and for the same reason as the release before it:
what has landed is one engine rule and one list of dictionary entries,
identical code on every OS. What was run, on this Arch/Hyprland
machine — re-stamp this heading with the version when the release is
cut, and add whatever else ships with it:

- **The whole workspace suite**, including two engine tests that fail
  on 0.17.7's code — one types `cd /tmp ` through the full pipeline and
  asserts the layout never moves, the other types `/tmp ghbdsn ` and
  asserts the *second* word is still corrected, so the veto cannot
  quietly widen to the rest of the line.
- **The new dictionary entries against the real bundled
  dictionaries**, both properties at once
  (`crates/poltertype-core/tests/shell_vocabulary.rs`): every entry
  survives the shipping detector pair, and none of them renders into a
  real word of another bundled layout. Entries that failed the second
  test — `src`, `cfg`, `ptr` — were left out rather than shipped.
- **The reporter's assumption, from their own machine's log**: the app
  loads only the layouts the OS reports as active (`layouts=2` here),
  which is what makes `/` a separator rather than a letter. The engine
  tests were changed to load the same two layouts for that reason —
  before, they loaded all fifteen, and bg-BG's `б` on the `/` key made
  the harness disagree with every real machine.

Not re-run: everything below that is not dated today — every Windows
and macOS bullet, the tooltip backends, the updater, and the NixOS and
systemd bullets stamped at 0.17.6. **The fixed build was not driven
live through uinput**; the tests above are what this stamp rests on.

**What the 0.17.7 pass actually checked.** Nothing platform-specific,
because nothing platform-specific moved: the release is three changes to
the detector, the undo-learning rule and the shape one line takes in a
user wordlist file, and all three are the same code on every OS. What was
run, on this Arch/Hyprland machine:

- **The whole workspace suite**, including two engine tests and a corpus
  test that fail on 0.17.6's code
  (`crates/poltertype-core/tests/overlay_poisoning.rs`, which is the
  evidence rather than a description of it). The corpus one plants the
  stray entry in a user overlay and puts the question to the **real**
  bundled dictionaries and layout tables, both directions.
- **The reported failure, reproduced from the reporter's own machine
  state** — their wordlist overlays and the app's log, which date each
  bad entry to an undo of a correction the engine had got right.

Not re-run: everything below that is not dated today, which this pass
does not touch — every Windows and macOS bullet, the tooltip backends,
the updater, and the NixOS and systemd bullets stamped at 0.17.6. **The
fixed build was not driven live through uinput**; the tests above are
what this stamp rests on.

**What the 0.17.6 pass actually checked.** Linux only, on two Wayland
machines — this Arch/Hyprland one and a NixOS 26.05 laptop (Hyprland,
AppImage) — and every line below is something that was run, not read:

- **NixOS, end to end on real hardware.** The setup script's
  declarative path (module written, `imports` patched, backup kept,
  staged for the flake), the owner's own `nixos-rebuild switch`, and
  afterwards `id` showing `input` and `uinput`, `/dev/uinput` at
  `0660 root:uinput`, binfmt registering `appimage_type_1/2`, and the
  AppImage running by path.
- **The entries the app writes point at the AppImage, not at its
  mount.** Both were rewritten to
  `~/Applications/poltertype-0.17.6-x86_64.AppImage` on that machine;
  before this release the autostart one pointed inside a temporary
  extraction directory.
- **Startup with `HYPRLAND_INSTANCE_SIGNATURE` unset** — the exact
  condition that killed the app at login — resolves the compositor from
  its socket directory instead: `layout switcher ready
  backend="linux-hyprland-hyprctl"`, `focus tracker ready
  backend="linux-hyprland-ipc"`.
- **Startup with neither that variable nor `DISPLAY`** now reaches
  `entering event loop` and stays there. The same command on 0.17.5's
  code left a coredump: `XDefaultRootWindow` on a null display, in
  `global-hotkey`'s X11 thread.
- **The systemd user service**, on a session whose
  `graphical-session.target` is active: written, `is-enabled` →
  `enabled`, the old `.desktop` entry removed, and
  `systemctl --user start` → `active (running)` with the app up.
- **The manual switch-last hotkey after a pause** — an engine test
  driving the real sequence, which fails on 0.17.5's code. Read the
  0.17.5 stamp below against it: that pass *did* verify this hotkey,
  by injecting the whole sequence through uinput. Injection is faster
  than a person, so it landed inside the two-second window the bug
  needed, and confirmed a feature that did not work for anybody. A
  green check can move too fast to be true.

Not re-run: everything on Windows and macOS, the tooltip backends, the
updater, and every bullet below that is not dated today.

**What the 0.17.5 pass actually checked.** Two things, both on this
machine: the Hotkeys pane renders the substituted chord (screenshot,
Hyprland/Wayland), and `Ctrl+Shift+F9` really does fire a force-switch
on the evdev backend — injected through uinput into a scratch window,
confirmed by `correction applied … reason=manual switch-last hotkey` in
the app's own log. The macOS half of the same fix is unrun, like every
macOS bullet here. Nothing else was re-verified.

**What the 0.17.4 pass actually checked.** Narrower still: the evdev
scan and the setup script's new self-check, both against this machine's
real `/dev/input` (29 nodes, 6 keyboards) — plus the half of the KDE
bullet below that a reporter's Plasma 6 log now settles. Nothing else
was re-run.

**What the 0.17.3 pass actually checked.** A narrow pass on top of
0.17.2's: the two bullets below are new and rest on this release's own
evidence; nothing else was re-run, so every other bullet still carries
the date of the pass that last verified it.

- the **compound guard** — both directions through the real bundled
  dictionaries and layout tables, on this machine
  (`crates/poltertype-core/tests/compound_corpus.rs`, which is the
  evidence, not a description of it);
- the **KDE layout backend** — against KWin's `keyboard_layout.h` and
  Qt's `qdbusutil.cpp` on 2026-08-20, and **not** against a running
  Plasma session; see the bullet below.

**What the 0.17.2 pass checked**, all of it re-run rather than
recalled:

- the **manifest-signature** bullet — six releases' `latest.json`
  fetched through the live redirector, all signed, and the constant
  flipped as a result;
- the **Linux** desktop-integration claims — on this machine, against
  a real Wayland session (`hyprctl clients` before and after);
- the **macOS artefact** claims — read out of the published v0.17.1
  DMG (`AppIcon.icns`, `Info.plist`, `_CodeSignature/`), not out of
  Apple's documentation.

Everything else — the layout tables, the key gate, focus tracking, the
tooltip backends, the macOS runtime paths — stands on its own earlier
dated evidence and was **not** re-run here. The 0.14.3 pass is still
what backs the Windows bullets.

- **The KDE layout backend still has not *switched* a layout on a KDE
  machine.** 0.17.3 rewrote it — Plasma has addressed layouts by index
  since 5.23 (2021) and we were passing xkb names, and the layout *list*
  was being read out of a `qdbus` error sentence that arrives on stdout
  with a success exit code, which is how a user's Plasma 6 session came
  up with zero usable layouts (issue #31). Reading is now **observed**:
  that reporter's v0.17.3 log resolves `en-US` and `ru-RU` off a live
  Plasma 6 and loads both. The write half — `setLayout` by index, and
  the pre-5.23 name fallback — is still only reasoned, from KWin's own
  `src/keyboard_layout.h` and Qt's `argumentToString`, and unit-tested
  against the exact strings both produce; **nobody on this side has a
  Plasma session**. The backend refuses to activate unless Plasma
  answers with a list it can parse, so if the write half is still wrong
  the failure is "the layout does not change", not "PolterType switches
  to the wrong one".

- **Nine of the fifteen bundled layouts have never been typed on.**
  0.9.0 added pl, cs, el, he, tr, bg, it, pt-PT and pt-BR. The
  mappings were generated from `xkeyboard-config` rather than
  transcribed by hand — which is why they are trustworthy at all, and
  the generator reproduced the existing `de_de.toml` byte-for-byte as
  a control — but generated-and-reviewed is not the same as *used*,
  and nobody here owns a Czech or a Bulgarian keyboard. What that
  does and doesn't cover:
  * The **characters** are as authoritative as the OS's own keymap
    data on Linux, and **someone has now checked Windows**
    (2026-08-04, by loading each keymap with `LoadKeyboardLayoutW` and
    reading it back through `MapVirtualKeyExW` + `ToUnicodeEx`). All
    fifteen match at the plain and shift levels, the nine unproven
    ones included; four keys differ in the whole set — `uk-UA` `0x2B`
    (`ґ`/`Ґ` on xkb, `\`/`/` on Windows), two in `el-GR`, one in
    `fr-FR`. Those four are no longer anybody's problem on Windows:
    since 0.14.3 the OS's own answer replaces the bundled table there
    (`ґ` turns out to live on `0x56`, and Windows users get it back),
    while Linux keeps reading the TOML. The data is deliberately left
    alone — one TOML serves three operating systems, and editing it
    for Windows would break Linux. macOS is still unchecked and still
    reads the TOML.
    Watch the method as much as the result — `ToUnicodeEx` answers
    `-1` for a dead key and still writes the character, so counting
    that as "produces nothing" reports every accent key on half of
    Europe as broken. It did, until it didn't.
  * **AltGr / third-level characters are not tracked at all**, by
    design and for every layout including the six older ones. This
    is what makes pl-PL a no-op against en-US.
  * The **macOS TIS identifiers** for the new languages
    (`com.apple.keylayout.Czech`, …) are from Apple's naming
    convention, not from a Mac. A wrong one means "switch to this
    language" silently targets a keyboard the user doesn't have; the
    reverse table is deliberately narrower than the forward one for
    exactly this reason. Same standing caveat as every other macOS
    path here.
  A report that a correction produces the wrong letter under one of
  these is a data bug in one TOML, not an engine bug — fix the row.

- **On Windows a language is not a keyboard — handled since 0.14.3,
  but not all the way down.** `hkl_to_layout_id` still keeps only the
  LCID, so all three of Windows' genuinely different Bulgarian
  keyboards are `bg-BG`. What changed is that we no longer *guess*
  which one: `LayoutSwitcher::describe_keymaps()` asks the OS what
  each installed keyboard actually produces, and `LayoutDb` lays that
  over the bundled table (`layouts/os_keymap.rs`). Precedence is
  bundled ← plug-ins ← **OS** ← user TOML.
  The numbers that made this worth doing, measured through the
  shipping path: `bg_bg.toml` reproduces Phonetic Traditional exactly,
  differs from Typewriter on 7 keys and from **Phonetic on 45 of 48**.
  That last user got corrections built from a keyboard they do not
  own, silently. Turkish (Q vs F) and Ukrainian (standard vs Enhanced)
  had the same exposure and are covered by the same change — as is any
  custom layout, which is the argument for querying the OS over
  declaring variants in TOML (see `docs/DECISIONS.md`, 2026-08-08).
  **The residual is real and narrow.** Two keyboards for one language
  still collapse to one `LayoutId` and only one table can be held; we
  keep the one currently in effect, else the first the OS lists. A
  user who keeps two keyboards for a language *and* types on the
  second gets the first one's table — a dropped `<config-dir>/
  poltertype/layouts/*.toml` is the answer, because it outranks the
  OS. Fixing it properly means putting the variant into `LayoutId`,
  which reaches config, UI, dictionary stems and `switch_to`.
  So a "corrections are garbage on Windows" report is no longer
  answered by asking which variant they use — ask for the log instead:
  `adopted the OS keymap` says which keyboard was read, and at debug
  level the next line names every key that disagreed.

- **The installers roughly tripled in 0.9.0**, from 17–28 MB to
  55–65 MB (MSI 54, DMG 63, AppImages 65), and the bundled data tree
  from 21 MB to 78 MB. Nine
  dictionaries did that, over half of it Turkish alone (agglutination
  → 5.8 M surface forms → two 15 MB FSTs). Nothing is *loaded* that
  the user's OS doesn't have enabled, so RAM is unaffected — but the
  download is now the first thing a new user experiences, and the
  updater pushes it at every existing install. Before bundling
  another large language, ask whether the answer is a language pack
  rather than more `data/`.

- **Holding keystrokes back during a correction: Linux/evdev by
  default, Windows opt-in **and now measured**, macOS not at all.**
  The Windows gate ran on real hardware for the first time on
  2026-08-04 and #7 closed with the answer "works, still not the
  default". Two bugs were fixed getting there, both of which lost
  typing outright rather than scrambling it: the flush called
  `send_keys`, which Windows and macOS answer `Unsupported`, and had
  no fallback — so held keys were swallowed and then dropped; and once
  it had one, translating held keys through a layout overlay dropped
  everything structural, the **spacebar** most of all, which is the
  boundary that triggers most corrections. Both fixes are shared with
  macOS, which had the same two holes and has still never had the gate
  switched on.
  It stays **off unless `POLTERTYPE_HOLD_KEYS=1`**, and the reason is
  now latency rather than fear: the absorb-and-sweep loop withholds
  keys for ≥75–100 ms after every correction, which reads as the caret
  lagging behind typing. That trades a rare mangled word for a
  constant delay, and it is the wrong default. No keyboard wedge was
  ever observed; `HoldState`'s self-healing deadline was never reached.
  Do not flip the default without the synchronous stream check
  described in #7.

  The rest of this bullet is about evdev. The key gate
  (`poltertype-input::KeyGate`, 0.6.0) is what stops a keystroke
  landing inside a correction. macOS has no implementation, and X11 is
  deliberately out (XTest serialises with real input). On Linux it also disables itself wherever it cannot
  work: keyd and friends hold our own uinput device, so grabbing the
  user's keyboard would block the correction too. Those users get the
  detect-and-repair path, which is *better than 0.5.0 but not
  airtight* — typing a whole command without pausing can still come
  out scrambled. Don't describe corrections as race-proof anywhere;
  say what `docs/PERMISSIONS.md` says. Since 0.6.3 the remapper check
  runs before **every** hold, not just at startup — the startup probe
  races keyd's async grab of our fresh emitter, and losing silently
  used to end with the gate funnelling the whole session's input into
  the app (see `docs/DECISIONS.md`, 2026-07-31). An `EBUSY` at hold
  time turns the gate off until restart.
- **A shortcut pressed mid-correction while keys are held is lost.**
  Backspace, arrows and Esc are re-emitted behind the correction, but
  a chord needs modifiers the emitter cannot reproduce, so the gate
  lets go and that one keypress never reaches the application. Narrow
  (the window is the length of one burst) but real.

- **Focus tracking is complete on Windows, Hyprland and X11, partial
  on other Wayland, and absent on macOS.** `focused_exe()` still
  returns `None` on macOS. On non-Hyprland Wayland it is no longer
  `None`: since 0.10.0 `atspi_focus` watches `window:activate` on the
  a11y bus and asks the bus which process owns the sending connection
  (`GetConnectionUnixProcessID` → `/proc/<pid>/exe`), which is the
  same executable basename the other backends report. Verified live
  on 2026-08-01 — reported `exe=chrome` 272 ms after an activation.
  **Read the partial carefully before relying on it.** Only apps with
  a live accessibility bridge are ever seen. GTK, Qt and Electron-with-
  a11y answer; most terminals do not — and a terminal is exactly where
  a developer types. An app that never emits also never *un*-focuses
  the previous one, so the answer can be stale in a way a compositor
  query never is; samples therefore carry an age and anything older
  than five minutes is treated as no answer. Do not write "focus
  tracking works on GNOME/KDE" anywhere: it is an improvement on
  nothing, not an equivalent of a real window query.
  *This gap has teeth.* The skip-list was harmless while it was dead
  everywhere; when the Hyprland/X11 tracker landed in 0.3.0 it made
  the shipped default list suddenly **live**, and the app went quiet
  in every editor and terminal a developer uses. 0.4.2 reversed it —
  `disabled_apps` is now **empty by default** and opt-in. Before you
  make anything else focus-keyed, ask what it does the day a tracker
  appears on a platform that didn't have one.
- **The suggestion tooltip renders on Linux only — but on more of
  Linux than this file used to claim.** `poltertype-popup` *probes*:
  layer-shell, then X11, then noop. Layer-shell covers wlroots
  compositors **and KWin** (verified against KWin 6.7.3 on
  2026-07-31 — the "no-op on KDE" line that stood here was simply
  wrong). Mutter has no layer-shell, but GNOME Wayland sessions run
  XWayland, so the X11 override-redirect probe still maps a window
  there. The real remaining gap is a Wayland session with neither
  layer-shell nor XWayland — plus **macOS and Windows**, which are
  noop and where the feature degrades to the keyboard accept chord
  alone, with no visible list (near-useless until a backend lands; the
  engine side works everywhere). Never re-state this as a list of
  desktop names: the code asks the compositor, and a name-based claim
  goes stale silently. Positioning is an anchor chain, best first: AT-SPI
  caret extents (apps with a live a11y bridge; PolterType raises
  `org.a11y.Status.IsEnabled` so bridges activate — apps already
  running before the first launch stay silent until restarted) →
  window bottom-centre → screen edge. **Without a caret the tooltip
  is only ever window-accurate, not caret-accurate** — it hangs above
  the focused window's bottom edge, which is right for a chat input
  and coarse for a caret mid-document. The pointer used to sit in
  that gap and is gone: an idle mouse dragged the tooltip to
  wherever it was parked. Since 0.17.7 a caret is used **only when it
  can be proved to belong to the focused window** — same process, same
  window size — because one desktop-wide slot holds whichever app
  moved a caret last, and a background chat window's caret composed
  with a maximised editor's rect placed the tooltip a thousand pixels
  off. The honest reading of that rung is therefore "caret-accurate in
  apps with a live bridge, window-accurate everywhere else", never
  "caret-accurate on Linux". See `docs/DECISIONS.md`, 2026-07-24,
  2026-07-29 and 2026-08-22.
  **Windows has a backend since 0.11.0** — a layered, topmost,
  `WS_EX_NOACTIVATE` window fed by `UpdateLayeredWindow`, sharing the
  renderer and the placement arithmetic the Linux backends already
  used. Its tests create a real window and hand real surfaces to
  Win32, so the plumbing is exercised rather than mocked; what nobody
  has signed off is whether the result *looks* right, which only a
  person can say. It is **window-accurate only**: `caret_hint()` has no
  Windows implementation, so the anchor chain starts one rung down.
  `GetGUIThreadInfo`'s `rcCaret` is the obvious source and is not
  wired up. macOS remains noop.
- **The AI subsystem is an interface with no backend, on purpose.**
  Since 0.10.0 the stubs are gone: one `LlmDetector` speaks three HTTP
  shapes and calls whatever endpoint the **user** configured — their
  Ollama, their API key, their gateway. We ship no model, no vendor
  SDK and no default endpoint, so **out of the box nothing answers**
  and enabling the feature changes no decision until the user points
  it somewhere. Never write "AI-powered".
  The gates are layered and each is real: the `ai` cargo feature, the
  `remote` sub-feature (without which no HTTP client is compiled in —
  `cargo tree` confirms it), `[ai].enabled`, and `[ai].allow_remote`
  for a **non-loopback** endpoint only. That last distinction is
  deliberate: `allow_remote` gates typed words *leaving the machine*,
  and 127.0.0.1 does not. `locality.rs` decides it, resolves no DNS,
  and fails closed. See `docs/AI.md`.
- **Portal key-sending exists but has never run; the AT-SPI
  *listener* was measured and refused.** 0.10.0 added a
  `RemoteDesktop` portal emitter (`linux/portal/`) so a Wayland user
  who never ran `setup-linux.sh` can still be typed for. It is tried
  **only when `uinput` cannot be opened**, so nobody with the group
  membership meets a consent dialog. Written from the specification
  against a machine with no RemoteDesktop backend — treat it exactly
  like the macOS paths: compiled, unit-tested where the logic is
  host-portable, executed by nobody. Its keycodes are evdev numbers,
  not X11's; that is the mistake to look for first. `libei` was
  skipped deliberately — the portal's `NotifyKeyboardKeycode` does the
  same job without a new protocol implementation.
  X11 uses XInput2 + XTest. AT-SPI keystroke listening is not a todo
  — `RegisterKeystrokeListener` returns false on wlroots and delivers
  nothing even with injected keys, because at-spi2-registryd relays
  only what the compositor hands it and only mutter does (see
  `DECISIONS.md`, 2026-08-01). AT-SPI *is* used for the caret and the
  focused application, which is a different interface entirely.
- **The onboarding walkthrough exists since 0.7.0, and has never run
  on macOS.** The tray alert opens the Settings window on its **Setup**
  pane (`poltertype --setup`), which probes the live machine via
  `poltertype_input::setup::probe_setup` and renders the result — plus
  a banner when no layout switcher exists. Verified on Wayland/evdev
  here; the macOS half (`AXIsProcessTrustedWithOptions`,
  `IOHIDCheckAccess`, the `x-apple.systempreferences:` deep links) is
  compiled by CI and executed by nobody. Two things it deliberately
  does NOT do: run anything with `sudo` (the Linux button copies the
  command instead), and imitate a system permission dialog (macOS shows
  its own). What is still missing from issue #10's wish list is the
  screenshots/GIFs of the macOS toggles.
- **Self-update is proven on Linux, assumed on Windows, unwritten-off
  on macOS.** The AppImage path is exercised. The **MSI path has been
  read but never watched**: verifying it needs two published releases,
  so it cannot be done before the release that would provide the
  second one. 0.11.0's installer was built, installed, run and
  uninstalled by hand — the *update* step is the one thing in that
  sequence nobody has seen work on Windows, and the first person to
  find out will be a 0.10.0 user taking 0.11.0. Do not describe it as
  tested. The `.app`-bundle swap in
  `poltertype-update/src/apply/macos.rs` is written from Apple's docs.
  It also strips `com.apple.quarantine` from the installed bundle —
  defensible only while the app is unsigned, and it must come out the
  day we ship notarised builds.
- **Manifest signatures are mandatory from v0.17.2 — which turns a
  forgotten signing step into an outage.** This is no longer a gap in
  the product; it is a gap in *us*, so it stays on this list. Since
  0.7.0 `latest.json` has carried a detached ed25519 signature, checked
  against a key compiled into the binary before any URL in the manifest
  is read; the private half is on the maintainer's machine and never
  enters CI, so a compromised GitHub account cannot forge it.
  `REQUIRE_SIGNATURE` was `false` until v0.17.2 — a wrong signature
  refused, a missing one warned about — because flipping early would
  have stranded users whose updater still resolved to an unsigned
  manifest.
  **Checked live through the real redirector on 2026-08-16: every
  release from v0.7.0 to v0.17.1 is signed.** The rollout condition (a
  signed `latest.json` for a full cycle, so 0.9.0 at the earliest) had
  therefore held eight times over, and the constant is now `true`.
  What that costs: signing stays a **manual** step
  (`cargo xtask manifest sign`, `docs/RELEASING.md` §7) because the key
  must not be a CI secret, so publishing a release without signing it
  is now every v0.17.2+ updater reporting "cannot update" until
  somebody signs and re-uploads the manifest. Nothing automated checks
  this. Builds older than v0.17.2 carry their own `false` and are
  unaffected.
  **The updater may now be described as verifying signed manifests —
  the installers still may not be called signed.** Those need
  certificates we do not hold; see `docs/CODE_SIGNING.md` and say what
  the README says.
- **An install that isn't ours can't self-update.** A distro package,
  a `cargo run` dev build, or a bare binary has no AppImage to swap
  (`$APPIMAGE` is unset) and no bundle to replace. Those users get a
  notification pointing at the Releases page instead — by design; we
  do not overwrite files a package manager owns.
- **We ship x86_64 and aarch64 Linux, x86_64 Windows, universal
  macOS — and nothing else.** `release.yml` publishes exactly four
  installers and `platform_key()` names exactly those four; anything
  else (ARM Windows, armv7, 32-bit) resolves to a key that isn't in
  the manifest and gets "no update for you" rather than a wrong
  download. aarch64 Linux is built natively on `ubuntu-24.04-arm`,
  added in 0.7.0. Keep the workflow, `platform_key()` and its test in
  step: a key one of them knows and the others don't is a silent
  no-updates bug for that platform.
- **Autostart works on all three platforms since 0.6.2** (registered
  per-user: LaunchAgent / HKCU run key / XDG entry; unticking removes
  it). The "never worked on any platform" bullet that used to sit here
  was deleted at 0.6.3 per its own instruction; the shape of that bug
  — a defaulted-on setting nothing reads — is preserved in
  `docs/DECISIONS.md` and the 0.6.2 changelog entry.
- **macOS: the input changes from 0.7.0 still have not run on a Mac,
  and 0.8.0 ships them again unchanged.** 0.6.2 was
  the first release runtime-tuned on real hardware (macOS 15, Intel, by
  an outside contributor) — but 0.7.0 rewrote the part that was
  validated: the tap now subscribes to `FlagsChanged`, so modifier
  edges reach the word buffer for the first time, and the emitter
  clears event flags and posts modifier releases. Reviewed, unit-tested
  where the logic is host-portable, compiled by CI, **executed by
  nobody**. If a macOS report arrives about words vanishing or
  corrections misfiring, start here. Still unproven besides: Apple
  Silicon (#3), the updater's `.app`-bundle swap, and the keystroke
  hold-back — the key gate is Linux/evdev only, so on macOS as on
  Windows a keystroke can still land inside a correction.
  `focused_exe()` remains `None` there.
- **Autostart on a bare compositor still needs one manual step.** The
  unit PolterType installs is wanted by `graphical-session.target`,
  which GNOME, KDE, Xfce and `uwsm` sessions reach on their own —
  verified on one such session (Hyprland via a display manager,
  2026-08-21). A bare Hyprland/Sway/river session started from a TTY
  reaches neither that target nor `~/.config/autostart`, so the unit
  sits there enabled and never runs. PolterType logs a warning when it
  installs into such a session and `docs/PERMISSIONS.md` carries the
  five-line wiring, but **the warning path itself is untested** — the
  machines here both reach the target.
- **On the X11 backend the OS hotkey grab still trusts a library that
  does not check its display.** `global-hotkey` opens the X display on
  its own thread and uses the handle unconditionally; PolterType now
  waits up to 15 s for a display before building the manager, which
  covers being started early, and skips it entirely on the evdev
  backend, which never used it. What it cannot cover is an X server
  that goes away *later* in the session — that is upstream's thread and
  upstream's null check.
- **The Settings window can crash on a sudden large resize, in debug
  builds only.** Reproduced 2026-08-19 on Linux/Hyprland: a big enough
  jump (not specifically narrow↔wide — a same-direction shrink to
  300×300 did it too) trips an `iced_tiny_skia` 0.13
  `debug_assert!("Quad with non-normal height!")` — some quad's height
  lands on exactly 0.0 for one frame — and the process dies
  (`iced_tiny_skia-0.13.0/src/engine.rs:43`). Not app code: no
  `unwrap`/`panic!` anywhere in `settings_ui`, and it reproduces
  identically on every pane tried (Languages, Plugins), so it's the
  shared window chrome or a scrollbar, not one pane's content. The
  assert compiles out of `--release`, so an installed build never hits
  this exact panic — but the *dev* autostart people actually run day to
  day is a debug build, so it is a live nuisance there.
  `min_size` on the window (`settings_ui/mod.rs`) is a hint, added on
  the chance it helps a real click-and-drag resize; a compositor-driven
  resize (`hyprctl dispatch`, the only way this was tested) ignores it
  outright, so treat it as unverified, not fixed. Root cause is still
  open — bisecting which widget (a scrollbar is the leading suspect) is
  the next step if this gets prioritised.

Deliberately out of scope (not gaps):

- **Code signing** — installers ship UNSIGNED. Apple Developer ID +
  Windows EV/OV cert tracked for a later phase. (Per-platform
  installers themselves *do* exist — see `installers/` and
  `.github/workflows/release.yml`.)
- **Store submissions.** Users install from the GitHub Release page.
  Nothing is published to any package manager yet; AUR, winget and
  Homebrew manifests are *staged* in `packaging/`.
  **`packaging/README.md` is the single source on this** — which
  targets exist, which are blocked and why, and the publish step for
  each. Don't re-derive the answer here or enumerate targets in this
  file: that list went stale once already. Two things that file will
  tell you and that get asked repeatedly: Flatpak is a decided **no**
  (uinput is not grantable, and layout switching needs host binaries),
  and Snap is blocked on a classic-confinement review rather than on
  code. Until a package *is* live, the README install table and the
  site must not mention it. `packaging/bump.sh <version>` re-points
  every staged target at a published release.
- **Global hotkey capture on Wayland** — works inside the focused
  Settings window; Wayland's security model means we don't see global
  key presses while another app has focus.
- **Downloading plug-in packs.** Installing one is supported since
  0.10.0 (`poltertype_core::plugins::install`) — but from a directory
  already on disk, never from a URL. That is the security boundary,
  not a missing feature: fetching third-party content into a process
  that reads every keystroke is a much wider channel than the
  updater's signed, no-payload manifest fetch, and a user who
  downloaded a pack themselves made the trust decision where they
  could see it. No archive also means no zip-slip and no
  decompression bomb. A marketplace **UI** is still queued.
- **Placeholders in smart-command actions** — nothing substitutes the
  typed text into an action. For `run_shell` that would be an
  argument-injection channel; elsewhere nobody has asked for it.
  (`run_shell` itself and multi-token triggers landed in 0.10.0 —
  `run_shell` is off by default behind `[commands].allow_run_shell`,
  runs no shell, and its threat model is in `commands/shell.rs`.)
- **WASM plug-in marketplace.**
- **Telemetry of any kind.**
