# Translating the PolterType interface

The settings window can speak your language. Adding one is **a single
TOML file** — no Rust, no rebuild, and you can test it on your own
machine before sending anything.

This is deliberately the same promise
[ADDING_A_LANGUAGE.md](ADDING_A_LANGUAGE.md) makes about keyboard
layouts: the things that vary between people live in data.

---

## The short version

1. Copy `data/i18n/uk.toml` to `data/i18n/<your-language>.toml`.
2. Translate the right-hand side of each line.
3. Pick your language under **General → Appearance → Interface
   language** and reopen the window.

That is the whole loop. To try it without touching the source tree,
drop the file into `<config-dir>/poltertype/i18n/` instead — the picker
offers whatever it finds on disk, so a file you dropped in yourself is
listed beside the shipped ones (under its bare code if PolterType has
no name for that language). `[general].ui_language` in `config.toml` is
the same setting, if you would rather type it.

---

## The format

One flat table. The key is stable and never translated; the value is
what appears on screen.

```toml
"languages.languages" = "Мови"
"footer.save" = "Зберегти"
```

Four rules, each of which exists because breaking it is easy:

* **A missing key is fine.** The English original is compiled into the
  program at every call site, so anything you have not translated
  stays English. A half-finished file is a useful file — send it.
* **An empty value means "not yet".** `"key" = ""` is ignored rather
  than drawn, because a blank button is worse than an English one.
* **`{}` is a value filled in at run time**, in order. Keep the same
  number as the English unless your language genuinely needs fewer;
  extra ones are left visible rather than crashing anything.
* **One bad line costs that line**, not your language. A value that
  isn't a string is skipped with a warning and everything else loads.

### What not to translate

* **PolterType.** The product name is the same in every language.
* **Layout ids** (`en-US`, `uk-UA`), config keys, file names and
  paths — they are things the user types, not things they read.
* **Keycap names** in hotkey chips (`Ctrl`, `Alt`, `Shift`) unless
  your platform genuinely labels them differently.

---

## Which file gets loaded

`[general].ui_language` in `config.toml` decides:

| Value | Effect |
|---|---|
| `"system"` (default) or `"auto"` | ask the environment |
| `"uk"`, `"pl"`, `"pt_BR"`, … | force that language |

Environment detection reads `LC_ALL`, `LC_MESSAGES` and `LANG`, in
that order — the same sequence the C library uses. **Windows sets none
of those**, so it lands on English unless the user picks a language
explicitly. That is a deliberate trade: reading the Windows locale
would mean platform-specific code in a crate that is not allowed to
hold any, and a picker the user can reach beats a guess.

A regional code falls back to the bare language: `uk_UA` finds
`uk.toml`. Ship a regional file only when the difference is real —
`pt_BR.toml` alongside `pt.toml` earns its place, `en_GB.toml` for one
word probably does not.

Three kinds of directory are read, in this order, and the last one to
name a key wins:

1. `<data_dir>/i18n/` — what the app ships.
2. `<data_dir>/plugins/<id>/i18n/` — what each installed plug-in ships
   (see the next section).
3. `<config-dir>/poltertype/i18n/` — yours.

Yours winning is what makes the edit-and-reopen loop possible.

---

## Translating a plug-in

A plug-in's settings pane and its tray entries are drawn by PolterType
but **worded by the plug-in**: their labels, explanations and options
come out of its `manifest.toml`. So a plug-in carries its own catalog,
in its own directory:

```
<data_dir>/plugins/<id>/i18n/<lang>.toml
```

Ask the plug-in for the file to start from — no key has to be guessed:

```console
$ poltertype --plugin-strings <id>
"summary" = "Answers your chats"
"pane.act.mode.label" = "Mode"
"pane.act.mode.option.auto" = "Automatic"
"pane.schedule.sends.field.room.label" = "Room"
```

Keys are derived from the manifest's own structure — a control's config
key (`act.mode`), or the command it runs, or a slug of its English label
for a tray entry or a section, which bind to neither. Translate the
right-hand side, save as `i18n/uk.toml` next to the manifest, reopen the
window (the tray reads its own copy when PolterType next starts).

Four things worth knowing:

* **A plug-in's catalog is confined to its own pane.** Whatever the file
  says, every key lands under `plugin.<id>.`; a plug-in cannot reword
  PolterType's own buttons. Writing the prefix out yourself is allowed
  and changes nothing.
* **It works for a language PolterType has never been translated into.**
  The catalogs are independent: pick `pl` — the picker lists it because
  the plug-in's `pl.toml` is on disk — and that plug-in is in Polish
  while the window around it stays English. Nothing has to be added to
  the app first.
* **Values are never translated.** An option's `value`, a control's key
  and a command's id are what reach the plug-in's config file and its
  program; only what is *read* changes. The drop-down shows the label
  and writes the value.
* **What the plug-in prints, only the plug-in can translate.** Report
  text and the rows of a list are produced at run time, so PolterType
  hands every plug-in process `POLTERTYPE_LOCALE` (`uk`, `pt_BR`, …) and
  the plug-in decides what to do with it.

A *data pack* — `kind = "pack"`, no program — is the other way round: its
`i18n/` is a translation of **PolterType itself**, taken as written, so
a whole language can be distributed as a pack.

The tray menu is translated too, a plug-in's own entries included: the
tray reads the same catalogs at startup, so a change of language takes
effect there when PolterType is next started.

The suggestion popup's own action — "Add to dictionary" — comes from
the catalog as well, so nothing the engine puts on screen while you
type is left in English.

Three things stay English on purpose. **Error notifications** carry the
operating system's own message, which is not ours to translate. The
**Setup pane's step titles** come from the permission probe in
`poltertype-input`, a crate with no dependency on the translation
loader; everything the Settings window puts around them — the headline,
the state badges, the buttons and the notes — is translated. And the
**plug-in supervisor's failures** ("declares no command", "did not
answer within 5000ms") name a plug-in's own ids beside an OS error:
they are read in bug reports more often than by the person who
clicked.

---

## Getting it upstream

Open a PR with the one file. Two things make it easy to review:

* **Say which strings you were unsure about.** Several are terms of
  art — "identifier guard", "plausibility", "stop words" — and a
  translator's note is more useful than a confident wrong guess.
* **Keep the section comments** from `uk.toml`. They group the file by
  pane, which is how the next person will read it.

If a string reads awkwardly because the English is awkward, say so.
Fixing the English is usually the better patch, and it improves every
other language at the same time.

---

## Adding a string as a developer

Wrap it at the call site:

```rust
Text::new(tr("general.behaviour", "Behaviour"))
```

Key convention is `<pane>.<slug of the English>`. Pass the English
text as the second argument — that is what makes a missing catalog
harmless, and it keeps the source readable without a lookup table.

For interpolated text, `format!` cannot be used (it needs a literal),
so use the positional form:

```rust
tr_args(
    "languages.status_restricted",
    "Restricted to {} layout(s).",
    &[&count.to_string()],
)
```

Then add the key to `data/i18n/uk.toml` — or leave it, and the next
translator will pick it up. `cargo test -p poltertype-core i18n`
checks that the shipped catalog still parses and still covers one
label per pane.
