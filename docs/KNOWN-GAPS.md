# Known gaps (as of v0.25.5)

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

**What the 0.25.5 pass actually checked (2026-08-30).** One report, and
one desktop can even have it: KDE Plasma Wayland, where it was reported
from. `ksycoca` is the only on-disk menu cache among the sessions this
guest offers — GTK and Qt-without-Plasma read a `.desktop` entry per
process and cannot go stale this way — so a sixteen-session sweep would
be fifteen rows of "not applicable".

Measured with `menu-probe.sh`, which asks the *desktop* what it would
launch rather than reading the file, and never refreshes anything: an
entry the session has already indexed, then the AppImage replaced by a
differently-named one, then two minutes of watching. Both builds, same
session, same hour:

| build | `applications/` mtime | what the menu would launch |
|---|---|---|
| 0.25.4, entry rewritten in place | unchanged | the old path, still, after 120 s |
| 0.25.5, entry renamed into place | moves | the new path, within 5 s |

The rest of the release is a rename where there was a write, so nothing
in the correction path moved and the 0.25.3 desktop sweep stands. The
property the fix rests on — that replacing the file, rather than its
contents, is what a menu cache can see — is a unit test now, and that
one runs on every platform CI builds.

**What the 0.25.4 pass actually checked (2026-08-29).** One report on
0.25.3, and no desktop dimension to it: whether the force-switch plays a
sound is decided in `poltertype-core` before any backend is involved,
and the guest has no audio device to hear it with. So the reading is a
unit test rather than a sweep — but a new kind of one. The engine's test
audio player used to drop every command on the floor, so sound was the
one thing nothing in this repository asserted on, and that is exactly
how a hardcoded `play_sound: true` sat in the manual-switch path since
the gesture existed (#47). The player now hands its receiver back and
the test reads both directions: chime off means silence, chime on means
one `Correct`.

The 0.25.3 desktop sweep stands unchanged — nothing in this release
touches the correction path itself.

**What the 0.25.3 pass actually checked (2026-08-29).** The one thing
this release changes, on every session the guest offers, plus a re-run
of the three the 0.25.2 pass covered. `edit-probe.py` clears a line —
once with `Ctrl+Backspace`, once with `Ctrl+A` and a plain Backspace —
and then asks both the force-switch and the automatic pass for the word
typed straight afterwards.

Each phase is measured against its own **reference line**: the same word
typed and closed with Enter, which is a submission boundary and so
switches nothing and corrects nothing. The measured line then has to
come back as something else. The first version of this probe counted
switches instead and expected an even number to land back where it
started — and one correction abandoned because the chord still looked
held, which is 0.25.1 behaviour and not a fault, shifted every line
after it and reported GNOME as broken while the app's own log carried
no refusal at all. The last two columns exist so that those two can
never be read as each other again: `refusals` counts the decline this
release is about, `applied/tried` shows a correction that started and
stood down.

| session          | corrects at all | chord | after Ctrl+Backspace | after Ctrl+A + BS | auto after the delete | refusals | applied/tried |
|------------------|-----------------|-------|----------------------|-------------------|-----------------------|----------|---------------|
| budgie-desktop   | **no**          | n/a   | n/a                  | n/a               | n/a                   | 0        | 0/0           |
| cinnamon         | yes             | yes   | yes                  | yes               | yes                   | 0        | 3/3           |
| cinnamon-wayland | **no**          | n/a   | n/a                  | n/a               | n/a                   | 0        | 0/0           |
| cinnamon2d       | yes             | yes   | yes                  | yes               | yes                   | 0        | 3/3           |
| fluxbox          | yes             | yes   | yes                  | yes               | yes                   | 0        | 3/3           |
| i3               | yes             | yes   | yes                  | yes               | yes                   | 0        | 3/3           |
| icewm-session    | yes             | yes   | yes                  | yes               | yes                   | 0        | 3/3           |
| labwc            | **no**          | n/a   | n/a                  | n/a               | n/a                   | 0        | 0/0           |
| lxqt             | yes             | yes   | yes                  | yes               | yes                   | 0        | 3/3           |
| mate             | **no**          | n/a   | n/a                  | n/a               | n/a                   | 0        | 0/0           |
| openbox          | yes             | yes   | yes                  | yes               | yes                   | 0        | 3/3           |
| plasma           | yes             | yes   | yes                  | yes               | yes                   | 0        | 3/3           |
| sway             | yes             | yes   | yes                  | yes               | yes                   | 0        | 3/3           |
| ubuntu           | yes             | yes   | yes                  | yes               | yes                   | 0        | 3/3           |
| xfce             | yes             | yes   | yes                  | yes               | yes                   | 0        | 3/3           |
| xfce-wayland     | **no**          | n/a   | n/a                  | n/a               | n/a                   | 0        | 0/0           |

Eleven of sixteen sessions can be read at all, and all eleven are green
in every column: no refusal anywhere in the sweep, and every correction
the gesture started also finished. KDE Plasma Wayland, which is where
#44 was reported from, is one of them.

The five that cannot be read are the rows this matrix has always called
unmeasurable, and none of them are about this release. Budgie, labwc and
Xfce's Wayland session have no layout backend, so nothing is ever
corrected and "the gesture answered" and "nothing happened" put the same
bytes in the terminal. cinnamon-wayland delivers no injected key to the
application at all. **MATE** is the interesting one: the X11 backend
stands down there by design — that desktop's settings daemon owns the
xkb group and its state does not track what the keyboard produces, so a
correction would delete a word and retype it unchanged — and pinning the
gsettings backend instead gets four gestures that start a correction and
stand down, with no refusal logged. Its row read `no` at 0.25.1 for the
same reason.

`config-probe.py` re-ran unchanged and every session that delivers our
injected keys is green on all six of its columns — the chord bound, the
old one retired, the new one live, pause written, read back at startup,
and resumed — with cinnamon-wayland the only `n/a`, for the reason
above. `tray-probe.py` likewise: every session with a StatusNotifier
host that lists our item shows the missed-word submenu holding the word.
It now reads the submenu's own **label** off D-Bus as well, because
naming the dictionary there is the other half of what this release does
for #38; on KDE Plasma Wayland it comes back as `Add a missed word to
the dictionary…`, which is the string the app was built with.

**What the 0.25.2 pass actually checked (2026-08-29).** Two settings
that only exist between a file and a running app, so neither can be
read off a screen. `config-probe.py` rewrites `config.toml` under a
running app and then presses the new chord and the old one; it also
quits the app and starts it again to see whether the pause state came
back. `tray-probe.py` reads the missed-word submenu off the tray's own
`com.canonical.dbusmenu` — the rows the desktop is holding, not the
ones we think we sent.

| session | display | chord bound | old chord dead | new chord live | pause written | starts paused | resumes | missed-word row |
|---|---|---|---|---|---|---|---|---|
| budgie-desktop | wayland | yes | yes | yes | yes | yes | yes | n/a |
| cinnamon | x11 | yes | yes | yes | yes | yes | yes | yes |
| cinnamon-wayland | wayland | **n/a** | n/a | n/a | n/a | n/a | n/a | n/a |
| cinnamon2d | x11 | yes | yes | yes | yes | yes | yes | yes |
| fluxbox | x11 | yes | yes | yes | yes | yes | yes | n/a |
| i3 | x11 | yes | yes | yes | yes | yes | yes | n/a |
| icewm-session | x11 | yes | yes | yes | yes | yes | yes | n/a |
| labwc | wayland | **n/a** | n/a | n/a | n/a | n/a | n/a | n/a |
| lxqt | x11 | yes | yes | yes | yes | yes | yes | yes |
| mate | x11 | yes | yes | yes | yes | yes | yes | n/a |
| openbox | x11 | yes | yes | yes | yes | yes | yes | n/a |
| plasma | wayland | yes | yes | yes | yes | yes | yes | yes |
| sway | wayland | yes | yes | yes | yes | yes | yes | n/a |
| ubuntu | wayland | yes | yes | yes | yes | yes | yes | yes |
| xfce | x11 | yes | yes | yes | yes | yes | yes | yes |
| xfce-wayland | wayland | yes | yes | yes | yes | yes | yes | n/a |

Fourteen of sixteen green in every column. The two that are not are
cinnamon-wayland and labwc, where no injected key reaches the app at
all — the same two rows the correction table calls unmeasurable, for
the same reason. `n/a` in the last column is a session with no
StatusNotifier host, or one whose watcher never registered our item
(Budgie and Xfce's Wayland session both have a watcher and neither
lists us); the row measures nothing there rather than failing. The
missed word itself was seen arriving in the tray on KDE Plasma
Wayland, which is where #38 was reported.

The X11 rows are the load-bearing ones for the rebind: there a chord is
an OS-level grab, so changing it means releasing one grab and taking
another, and that is the half that can fail quietly.

**The first sweep's rows for this probe were wrong, and the app was
not what changed.** The probe waited a fixed eight seconds after
starting the app — but the config watcher is started with the event
loop, and building that loop initialises GTK, which on a session with
no tray host takes tens of seconds. So the file was rewritten before
anything was watching it, and sway read as a rebind that never took.
It now waits for the app to say it is up, and keeps one virtual
keyboard across the restart rather than making a second one the app has
to rescan for. Same binary, same commit: plasma, sway, budgie, mate and
xfce-wayland all went from failing to green.

The correction matrix itself is unchanged from 0.25.1 — every session
with a layout backend corrects, `Shift+Shift` puts the word back, all
three 0.22.0 phases pass, and no control character reached any
application anywhere in the sweep. GNOME needed a second run of the
hotkey probe: the first left two of six lines corrected-but-not-undone,
the second was clean in all six. That is the gnome-settings-daemon
layout-revert race already recorded for 0.25.1, not a change in this
release.

**What the 0.25.1 pass actually checked (2026-08-29).** The
force-switch hotkey again, in every shape the guest can produce, on
every session the display manager offers — because the release is two
fixes to the previous release's fix and neither was caught by a test.

The probe now drives six lines instead of two: the chord tapped, held
~1 s, held ~3 s, tapped again, held ~6 s — past the wait — and tapped
once more. The last two are what #44 turned out to need. It also counts
**control characters** in the capture, which is the only reading that
separates "nothing was typed" from "something illegible was": a burst
that goes out under a held Ctrl arrives as `^H` and the control code of
every letter, and `read_lines` strips escape sequences and applies
erases, so it renders that as a plausible-looking word.

| session        | corrects at all | tap | held ~1 s (#39) | held ~3 s (#44) | pressed after (#44) | held past the wait | pressed after that | Caps Lock (#41) | stray control bytes |
|----------------|-----------------|-----|-----------------|-----------------|---------------------|--------------------|--------------------|-----------------|---------------------|
| budgie-desktop | **no**          | n/a | n/a             | n/a             | n/a                 | n/a                | n/a                | n/a             | 0                   |
| cinnamon       | yes             | yes | yes             | yes             | yes                 | yes                | yes                | yes             | 0                   |
| cinnamon2d     | yes             | yes | yes             | yes             | yes                 | yes                | yes                | yes             | 0                   |
| fluxbox        | yes             | yes | yes             | yes             | yes                 | yes                | yes                | yes             | 0                   |
| i3             | yes             | yes | yes             | yes             | yes                 | yes                | yes                | yes             | 0                   |
| icewm-session  | yes             | yes | yes             | yes             | yes                 | yes                | yes                | yes             | 0                   |
| labwc          | **no**          | n/a | n/a             | n/a             | n/a                 | n/a                | n/a                | n/a             | 0                   |
| lxqt           | yes             | yes | yes             | yes             | yes                 | yes                | yes                | yes             | 0                   |
| mate           | **no**          | n/a | n/a             | n/a             | n/a                 | n/a                | n/a                | n/a             | 0                   |
| openbox        | yes             | yes | yes             | yes             | yes                 | yes                | yes                | yes             | 0                   |
| plasma         | yes             | yes | yes             | yes             | yes                 | yes                | yes                | n/a             | 0                   |
| sway           | yes             | yes | yes             | yes             | yes                 | yes                | yes                | n/a             | 0                   |
| ubuntu         | yes             | yes | yes             | yes             | yes                 | yes                | yes                | n/a             | 0                   |
| xfce           | yes             | yes | yes             | yes             | yes                 | yes                | yes                | yes             | 0                   |
| xfce-wayland   | **no**          | n/a | n/a             | n/a             | n/a                 | n/a                | n/a                | n/a             | 0                   |

Every session that can switch layouts is green in every column, and no
control character reached any application anywhere in the sweep. Read
only those rows: Budgie, labwc, MATE and Xfce's Wayland session have no
layout-switching backend, so nothing is corrected there and every
hotkey verdict is unmeasurable rather than failed. Caps Lock reads
`n/a` on the three Wayland sessions that do correct, for the same
reason as in 0.25.0 — the binding needs `caps:none` and none of the
three can be given it from outside.

Two sessions were **not run**: Cinnamon's Wayland session never became
ready (it has produced no terminal in any sweep since it was
installed), and `i3-with-shmlog`, which is i3's debug variant and is
covered by the i3 row.

One row is honestly intermittent, and it is not this release's doing.
GNOME's settings daemon sometimes puts the layout back before
PolterType can type, and the guard then declines with `the desktop put
the layout back before we could type; leaving the word alone` — the
right answer, but it makes whichever phase it lands on read as a
failure. Three runs on GNOME: the race was lost in two of them, at a
different phase each time, and the third was green in every column.
Nothing in this release touches that path, and the new wait was never
the cause — zero give-ups in every one of those logs.

What this pass did **not** check: anything on macOS or Windows. The
engine change is platform-neutral and unit-tested; the Hotkeys-pane
change was measured on this laptop's own session rather than compiled
and hoped for, but on Wayland/Hyprland, not on the platforms whose
listeners report Caps Lock differently.

**What the 0.25.0 pass actually checked (2026-08-28).** The
force-switch hotkey, which four of the five reports are about, across
the whole desktop matrix — and two shapes of it the matrix had never
been able to ask.

`matrix.py` binds `Shift+Shift`, because a modifier-only chord needs no
OS-level grab and so is the one shape that works on every session in
the guest. That is exactly why the sweep had never measured the two
that were reported: an ordinary chord **held down** past the kernel's
repeat delay (#39), and **Caps Lock** as the whole binding (#41). A
second probe runs those now, each as its own app run with its own
`config.toml`, after every session. A third phase went into `matrix.py`
itself for #40 — the hotkey on a word with no separator after it,
twice, then that word rubbed out, a shorter one typed, and the hotkey
again, which has to act on the second word.

| session | corrects at all | hotkey undo | pressed twice | the word after one it switched (#40) | chord held down (#39) | Caps Lock (#41) |
|---|---|---|---|---|---|---|
| KDE Plasma (Wayland) | yes | yes | yes | yes | yes | n/a |
| sway | yes | yes | yes | yes | yes | n/a |
| GNOME (Wayland) | yes | yes | yes | yes | yes | n/a |
| Cinnamon (X11) | yes | yes | yes | yes | yes | yes |
| Xfce (X11) | yes | yes | yes | yes | yes | yes |
| LXQt | yes | yes | yes | yes | yes | yes |
| i3 | yes | yes | yes | yes | yes | yes |
| openbox | yes | yes | yes | yes | yes | yes |
| fluxbox | yes | yes | yes | yes | yes | yes |
| IceWM | yes | yes | yes | yes | yes | yes |
| MATE | **no** | n/a | n/a | n/a | n/a | n/a |
| Budgie (Wayland) | **no** | n/a | n/a | n/a | n/a | n/a |
| labwc | **no** | n/a | n/a | n/a | n/a | n/a |
| Xfce (Wayland) | **no** | n/a | n/a | n/a | n/a | n/a |
| Cinnamon (Wayland) | **no** | n/a | n/a | n/a | n/a | n/a |

Read only the rows where the session corrects at all. Budgie, labwc,
Xfce's Wayland session and MATE have no layout-switching backend in
this guest, so nothing is ever corrected there and "the hotkey put the
word back" and "nothing happened" write the same bytes; Cinnamon's
Wayland session delivered nothing to the terminal, as it has in every
sweep since it was installed. Caps Lock reads `n/a` on the three
Wayland sessions that do correct because the binding needs `caps:none`
and none of the three could be given it from outside: sway keeps its
keymap in its own config, and KDE's `kxkbrc` route did not take.

That is also what closes a gap this file had carried since 0.20.0 —
the mid-word switch that "did nothing at all" on Xfce/X11 under
`Ctrl+Shift+Backspace`. It was the Ctrl, not the Backspace, and Xfce
(X11) is a yes in every column above.

What this pass did **not** check: anything on macOS or Windows. Four
things in this release live there — the TCC state the Setup pane now
recognises (#42), the two system-theme probes, the punctuation
scancodes as those listeners report them, and Caps Lock on the
key-stream path — and every one is compiled, unit-tested where there is
logic to test, and run by nobody.

One measurement in this pass was wrong for most of an afternoon, and
the next person will hit it too. An ordinary chord hotkey reaches the
focused application as well as us — that is what `observed_not_consumed`
means — and a terminal renders `Ctrl+Shift+F9` as six literal bytes of
`ESC [20;6~`, which land in the line the correction is about to fix and
soak up six of its seven backspaces. Every Wayland row then read as a
correction that erased nothing, which looks exactly like a regression
and is not one: strip the escape sequence before applying the erases
and the word is correct. X11 does not show it, because there the grab
consumes the key.

**What the 0.24.0 pass actually checked (2026-08-28).** Selection
conversion, which the release exists for, in two passes.

End to end on **KDE Plasma Wayland** in the matrix guest: a two-word
passage typed on the wrong layout, selected, the hotkey pressed — the
converted text on screen, and the clipboard afterwards holding what it
held before. That run is also what found the two mechanisms that
*compile* and do the wrong thing (`send_text` typing `43f` through
Wayland's compose path; a scancode replay that cannot express the
newline `Ctrl+A` picks up) and the modifier trap: the hotkey fires on
the press, so its own `Ctrl+Shift` is still down when the copy chord
goes out.

Then the precondition on **all fifteen sessions** — can a process with
no window and no focus round-trip this session's clipboard at all. That
is the table below, and it is a measurement of the *session*, not of
PolterType: the same question the app's own probe asks before it lets
the toggle be ticked. Two sessions answer no.

What this pass did **not** check: the end-to-end conversion on the
other fourteen sessions, or on Windows. The Windows `send_chord` is new
in this release and has been **compiled, not run** — the engine's *use*
of it is unit-tested through a fake emitter, but no test and no person
has exercised the `SendInput` path on a Windows machine. macOS is not
a gap here for once: it has no `send_chord` at all, so the toggle
reports itself unavailable rather than half-working.

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
- **The manual switch on a word still being typed: the X11 hole has a
  name now.** 0.20.0 made the force-switch act on the word under the
  caret, and it was measured working on KDE Plasma Wayland and GNOME
  Wayland — where the default chord is already substituted to
  `Ctrl+Shift+F9` — while the same probe on Xfce/`xfwm4` (X11, default
  chord `Ctrl+Shift+Backspace`) switched the *finished* word and did
  nothing at all mid-word, logging an empty word buffer. This file said
  the reason was not established, and ruled out the obvious suspect: a
  Backspace pops one key rather than clearing the buffer.
  It was not the Backspace. It was the **Ctrl**. `handle_key` reads any
  press carrying Ctrl/Alt/Meta as a shortcut that may have edited the
  text arbitrarily and drops the word buffer — so the chord's own key
  emptied the buffer the switch was about to read. 0.25.0 exempts the
  force-switch chord's own key from that (and only that chord: the
  pause default is Space, which would close the word). The same taint
  is what made the gesture stop answering for every word typed after
  one it had switched, which is how it was reported (#40).
  Measured, not just reasoned: Xfce (X11) is a yes in every column of
  the 0.25.0 table above, as is every other session in the guest that
  can switch layouts at all. Windows and macOS carry the same
  Backspace-based default and have never been measured for it.

- **Caps Lock works as the force-switch key, and only if you take the
  lock off it first** (0.25.0, #41). PolterType watches keys and never
  swallows them, so binding this one still latches the lock on every
  press — and a latched lock makes the corrected word come back in
  capitals, because the replay is scancodes and the *system* applies
  the lock to them. `caps:none` (or the equivalent in whatever remapper
  you run) is what makes the key carry the binding and nothing else;
  the Hotkeys pane says so under the row. Once it is neutralised no
  OS-level shortcut registry can find the key any more — `XGrabKey`
  resolves a key through its keysym and it no longer has one — so the
  binding is read off the key stream on every platform instead.
  Measured on all seven X11 sessions in the guest that can switch
  layouts, 2026-08-28: bound live the word comes back in capitals,
  bound with `caps:none` it comes back exactly as typed. Unmeasured on
  the Wayland sessions, where nothing outside the compositor can set
  that option — sway keeps its keymap in its own config — and on
  Windows and macOS, where the same key-stream path is now used and the
  listeners do report the key.

  The precondition and the Hotkeys pane were at odds until 0.25.1:
  neutralising the key is what leaves it with no keysym, the pane
  matched Caps Lock by *name*, and so *Rebind* recognised the key only
  until the user did the one thing the pane told them to do. It now
  matches the physical code. Measured both ways on a live `caps:none`
  session, 2026-08-29 — 0.25.0 never sees the key, 0.25.1 captures
  `CapsLock`.

- **A correction cannot happen while the key that asked for it is
  still down, on either display server** (0.25.1, #44). On X11 the
  passive grab that delivered the chord goes *active* on the press:
  everything the correction emits is handed to the grabbing client
  rather than to the application, and the key's own release is never
  delivered to us at all. On Wayland the modifiers are the problem
  instead — the release PolterType sends for them comes from a virtual
  keyboard that never pressed them and changes nothing, so the burst
  arrives as `Ctrl+H`, `Ctrl+G`, `Ctrl+B`. Both measured in the guest,
  2026-08-28/29. So nothing is switched, deleted or typed until the
  key comes up; past five seconds the word is left exactly as typed
  and the gesture stays live for the next press. What this costs is
  real and is the design: **lean on the key for more than five seconds
  and that press does nothing at all.**

- **A shortcut that edits the line without a backwards deletion we can
  see still leaves the force-switch declining the next word** (#44). A
  press carrying Ctrl/Alt/Meta can edit the text arbitrarily, so the
  buffer stops vouching for what is on screen, and both the manual
  switch and the automatic pass then refuse the word typed straight
  afterwards — a word half-switched in place is worse than one left
  alone. The refusal is deliberate; how far it reached was not. From
  0.25.3 a deletion lifts it, because a deletion erases the very text
  the refusal protects: `Ctrl+Backspace`, `Cmd`/`Alt+Backspace`, and any
  plain Backspace run that goes past everything we track — which is what
  `Ctrl+A` or `Shift+Home` followed by Backspace comes to.

  What still declines is a selection removed with **Delete** rather than
  Backspace — from the outside that is indistinguishable from a Delete
  with no selection, which leaves the text to the left exactly where it
  was — and any shortcut that edits nothing at all (`Ctrl+C`, `Ctrl+S`,
  `Ctrl+Z`) pressed in the middle of a word. A space re-syncs the buffer
  in every case, and the refusal now has its own debug line rather than
  sharing one with "the hotkey fired and there was nothing to switch".
  Measured across the desktop matrix, 2026-08-29.

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

- **Anything the app writes to `config.toml` drops comments you added
  by hand.** The file is serialised from the settings struct, never
  edited in place. That has always been true of the Settings window's
  Save; since 0.25.2 it is also true of pausing, which records its
  state there.

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

- **Selection conversion is unavailable on GNOME and Cinnamon's
  Wayland sessions, and on macOS.** Reading the clipboard from a
  process that holds no window and never takes focus needs a
  data-control protocol. Measured 2026-08-28 on **all fifteen matrix
  sessions**, by asking each one whether a windowless process can put a
  marker on its clipboard and read the same marker back:

  | session | `ext_data_control` | `zwlr_data_control` | windowless round trip |
  |---|---|---|---|
  | KDE Plasma (Wayland) | yes | no | yes |
  | sway, labwc, Budgie, Xfce (Wayland) | yes | yes | yes |
  | **GNOME (Wayland)** | **no** | **no** | only by taking focus |
  | **Cinnamon (Wayland)** | **no** | **no** | **no** |
  | Cinnamon, Xfce, MATE, LXQt, i3, openbox, fluxbox, icewm (X11) | n/a | n/a | yes |

  The eight X11 sessions need no protocol — an X client owns a
  selection without a mapped window — and all eight round-trip. Of the
  seven Wayland sessions, five advertise a data-control protocol and
  two do not.

  GNOME is the trap in that table: `wl-clipboard` appears to work
  there, because it falls back to creating a surface and taking focus —
  the one thing this app will not do. The library we link has no such
  fallback, so the feature reports itself unavailable instead of
  stealing focus, and GNOME reads as unavailable despite the "yes" a
  naive probe returns.

  macOS is unavailable for a different reason: its emitter cannot yet
  hold modifiers around a key (`send_chord`), so it cannot press the
  copy shortcut at all. The Hotkeys pane disables the toggle and gives
  the reason in each case.
- **A clipboard holding an image or files cannot be preserved across a
  selection conversion.** The conversion reads and writes text; a
  non-text clipboard is replaced by the selection and not put back.
  Part of why the feature is off by default.
- **Nothing distinguishes a password field.** A selection inside one is
  copied like any other text while selection conversion is enabled.

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

- **Self-update: the Linux AppImage path was a coin flip until
  the fix below, macOS is proven end to end, and Windows was broken
  from the first release that shipped it until 0.20.0.**
  **Linux, found on a NixOS + Hyprland laptop and true of every
  session that runs the app as a systemd user service — including the
  unit PolterType's own "run at login" toggle writes.** The updater
  spawned a helper script, in its own process group, whose first act
  was to wait for the app's PID to disappear before swapping the
  AppImage. A process group is not a cgroup: systemd stops a service
  when its main process exits and, under the default
  `KillMode=control-group`, SIGKILLs everything still in that cgroup —
  so the helper was killed at the exact instant it was waiting for,
  inside its poll loop, before it reached the swap. It left no marker
  and no second log line, so from the outside "the app quit and never
  came back" was the entire symptom. One maintainer laptop's logs
  record eight "Restart to update" clicks across five releases: the
  five made from the systemd unit all failed, and the three that
  worked were instances the *previous* helper had started, which
  inherit a session scope instead — a scope has no main process, so
  nothing tears it down. The updater now renames the new AppImage over
  the old one from inside the app, where nothing has to outlive us:
  `rename`
  replaces a directory entry and leaves the running image's inode
  alone. Only the relaunch is delegated now, and under a service it
  goes to a transient `systemd-run` unit — its own cgroup — which
  starts the service again rather than launching the file beside a
  dead unit. An update that installs but cannot restart the app now
  says so and leaves the app running, instead of quitting into
  nothing.
  The `.app`-bundle swap was validated
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
- **On macOS every self-update costs both permissions, and until
  0.25.0 the app offered a button that could not give them back**
  (#42). Our bundles are ad-hoc signed, so TCC keys Accessibility and
  Input Monitoring to the code-directory hash rather than to a team
  identifier: the updater replaces the bundle, the hash changes, and
  the app is denied while both switches still read "on". Because a
  record exists, `AXIsProcessTrustedWithOptions` and
  `IOHIDRequestAccess` return without raising a dialog — so *Ask macOS
  now* did nothing at all, and the only fix is removing PolterType from
  each list and adding it back. `IOHIDCheckAccess` can tell a recorded
  denial from "never asked", which the Setup pane now uses to say that
  in words and open the pane; `AXIsProcessTrusted` cannot, so
  Accessibility follows Input Monitoring on the reasoning that TCC is
  asked for both at the same moment. The Updates card says the price
  before it is paid.
  **A Developer ID signature is the actual fix** and is out of scope
  until there is an Apple developer account; see
  `docs/CODE_SIGNING.md`. Everything in this bullet is **compiled, not
  run** — there is no Mac here, and the reporter's machine reproduces
  it every time.
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
