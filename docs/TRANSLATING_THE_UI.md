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
3. Set `[general].ui_language` in `config.toml` to your language code
   and reopen the window.

That is the whole loop. To try it without touching the source tree,
drop the file into `<config-dir>/poltertype/i18n/` instead.

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

Files are looked up in `<data_dir>/i18n/` (shipped with the app) and
`<config-dir>/poltertype/i18n/` (yours). Yours wins, which is what
makes the edit-and-reopen loop possible.

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
