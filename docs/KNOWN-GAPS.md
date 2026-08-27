# Known gaps (as of v0.23.0)

Things a reader of the docs might reasonably assume work, but don't.
Check here before promising any of them (especially on the website).
**Re-stamp this heading and re-verify every bullet at every release —
`docs/RELEASING.md` step 2 makes that a blocker, not a chore.**

Say which platform a pass actually covered rather than re-stamping
silently: a heading that claims more than was checked is worse than a
stale one, because nobody can tell which bullets it means. It also went
three releases without a stamp (0.14.3 → 0.17.2), which is what the
sentence above exists to prevent.

## What each release pass actually checked

**What the 0.23.0 pass actually checked (2026-08-27).** One machine —
this Hyprland laptop — and the Settings window, because the release
moves it to iced 0.14 and that touches every pane. The window was
opened and photographed after each step of the migration, which is how
three separate wrong spellings of the brand-mark canvas were caught: it
drew at the window's origin, then masked to a sliver, then not at all,
before the idiomatic form rendered it whole. The resize crash below was
re-tested by driving the window through 300×300, 1400×1000, 320×340 and
900×700 in sequence.

Two gaps came out of this file rather than being re-stamped: the
"winit window" ghost toplevel (#35) and that resize crash. The ghost
**was** measured where it lived — a `WAYLAND_DEBUG=1` trace of the
Settings process on KDE Plasma Wayland in the matrix guest, run against
the *published* AppImages with 0.22.0 as a control: 0.22.0 creates two
toplevels and titles one of them `winit window`, 0.23.0 creates one and
titles it Settings. Neither destroys anything, which is why the old one
lingered. The control matters — without it the trace showing one
toplevel proves only that the trace ran.

The missed-word tray list (#38) was checked on a live run: three
tooltips offered and lost, the list logging 1, 2, 3 rows, and none of
the three words appearing anywhere in the log.


**What the 0.22.0 pass actually checked (2026-08-27).** The manual
force-switch, which the release exists for, on two machines.

On this Hyprland + keyd laptop: a wrong-layout word typed through a
virtual keyboard, auto-corrected, then three presses of the hotkey
running, each read back out of the saved file rather than out of our
own API — undone, moved on, undone again, with the dictionary taught by
the first press only. Then the same word typed with auto-switching
paused: left alone, and still switched by the hotkey.

Then the desktop matrix, fifteen sessions, with three checks the sweep
had never made before — the hotkey pressed **twice**, the pause chord
pressed **after** a correction had already run, and the manual switch
made **while paused**. All three pass on every session that corrects at
all: KDE Plasma Wayland, GNOME Wayland, sway, and Cinnamon, Xfce, LXQt,
i3, icewm, openbox and fluxbox on X11 — ten sessions, covering both
hotkey paths (an OS-level grab on X11, the key stream on Wayland).

Four sessions measure nothing because they have no layout-switching
backend at all — Budgie, labwc, Xfce/Wayland and MATE, each logging
`no layout switcher backend` on every word. That is unchanged from the
0.21.0 pass and is the gap already recorded below, not a regression.
MATE is worth naming twice: with the session itself sitting on the
second layout, the word the harness types renders as the *corrected*
form without anybody correcting it, and the sweep reported a green row
for a desktop where layout switching was off entirely. The report now
reads the log rather than the text for that distinction.


**What the 0.21.0 pass actually checked (2026-08-27).** Linux only,
and three things: the modifier-only chord this release adds, whether
the Settings window renders its own text, and — because the sweep that
measured the first one said so — which layout backend a session picks.

- **The modifier-only force-switch, across the desktop matrix.**
  `Shift+Shift` bound as the force-switch, a wrong-layout word typed
  through a virtual keyboard, auto-corrected, and then put back by two
  taps of Shift — read out of a terminal, never out of our own API
  (#26's lesson). Measured working on every session that corrects at
  all: KDE Plasma Wayland, GNOME Wayland, sway, Cinnamon X11 (both
  sessions), Xfce X11, i3, fluxbox, icewm, LXQt, openbox.
- **The Hotkeys pane's capture of that gesture**, driven by injected
  keystrokes in the guest's Xfce/X11 session: one tap of Shift leaves
  the row asking for its twin, the second binds `Shift+Shift`, and
  Ctrl+Shift held together binds on release. This is where the first
  attempt was broken — the capture subscribed to key presses only, so
  the gesture accumulated and never completed. No test can drive an
  iced subscription; only the VM could catch it.
- **That the window renders its text at all.** It did not, on a stock
  Ubuntu 26.04 desktop: both it and the suggestion tooltip asked for a
  font family by a name most machines do not have, and what the font
  database answered with had no letters in it. Fixed and re-measured
  in the same session.

Two bugs the sweep found that had nothing to do with the release, both
fixed in it and both re-measured after: hotkeys armed 25 seconds late
on a session where GTK's start-up blocks (sway), and the gsettings
backend claiming any session on a machine whose `dconf` a GNOME
install had ever populated.

Not re-run: everything on Windows and macOS, the updater, the tooltip
backends, and every bullet below that is not dated today. Two of those
are worth naming:

- **The modifier-only chord has never run on Windows or macOS.** It
  takes the same key-stream matcher the suggestion digits already use
  there, off the same listeners, and the scancodes it matches come
  from those listeners' own tables — but that is reasoning, not
  measurement, and this file only counts the second one.
- **The Settings window writes no log.** It is a child process that
  starts before `init_tracing` on purpose (it would steal the tray's
  log rotation), so every `info!`/`warn!` inside it goes nowhere. A
  user reporting "the pane did nothing" cannot be answered from a log
  file today.

**What the 0.20.0 pass actually checked (2026-08-26).** Two things, and
nothing else was re-run.

- **The Windows self-update, from the event log rather than a rerun.**
  Windows cannot be booted from here without ending the session, so this
  pass read the machine instead: `Microsoft-Windows-PowerShell/Operational`
  has five of our installer spawns across three releases, each logging
  event 40961 ("console is starting up") and then no 53504 and no 40962 —
  the process died mid-initialisation every time. `Application.evtx`
  carries every MsiInstaller transaction the machine has ever run and
  **not one** from the staging directory. Both facts are what
  `DETACHED_PROCESS` predicts, and the fix is verified only as far as
  code and its tests: **the next person to touch this must click
  "Restart to update" on a real Windows install fed by a published
  release.** Running the installer script by hand does not test it —
  that is exactly the mistake the 0.18.0 pass made.
- **KDE Plasma Wayland**, in the desktop-matrix VM (Ubuntu 26.04,
  `kwin_wayland`), because two bug reports came from it. Measured:
  the layout backend picks `linux-kde-qdbus`, the listener is
  `linux-wayland-evdev` with the key gate on, a wrong-layout word is
  corrected end to end, and — measured for the first time anywhere —
  the manual switch-last hotkey fires and both of its cases work.

**What the 0.19.0 pass actually checked (2026-08-24/25).** Linux
only, in a VirtualBox guest (Ubuntu 26.04) carrying seventeen sessions.
Each was measured the same way: a wrong-layout word injected through a
virtual keyboard, and the bytes that reached the terminal read back —
never what the desktop said about itself.

Corrects a word end to end — Hyprland on this machine, the rest in the
guest:

| Session | Display | Layout backend |
|---|---|---|
| Hyprland | Wayland | `linux-hyprland-hyprctl` |
| KDE Plasma 6 | Wayland | `linux-kde-qdbus` |
| GNOME 49 | Wayland | `linux-gsettings` + the shell's shortcut |
| sway 1.11 | Wayland | `linux-sway-swaymsg` |
| Xfce 4.20 | X11 | `linux-x11-xkb` |
| LXQt | X11 | `linux-x11-xkb` |
| Cinnamon 6.4 | X11 | `linux-cinnamon-xkb` |
| i3 | X11 | `linux-x11-xkb` |
| icewm | X11 | `linux-x11-xkb` |
| openbox | X11 | `linux-x11-xkb` |
| fluxbox | X11 | `linux-x11-xkb` |

Declines with the reason in the log; **the word is left untouched**:

| Session | Why |
|---|---|
| MATE | its group state tracks neither our write nor its own switch |
| labwc, Budgie (Wayland), Xfce (Wayland) | wlroots: no layout API |

Also new since 0.18.1:

- **The X11 input stack ran for the first time** — `linux-x11-xinput2`,
  `linux-x11-xtest`, `linux-x11-ewmh`, `linux-x11-override-redirect`.
  This laptop runs Hyprland and had never executed any of them.
- **Caps Lock**, on this machine: a word typed under a latched lock
  comes back in the right case, a digit stays a digit, and the latch is
  read from the kernel LED.
- **A layout switch is checked, not assumed** — three readings across
  ~80 ms, where a source that can contradict us exists.

**Not** measured:

- the **key gate** never engages under VirtualBox, so held keystrokes
  were tested only on this laptop;
- **niri** and **river** have no backend: neither is packaged for
  Ubuntu 26.04, so neither could be run before shipping;
- **Cinnamon on Wayland** (experimental in 6.4) produced no terminal to
  type into — unmeasured, not passing;
- every **Windows** and **macOS** bullet stands on its earlier date.

**Earlier passes (0.17.2 → 0.18.1)** are in this file's git history:
each stamped what it had actually run — a Windows pass on real
hardware at 0.18.0, NixOS end to end at 0.17.6, the manifest-signature
sweep at 0.17.2 — and every conclusion of theirs that still stands was
folded into the bullets below. One thing they left is a rule rather
than a record: **injection is faster than a person.** The 0.17.5 pass
confirmed a hotkey by driving the whole sequence through uinput,
which landed inside a two-second window a real hand would have missed
and signed off a feature that worked for nobody. A green check can
move too fast to be true.

## Switching the layout on Linux

- **The KDE layout backend is now observed end to end** — but only on
  Plasma **6**, and only on Wayland. 0.17.3 rewrote it (Plasma has
  addressed layouts by index since 5.23, and we were passing xkb names;
  the layout *list* was being read out of a `qdbus` error sentence that
  arrives on stdout with a success exit code, which is how a user's
  Plasma 6 session came up with zero usable layouts — issue #31).
  Reading was confirmed from that reporter's log in 0.17.4. The write
  half — `setLayout` by index — was measured on 2026-08-24 in the
  desktop-matrix guest: the call returns `true`, `getLayout` agrees,
  Plasma's own tray indicator moves, and a wrong-layout word typed into
  a terminal comes back corrected. What is **still** only reasoned is
  the **pre-5.23 name fallback**, which no Plasma 5 session here has
  ever exercised, and **Plasma on X11**, which Ubuntu 26.04 no longer
  packages a session for.

- **A backend that is merely installed used to take the session.**
  Ubuntu autostarts fcitx5 with language support; `fcitx5-remote -t 1`
  exits 0 on a desktop where fcitx owns no input method, and
  `fcitx5-remote -n` then answers with an empty line. Measured across
  the seventeen-session sweep on 2026-08-24: on GNOME, Xfce, MATE,
  LXQt, Budgie, sway, labwc, i3, openbox, fluxbox and icewm the app
  came up on `linux-fcitx5-remote`, reported `count=1` of an empty
  layout id, loaded **zero** layouts — and logged `layout switcher
  ready` while being unable to correct anything at all. Fixed twice
  over: a backend must now name a layout before it is selected, and an
  input method must be the one *this session uses* —
  `XMODIFIERS=@im=fcitx` — rather than merely a running process. IBus
  gets the same guard; its own doc comment had warned about exactly
  this for releases.

- **MATE has no layout backend, and cannot have one yet.** Measured
  2026-08-24: the X11 group lock returns success, `XkbGetState` keeps
  reporting the new group 80 ms later, and the keystrokes still come
  out in the old layout — while the session's own `Alt+Shift` moves the
  keyboard for real and the same `XkbGetState` fails to notice *that*
  either. Its group state tracks neither direction, so there is nothing
  to verify a correction against, and a correction that goes ahead
  regardless deletes a word and retypes it unchanged. PolterType stands
  down and says so. Making MATE work needs a reading that follows what
  the keys actually produce; `mate-settings-daemon` publishes none we
  have found.

- **The wlroots compositors have no layout API to drive.** labwc, and
  therefore Budgie's and Xfce's Wayland sessions, keep their xkb
  configuration in their own config files with no runtime interface.
  Where a GNOME schema happens to be populated the app used to write to
  it and believe the switch — the #26 shape again — and now stands
  down instead. sway is the exception and is supported, because it has
  an IPC that answers honestly. niri and river expose the same kind of
  CLI; neither is packaged for the distro the desktop matrix runs, so
  neither has a backend.

## Languages and keyboard data

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
  What 0.18.0 did fix is the half a user could see: `list_active` was
  handing the same id back once per *keyboard*, so three Bulgarian
  keyboards became three identical `bg-BG` rows in the Languages pane
  — two of them rows the engine had already logged it was ignoring.
  It deduplicates now, first one wins, which is the choice everything
  downstream was making anyway.
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

## Corrections, the key gate and hotkeys

- **Holding keystrokes back during a correction: on by default on
  Linux/evdev, opt-in and measured on Windows (0.11.0) and macOS
  (0.13.0).**
  The Windows gate ran on real hardware for the first time on
  2026-08-04 and #7 closed with the answer "works, still not the
  default". Two bugs were fixed getting there, both of which lost
  typing outright rather than scrambling it: the flush called
  `send_keys`, which Windows and macOS answer `Unsupported`, and had
  no fallback — so held keys were swallowed and then dropped; and once
  it had one, translating held keys through a layout overlay dropped
  everything structural, the **spacebar** most of all, which is the
  boundary that triggers most corrections. Both fixes are shared with
  macOS, which had the same two holes; 0.13.0 then validated the gate
  there on Intel hardware — a four-key burst fired mid-correction
  lands exactly once, in order, in the freshly switched layout.
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
  landing inside a correction; X11 is deliberately out (XTest
  serialises with real input). On Linux it also disables itself wherever it cannot
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
- **The manual switch on a word still being typed is measured on
  Wayland only, and did not work in the one X11 run.** 0.20.0 made the
  force-switch act on the word under the caret. Measured working on KDE
  Plasma Wayland and on GNOME Wayland, where the default chord is
  already substituted to `Ctrl+Shift+F9`. The same probe on Xfce/`xfwm4`
  (X11, default chord `Ctrl+Shift+Backspace`) switched the *finished*
  word correctly and did nothing at all mid-word, logging an empty word
  buffer. **Why the buffer was empty is not established** — the obvious
  suspect is that our own listener sees the Backspace of the chord, but
  a Backspace pops one key rather than clearing the buffer, so that
  does not explain it on its own. Not shipped as a root cause until
  somebody reproduces it; the finished-word case is unaffected
  everywhere, and Windows and macOS carry the same Backspace-based
  default and have not been measured for this at all.

- **A shortcut pressed mid-correction while keys are held is lost.**
  Backspace, arrows and Esc are re-emitted behind the correction, but
  a chord needs modifiers the emitter cannot reproduce, so the gate
  lets go and that one keypress never reaches the application. Narrow
  (the window is the length of one burst) but real.

- **Until 0.18.0, Windows ignored every keystroke that arrived from
  another program.** The engine drops events flagged `injected` — it
  has to, or it corrects its own replay — and macOS flags only the
  events *we* stamped. The Windows listener computed `ours` the same
  way and then OR-ed in `LLKHF_INJECTED`, which Windows sets for
  anything synthetic. So a software KM switch (Deskflow, Synergy,
  Barrier), the on-screen keyboard, voice typing and every remapper
  that re-injects made PolterType go quiet — on Windows and nowhere
  else, with no log line and nothing in this file. Its own listener
  comment said those keys should be treated "exactly like the user's".
  Now they are.
  Two things follow. The safety net for a remapper that strips our
  marker is the engine's expected-echo queue, the same one keyd made
  necessary on Linux — it has never been exercised on Windows, because
  nothing on Windows was stripping anything. And this is *why* no
  Windows runtime test had ever run: injection is the only way to
  drive the app without a person, and the app was built to ignore it.

## Focus, the tooltip and the Settings window

- **Focus tracking is complete on Windows, macOS, Hyprland and X11,
  and partial on other Wayland.** macOS answers since 0.15.0
  (frontmost pid via `NSWorkspace` → `proc_pidpath`). On non-Hyprland
  Wayland it is no longer
  `None` either: since 0.10.0 `atspi_focus` watches `window:activate` on the
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
- **The suggestion tooltip renders on all three platforms; what
  varies is how well it can be aimed.** On Linux `poltertype-popup`
  *probes*: layer-shell, then X11, then noop. Layer-shell covers
  wlroots compositors **and KWin** (verified against KWin 6.7.3 on
  2026-07-31 — the "no-op on KDE" line that stood here was simply
  wrong). Mutter has no layer-shell, but GNOME Wayland sessions run
  XWayland, so the X11 override-redirect probe still maps a window
  there. The remaining Linux gap is a Wayland session with neither
  layer-shell nor XWayland. Never re-state this as a list of
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
  wherever it was parked. Since 0.17.8 a caret is used **only when it
  can be proved to belong to the focused window** — same process, same
  window size — because one desktop-wide slot holds whichever app
  moved a caret last, and a background chat window's caret composed
  with a maximised editor's rect placed the tooltip a thousand pixels
  off. The honest reading of that rung is therefore "caret-accurate in
  apps with a live bridge, window-accurate everywhere else", never
  "caret-accurate on Linux". See `docs/DECISIONS.md`, 2026-07-24,
  2026-07-29 and 2026-08-22.
  **Windows has a backend since 0.11.0, and a caret since 0.18.0.**
  A layered, topmost, `WS_EX_NOACTIVATE` window fed by
  `UpdateLayeredWindow`, sharing the renderer and the placement
  arithmetic the Linux backends already used — and now the same anchor
  chain, because `caret_hint()` reads `GetGUIThreadInfo`'s `rcCaret`
  off the **foreground thread**. That makes the sample this window's by
  construction, so unlike Linux it carries no age and nothing to prove:
  there is no desktop-wide slot to be confused by. Looked at on real
  hardware for the first time this release (see the 0.18.0 stamp).
  **Read the same caveat as everywhere else:** only apps that keep a
  real Win32 caret are ever seen. Notepad-shaped controls do; a program
  that draws its own — most browsers, most terminals, most Electron —
  reports `hwndCaret` as null and gets the window rung, exactly as
  before. Measured here: Notepad yes, Firefox no, our own iced Settings
  window no.
  The tooltip's own arithmetic also needed the process to stop being
  DPI-*unaware*, which every build up to and including 0.17.8 was: it
  sized itself from the monitor's real DPI and positioned itself in
  coordinates Windows had virtualised to 96, and the two only agree at
  100% scale. 0.18.0 ships a per-monitor-v2 manifest. **That fix is
  unobserved at any other scale** — this machine has one 100% panel.
  **macOS has a backend since 0.15.0** — a non-activating `NSPanel`,
  anchored on the Accessibility API's caret, hardware-verified on
  Intel (`docs/MACOS_POPUP.md`). Its caret answers are validated
  rather than trusted: Chrome and Terminal report ones nowhere near
  the text, and a rejected caret costs that window the caret rung, not
  the tooltip.

## AI, plug-ins and onboarding

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
  a banner when no layout switcher exists. Verified on Wayland/evdev,
  and on Windows since 0.18.0, where the pane has nothing to ask for
  and says so ("Nothing to set up on Windows") while naming the live
  backend; the macOS half (`AXIsProcessTrustedWithOptions`,
  `IOHIDCheckAccess`, the `x-apple.systempreferences:` deep links) is
  compiled by CI and executed by nobody. Two things it deliberately
  does NOT do: run anything with `sudo` (the Linux button copies the
  command instead), and imitate a system permission dialog (macOS shows
  its own). What is still missing from issue #10's wish list is the
  screenshots/GIFs of the macOS toggles.

## Updating and installing

- **Self-update: Linux and macOS are proven end to end; Windows was
  broken from the first release that shipped it until 0.20.0.**
  The AppImage path is exercised. The `.app`-bundle swap was validated
  on Apple Silicon by a contributor at 0.19.0 (0.18.1 → 0.19.0 on an
  M1 Pro, issue #3) — it had been written from Apple's docs and never
  run. It also strips `com.apple.quarantine` from the installed bundle:
  defensible only while the app is unsigned, and it must come out the
  day we ship notarised builds.
  **Windows never once installed an update of its own accord.** The
  0.18.0 pass recorded the MSI path as "watched", and it had been —
  but what was watched was the *script*, started by hand from a
  console. The step nobody watched was the hand-off, and the hand-off
  was where it was broken: the installer was spawned with
  `DETACHED_PROCESS`, which leaves a process with no console at all,
  and Windows PowerShell 5.1 cannot start without one. The event log
  on the maintainer's own machine records five spawns across three
  releases, each logging "PowerShell console is starting up" and then
  dying before its first statement, and not one Windows Installer
  transaction from the staging directory in the whole log. From inside
  the app it was indistinguishable from success: the tray quit, the
  update did not install, the app did not come back, and after three
  such clicks the verified download was deleted as un-installable.
  0.20.0 spawns with `CREATE_NO_WINDOW` — what the rest of this
  codebase already used — and no longer takes the app down on trust:
  every installer script announces itself, the app reads that greeting
  back before quitting, and a hand-off that never happened leaves the
  app running and says so. **A note for whoever verifies this next:
  running the installer script by hand does not test the updater.**
  The only run that counts is clicking "Restart to update" in a tray
  fed by a published release.
  Still open: five minutes of retries on a busy Windows Installer
  (1618) is a guess, and a machine whose installer is busy for longer
  still loses that attempt — though it now says so instead of going
  quiet.
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

## Loose ends, per platform

- **macOS: what 0.7.0 changed in the input path has still never been
  exercised.** 0.6.2 was runtime-tuned on real hardware (macOS 15,
  Intel, by an outside contributor) — and then 0.7.0 rewrote the part
  that had been validated: the tap subscribes to `FlagsChanged`, so
  modifier edges reach the word buffer, and the emitter clears event
  flags and posts modifier releases. Real Macs have been back since —
  0.15.0 for the tooltip, focus tracking and caret validation (Intel),
  0.19.0 for Apple Silicon, where first launch, detection,
  corrections, the force-switch hotkey and the self-update were all
  confirmed on an M1 Pro (#3) — but a correction typed under a *held*
  modifier, which is exactly what those changes are for, has been in
  nobody's report. If one arrives about words vanishing or a ⌘⌫ going
  out instead of a correction, start here.
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

## Deliberately out of scope (not gaps)

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
