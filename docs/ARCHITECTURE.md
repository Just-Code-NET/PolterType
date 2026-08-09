# Architecture

Why the pieces are shaped the way they are. This file exists so the
source can stay short: a module header says *what* a module is, and
links here for *why*. Dated decisions with a "we considered X and
rejected it" shape live in [DECISIONS.md](DECISIONS.md) instead; this
file is the standing explanation.

See also [PLAN.md](PLAN.md) for the product-level design and
[PERMISSIONS.md](PERMISSIONS.md) for what each platform demands of the
user.

## Contents

- [Settings UI: why a separate process](#settings-ui-why-a-separate-process)
- [Smart commands: why text triggers, not hotkeys](#smart-commands-why-text-triggers-not-hotkeys)
- [The correction path](#the-correction-path)
- [Key gate: holding keystrokes back](#key-gate-holding-keystrokes-back)
- [Detector pipeline](#detector-pipeline)
- [Layout tables and their precedence](#layout-tables-and-their-precedence)
- [Focus tracking](#focus-tracking)
- [Suggestion tooltip](#suggestion-tooltip)
- [Plug-ins](#plug-ins)
- [Updater](#updater)

## Settings UI: why a separate process

The tray (`tao::EventLoop` + `tray-icon`) and `iced` both want to own
the platform's main thread on macOS — `NSApplication` is a singleton
and the tray already binds it. Rather than choreograph a thread swap,
the Settings UI ships as a CLI subcommand, `poltertype --settings`,
which the tray spawns as a child process.

The two share nothing at runtime: the UI reads and writes
`config.toml` on disk, and when it exits the tray sees
`SettingsReloaded` and refreshes its caches. The named pane
(`--setup`, `--plugins`) is the entire protocol between the two
processes.

The subprocess boundary is not just a macOS workaround. A crash in the
UI cannot take down the engine or the keyboard hook; all three
platforms behave identically with no per-platform thread juggling; and
the UI can be run on its own, in a debugger, or from a test driver.

Two consequences worth knowing before editing `settings_ui`:

- **The i18n catalog must be loaded before the first widget exists.**
  `tr` is called from the view function, which runs every frame.
  Finding no catalog is not an error — every call site is written in
  English and simply stays that way.
- **`exit_on_close_request(false)` is load-bearing.** The window
  intercepts the close request so an unsaved Wordlists edit is flushed
  to disk first. Without it, a user who typed a word and used the
  window's close button instead of Save lost the edit silently.
- **A pane the window *opens* on never fires its selection handler.**
  Controls that have to ask a plug-in are kicked off from
  `startup_task()` for exactly that reason, or the pane sits at
  "Asking the plug-in…" for ever.

## Smart commands: why text triggers, not hotkeys

Smart commands are text-trigger expansions in the manner of
TextExpander, Espanso and AutoHotkey hotstrings: the user types a short
token like `anrl `, the engine recognises it on the word boundary,
deletes the token and the boundary, and runs an action.

`[hotkeys]` already spends the user's two global key combinations
(pause, switch-last). More global hotkeys would be a different
feature with worse properties: they collide with system-wide bindings,
they are invisible where a typed trigger is right there in the text,
and the OS caps how many can be registered. Text triggers have none of
those limits — they live inside the same word-boundary pipeline that
already does layout correction, so a user can have hundreds at no
cost.

**Ordering is significant.** Trigger lookup runs on every word
boundary *before* the structural-boundary, disabled-app and identifier
filters:

1. User types `anrl<space>`.
2. Word boundary fires.
3. Trigger lookup: `anrl` matches → dispatch the action, backspace the
   token and the boundary, re-emit whatever text the action leaves
   behind, return.
4. Otherwise the normal layout-correction pipeline runs.

Running before the filters is what makes a snippet like `=>` work
inside an IDE. Those filters exist to veto *automatic* switching;
expansion is what the user actively asked for, so they do not apply.

**The action surface is deliberately small.** Each variant maps to one
OS primitive already known to be safe: `TypeText` → `send_text`,
`SwitchLayout` → `switch_to`, `OpenPath` → `opener::open`.
`RunShell` (0.10.0) is the one with a threat model rather than a
one-line description — off unless `[commands].allow_run_shell`, never
run through a shell, never handed anything the user typed as an
argument. `commands/shell.rs` holds the reasoning; read it before
changing any of that.

Multi-token triggers also landed in 0.10.0. The word buffer still
resets at every boundary, so `WordHistory` holds the last few
completed words alongside it — bounded by length, by the idle timeout
and by the focused application, because that history is the only place
the engine keeps more of the user's text than the word being typed.

Two things are intentionally absent. **Case-insensitive matching**:
triggers are matched exactly, so users can pick tokens that do not
collide with prose — a case-insensitive `best regards` would fire on
an ordinary sign-off. **Placeholders that substitute typed text into
an action**: for `run_shell` that is an argument-injection channel, and
elsewhere nobody has asked for it.

## The correction path

A correction is three phases with a hard ordering, all in
`engine/switcher/correction.rs`: **absorb → emit (delete, replay) →
flush**. Every part of it exists because a keystroke the user lands
mid-correction is ordered against our emitted events *by the
compositor*, and no after-the-fact counting can undo that.

**Switch the layout first.** Flipping the layout does not touch
existing text, so a failed switch aborts with the user's word intact.
The old order — backspaces, then switch — destroyed the word every
time the switch failed. Switching first also overlaps the
compositor's xkb propagation with the backspace burst, and means
keystrokes landing mid-correction already produce glyphs in the layout
the user intended.

**Absorb.** Before deleting anything, watch the key stream. While
presses keep arriving, fold them into the plan — they are the start of
the user's next word, already on screen in the layout just switched
to. Emission starts only after the stream has come back empty three
times running (~60 ms; a fast typist's inter-key gap is 45–60 ms plus
listener lag, so two probes can land inside a single gap). A
correction fired by a chord also waits for that chord to come up: our
replay reaches the application the way the user's keys do, so typing
under a held `Ctrl` produces shortcuts and nothing lands.

If a *boundary* arrives while absorbing, the user finished their next
word too — include it and re-process it afterwards so it gets its own
decision. If a submission (Enter/Tab) or anything murkier (Backspace,
navigation, a shortcut) arrives, abort the whole correction: nothing
has been emitted, so the text is untouched. The layout stays switched
and the buffer is tainted.

**Emit.** Erase the on-screen characters, then retype the corrected
word plus everything typed while preparing. Concretely, `зтзь ш `
came out as `ipnpm ` when the `i` reached the app between deletion and
replay, and as `pinpm ` / `pnpmi ` when it reached it mid-replay.
Replay prefers the original scancodes against the freshly switched
layout — the only path that works in Wayland-native and terminal apps.
Backends with a real Unicode-emit API (`KEYEVENTF_UNICODE`,
`CGEventKeyboardSetUnicodeString`) answer `Unsupported` and fall back
to `send_text`.

**Intrusion probe**, for when the gate is unavailable: anything on the
wire after the replay was pressed during it, so it is on screen
somewhere inside the text just typed. The position is unknown but the
character count is not, so erasing that many plus the intruders and
retyping restores typed order. That repair is itself a burst, so it
waits for a pause first — and if no pause comes, the screen is left
as-is and the engine stops vouching for it. A scrambled word the user
can fix beats a correction chasing their fingers across the line.

**Echo drain.** `consume_echo` matches by scancode, so while the echo
queue is non-empty a real user press of a scancode we just replayed
would be swallowed as an echo — very possible, since the next word
often starts with the same letters. Draining while the user is still
inside the verified pause empties the queue in one keyd round-trip.
Bounded, because backends that tag their echoes injected (Windows,
macOS) never send them back through the listener and would otherwise
wait for ever.

## Key gate: holding keystrokes back

The gate (`poltertype-input::KeyGate`) holds the user's keys back for
exactly the length of the burst. Held keys still reach us; they queue
instead of landing in the text, and we type them out once the
correction is down. Platform status, defaults and the remapper
interaction are in [PERMISSIONS.md](PERMISSIONS.md) and the project
README — do not restate them in source comments, they go stale.

The invariant that matters when editing: **held-and-dropped is
strictly worse than never held.** It is the one way this feature can
*lose* typing rather than merely scramble it. `send_keys` replays raw
scancodes, which is what the gate was written against on evdev;
Windows and macOS answer `Unsupported` on purpose, because their
Unicode-emit API is more robust there and replaying scancodes would
race the layout switch. Every flush path must therefore fall back to
`send_text` rather than give up — a gate that swallows keystrokes from
the application and then fails to give them back is a regression, not
a degradation. Keystrokes that are not characters (Backspace, arrows,
Esc) translate to nothing in any layout and are dropped; that loss is
bounded by one burst.

## Plug-ins

A plug-in is a **separate process**, never code loaded into this one.
That is what keeps the process holding the global keyboard hook out of
the blast radius of third-party code: a plug-in that panics, deadlocks
or is outright malicious costs the user that plug-in, not their
keyboard. Two kinds run — a **service**, started and stopped with
PolterType, and a **command**, a one-shot invocation behind a menu
entry or a button.

Four things the supervisor deliberately does not do:

- **No restart loop.** A service that dies stays dead until the user
  asks again. Automatic restart turns a plug-in that crashes on startup
  into a fork bomb that also fills the log, and hides exactly the
  failure the user needs to see.
- **No shell.** Arguments come from the manifest as a list and are
  passed as a list. There is no string to quote, so nothing to quote
  wrongly.
- **No inherited standard input.** A plug-in gets a null stdin and can
  never sit waiting on a terminal a tray app does not have.
- **No filtering of a plug-in's output.** It goes to
  `logs/plugin-<id>.log`, truncated at every start, because a tray app
  launched from a desktop entry has no terminal to inherit and the one
  line explaining a death would go nowhere. PolterType's rule about
  never logging typed text binds PolterType; a plug-in that reads
  keystrokes was trusted with them by being installed at all.

Stopping is a declared `stop` command rather than a signal, on every
platform. SIGTERM still requires the plug-in to install a handler, and
Windows' console control event was measured and refused — addressed to
the child's process group it returned success and did nothing;
addressed to the whole console it killed the sender. See
[DECISIONS.md](DECISIONS.md).

Every call into a plug-in is bounded. The state read runs on the UI
thread while a menu is drawn, so its deadline is short; a report runs
off it and gets longer. A pane that says "it did not answer" is
honest, one that never renders is not.
