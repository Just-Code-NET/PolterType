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
- [Hotkeys with no key in them](#hotkeys-with-no-key-in-them)
- [Choosing a layout backend on Linux](#choosing-a-layout-backend-on-linux)
- [The correction path](#the-correction-path)
- [Key gate: holding keystrokes back](#key-gate-holding-keystrokes-back)
- [Plug-ins](#plug-ins)

Subsystems not listed here keep their reasoning in the module header,
because it is short enough to live next to the code. Add a section when
a header starts growing a second argument — that is the signal it has
outgrown the source.

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

Four consequences worth knowing before editing `settings_ui`:

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
- **A gesture needs every subscription it is made of.** Capturing a
  hotkey means `on_key_press` *and* `on_key_release`; with only the
  first, a modifier tap updates the capture state and the gesture never
  completes — which is exactly how modifier-only binding shipped
  broken the first time. Nothing in the test suite can drive an iced
  subscription, so logic that has to be checked belongs in a pure
  function the subscription calls (`mod_capture_step`), not in the
  handler.

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

## Hotkeys with no key in them

Either global hotkey can be bound to modifiers alone — `Shift+Shift`,
or two held together like `Ctrl+Shift` (issue #32). Two properties of
that shape decide the design.

**It cannot be an OS hotkey.** `HotKey::new` needs a key code and there
is none, so a modifier chord is matched off the raw key stream instead
— the same stream the engine already reads on every platform, which is
why `SetKeystreamHotkeys` goes out unconditionally while `os_grab()`
covers only the `Key` bindings. It also has to be armed *before* the
event loop starts: on a session where GTK's start-up blocks, the tray
took 25 seconds to get there, and a hotkey that only works once the
tray is up is one the user reports as broken.

**Nothing fires on press.** The chord is judged when the last modifier
comes back up, and only if the set held was *exactly* the chord's, no
other key went down in between, and the hold was shorter than
`MOD_TAP_MAX`. That is what lets `Shift+Shift` live beside `Ctrl+C`,
`Ctrl+Shift+V` and typing capitals — each of those either dirties the
gesture or fails the exact-set test — and the tap deadline is what
keeps a Shift held while reaching for a capital from counting.

Two shapes are refused, and for reasons that live outside the matcher.
A **single** modifier is out because mouse buttons reach the engine on
Linux only (`SC_POINTER_BUTTON`): on Windows and macOS a bare `Shift`
binding would fire on every Shift+click, and a gesture that holds on
one platform and misfires on the others is worse than no gesture.
**Caps Lock** is out because it is observed rather than consumed —
binding it would flip the lock as well as fire.

Modifier chords are never consumed either: the keys still reach the
focused application. For a bare modifier that is harmless, and it is
why this is the one chord shape that behaves identically everywhere.

## Choosing a layout backend on Linux

Eight backends are probed in priority order (`linux/mod.rs`), and two
rules decide which one takes the session. A bug wrote each of them.

**A backend must be able to name a layout.** Ubuntu installs fcitx5
with language support and autostarts it, so `fcitx5-remote -t 1` exits
0 on a desktop where fcitx owns no input method at all, and `-n` then
answers with an empty line. Before the rule, that backend won on GNOME,
Xfce, MATE, LXQt, Budgie, sway, labwc and every bare WM, reported one
layout whose id was the empty string, loaded zero layouts, and logged
`layout switcher ready` on an app that could no longer correct
anything. A backend that initialises but names nothing now stands down
for the next one (`names_a_layout`).

**A backend must be the mechanism the session actually uses.**
`org.gnome.desktop.input-sources` lives in `dconf` — a file in the
user's home directory, which outlives the session that wrote it — so a
machine that once ran GNOME keeps a populated schema for ever. In i3,
fluxbox, icewm, LXQt, openbox or Xfce/X11 the gsettings backend would
then win, write a key nothing in that session reads, and decline every
correction with "the desktop put the layout back before we could type".
Finding the schema populated is therefore not enough: the session has
to be one whose own daemon acts on it (`session_reads_this_schema`).
The wlroots process check stays alongside that, because labwc reports
itself as Budgie.

Both rules are the same lesson as
[#26](https://github.com/Just-Code-NET/PolterType/issues/26): probe by
what a desktop *does*, never by what it ships or by what it calls
itself. The X11 XKB backend is probed last for a related reason —
where a desktop drives the session, locking the xkb group underneath it
leaves that desktop's own indicator lying.

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

**The boundary key is the exception, and has to be.** Re-reading the
*word* under the new layout is the entire correction; re-reading the
key that closed it is a second, unasked-for edit. `Shift`+`0x35` is
`,` in uk-UA and `?` in en-US, so replaying it as pressed rewrote the
user's punctuation. The separator is therefore looked up by character
in the target layout and replayed on whichever key produces it there
(`boundary_key_for`), falling back to the key as typed when the target
cannot produce that character at all — a few layouts reach some
punctuation through AltGr, which PolterType does not track.

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

Two kinds of thing are called a plug-in, and the manifest has to say
which before anything is installed. A **pack** is data — layouts,
wordlists, translations — and cannot execute at all; the installer's
allow-list is what guarantees that, and
[DATA_LAYOUT.md](DATA_LAYOUT.md) is its contract. An **extension**
ships a program. The kind is declared rather than inferred, so a pack
cannot quietly become executable by gaining a field.

The rest of this section is about extensions. An extension is a
**separate process**, never code loaded into this one. That is what
keeps the process holding the global keyboard hook out of the blast
radius of third-party code: a plug-in that panics, deadlocks or is
outright malicious costs the user that plug-in, not their keyboard.
Two kinds of process run — a **service**, started and stopped with
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

### The tray menu a plug-in declares

The same rule one storey down: a plug-in supplies text, PolterType
draws the menu. A manifest can name its entries in advance, but not a
queue — the things that arrived while nobody was looking — so a
`tray_list` names a command whose *rows* are produced while the menu
is being opened, in the same tab-separated form the pane's tick-box
lists use. Each row becomes a submenu of its own: the label is what
fits on one line, and the detail is one hover away, which is the only
place in a tray menu detail can live at all.

What a row can *do* comes from the manifest and nowhere else. The
plug-in supplies the row's text, never its actions, and `{id}` is
substituted as a whole argument rather than into one — so a row's own
text can never become a second flag.

The icon is shared, and stays PolterType's. A plug-in names the state
key that counts, and a value above zero puts a mark in the icon's
top-right corner — top, because the bottom-right is the pause
indicator and a paused PolterType with work waiting has to be able to
say both. The count itself goes in the tooltip. A plug-in gets to
raise that mark; it never gets to replace the icon, draw on it, or
choose what it looks like.

### The settings pane a plug-in declares

A plug-in describes its settings; PolterType draws them. That is the
whole isolation story on the UI side — a plug-in cannot render a pixel,
so it cannot imitate a system prompt, PolterType's own dialogs or
another plug-in. What it may declare is a small closed set of controls
(toggle, choice, suggest, text, number, decimal, strings, button,
report, list, records, section) bound to dotted keys in **its own**
config file. Edits go through `toml_edit`, so the prose in that file —
where a plug-in explains what each switch costs — survives. A control
kind this PolterType does not know loads as `Unknown` rather than
failing the parse, because serde's refusal would take the whole
manifest, and the plug-in with it, over one word from a newer version.

Seven decisions worth keeping:

- **A decimal is not a number.** TOML's integer and float are different
  types to the program reading the file back, and a plug-in expecting
  `0.35` cannot parse `1`. So `decimal` is its own kind and always
  writes a float, even for a round value.
- **Typing does not write.** A value settles when the user moves on,
  and at the latest when the window closes. Saving on every keystroke —
  which this pane used to do — walks a threshold from `0.9` to `0.95`
  through `0`, and for the length of that keystroke a gate in a running
  plug-in is wide open.
- **Sections are navigation, not decoration.** A capable plug-in has
  around a hundred settings, so PolterType lists its sections beside
  the window's own nav and shows one at a time. A section nobody is
  looking at is also unasked: a `list` whose rows come from the plug-in
  is not spawned until its section is on screen, which is what keeps
  opening this pane from waking every chat client on the machine.
  Exactly one region of the pane scrolls, so a wheel never lands on an
  ambiguous boundary.
- **An option that has to be explained stops being a drop-down.** A
  `choice` between `ask`, `auto` and `off` is three words. A choice
  between nine language models has to be *compared*, and a drop-down
  shows one at a time with nowhere to put the sentence saying what each
  is for — so an option may carry a `detail` and a `link`, and a choice
  with any described option is drawn as a column of radio rows instead.
  The link is `https` only, refused at manifest load and checked again
  at the click, and its **visible text is the address**: a plug-in
  supplying a destination is a third party deciding where PolterType
  sends somebody, and a friendly label over an arbitrary URL is exactly
  the shape the draw-it-ourselves rule exists to prevent.
- **`records` is for a setting that is a list of composite things** —
  entries of several fields each, where a `strings` list gives one
  unstructured line per entry and numbered slots cap at whatever number
  somebody guessed. The control names an array of tables and declares
  what one row holds; PolterType draws a card per entry with Add and
  Remove. Row fields are single names, not dotted paths, and cannot
  themselves be sections, buttons, reports or more records — a pane
  that nests is a config editor, and this is not one. A new row is
  written **empty**, so the plug-in's own defaults cover every field
  left out rather than a value nobody chose.
- **A `suggest` is a text box that knows the answers, not a choice with
  an escape hatch.** Its candidates are whatever the plug-in can see,
  but the set is not closed: what somebody wants may live in a client
  that is not running, and a `choice` gives no way to say so. So it is
  free text with the candidates beside it, narrowing as it is typed
  into, fed by the manifest's `options`, by a `command` (the same
  tab-separated rows a `list` ticks), or by both. **The row's id is
  what gets written, never its label** — what is picked has to be what
  is stored, and a name that reads well but identifies nobody is a
  message sent to nobody. Until the plug-in answers, the box stays a
  plain text box rather than a dead drop-down. Matches are drawn
  **inline and bounded** — a handful under the box, the rest counted —
  because iced's own combo box sizes its overlay to its options, and
  ninety-five of them covered the whole form.
- **A row of a `records` group can be acted on.** A group may declare
  `actions` and an `id_field`; each action is a button on that row's
  card, running a declared command with `{id}` substituted as one whole
  argument, never pasted into one — exactly as a tray list's per-row
  action works. Without it a pane can describe "send this at nine on
  Tuesday" and offer no way to find out before Tuesday whether it
  works, which for the one feature that writes to another person
  unattended is the wrong trade. A row whose naming field is empty gets
  the button **visibly disabled** rather than hidden: a command run
  against a nameless row is a command run against nothing. The button
  **waits for its command and shows what it printed**, because "did it
  go?" is the only question it is pressed to answer and a detached run
  answers it nowhere. It runs off the UI thread with a deadline long
  enough for an action that opens another application and types
  (`ACTION_TIMEOUT`, 90 s, against a report's 6), then refreshes the
  reports it just invalidated — those only, since re-asking a list
  would wake a client for a press that had nothing to do with it. One
  at a time: these steal focus.
