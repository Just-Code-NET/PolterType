# Adding a language to Poltertype

`poltertype` is data-driven: each supported keyboard layout is one
TOML file, and each language's dictionary is a plain wordlist. There
are two paths to add a language, depending on whether you want the
addition baked into the binary or loaded at runtime from your config
directory.

> **TL;DR** — for a one-off custom layout on your own machine, drop
> a `*.toml` into `<config-dir>/poltertype/layouts/` and a matching
> wordlist into `<config-dir>/poltertype/wordlists/`. To upstream
> a language, edit the bundled files and submit a PR.

---

## 1. Layouts (where each scancode maps to which character)

A layout TOML answers one question per row:
*"when the user presses physical key X under this layout, which
character does the OS produce?"*

The file lives at `data/layout-mappings/<stem>.toml` (bundled) or
`<config-dir>/poltertype/layouts/<stem>.toml` (user). The schema
is identical.

### Schema

```toml
id     = "pl-PL"          # BCP-47-style; unique
name   = "Polski"         # display label (optional)
script = "Latin"          # Latin / Cyrillic / Greek / Armenian / Hebrew / Arabic / Other

[keys]
# Win SC Set-1 scancodes → produced character.
# `plain` = unshifted, `shift` = shifted variant (optional).
0x10 = { plain = "q", shift = "Q" }
0x11 = { plain = "w", shift = "W" }
# ... fill in alphanumeric and punctuation rows
```

### Which scancodes matter

Cover at least the alphanumeric block plus the punctuation that
serves as a word boundary in your language. The bundled files
(`en_us.toml`, `uk_ua.toml`, `ru_ru.toml`, `de_de.toml`, `es_es.toml`,
`fr_fr.toml`) are good copy-paste templates. The detector ignores
scancodes you don't list, so omitting `0x29` (backtick) for a layout
that doesn't use it is fine.

### Dead keys

Poltertype tracks *the character produced per keystroke*, not the
OS-level dead-key state machine. Surface the spacing equivalent
(`´`, `^`, `¨`) — that matches what shows up in a mid-correction
buffer the rare time the user actually presses a dead key alone.

### Test it

Add a TOML, restart Poltertype, and watch the log:

```
loaded user layout layout=pl-PL keys=46 dict=false stem=pl_pl
```

If `keys=` is too low, you missed entries. If the layout doesn't
appear, look for a parse error in the log.

---

## 2. Wordlists (so the dictionary detector recognises the language)

Without a wordlist, Poltertype falls back to the plausibility
detector — which is decent for distinctive scripts (Cyrillic vs
Latin) but unreliable inside a single script (German vs English).
A real wordlist makes detection trustworthy.

Three filenames per layout, all under either `data/wordlists/`
(bundled) or `<config-dir>/poltertype/wordlists/` (user):

| File | Goes into | Used for |
|---|---|---|
| `<stem>.txt` | `user_overlay` (full-length lookup) | The main dictionary. One lowercase word per line. |
| `<stem>-extras.txt` | same as above | Optional second file for organisation (tech vocab, surnames, …). Merged with `<stem>.txt`. |
| `<stem>-stop.txt` | `short_stop_words` (≤2-letter lookup) | Hand-curated 1- and 2-letter words. **Required** for the bundled path (`include_str!`'d at compile time); optional for user-side. |

### Format

```
# blank lines and `#` comments ignored
# one lowercase word per line
hello
world
function
```

### Sourcing

Bundled languages source their wordlists from public projects with
non-restrictive licences — see `data/wordlists/CREDITS.md`. The
`cargo xtask wordlists fetch` command re-downloads them.

For a user-side language, anything works: a Hunspell stems file
stripped of affix flags, an `aspell dump master` output, a frequency
list — as long as it's one lowercase word per line.

### Wiring a new bundled language

To bundle a new language so users get it without touching their
config directory:

1. Drop the layout TOML into `data/layout-mappings/<stem>.toml`.
2. Drop the wordlists into `data/wordlists/`:
   * `<stem>.txt` — large dictionary
   * `<stem>-extras.txt` — your tech-vocab extras (optional)
   * `<stem>-stop.txt` — short stop words (always commit, even if
     near-empty — `include_str!` requires the file to exist)
3. Add the stem to `crates/poltertype-core/build.rs::LAYOUTS`:

   ```rust
   const LAYOUTS: &[(&str, &str)] = &[
       ("en_us", "en-US"),
       // ...
       ("pl_pl", "pl-PL"),
   ];
   ```

4. Add the matching triple to
   `crates/poltertype-core/src/layouts/db.rs::embedded_layouts()`:

   ```rust
   (
       "pl_pl.toml",
       include_str!("../../../data/layout-mappings/pl_pl.toml"),
       "pl_pl",
   ),
   ```

5. Optional but recommended: extend `derive_vowels` with the
   language's vowel set (especially if it uses accented vowels —
   the plausibility detector counts vowel ratio, and unrecognised
   vowels score as consonants).
6. `cargo build` and `cargo test --workspace`.

The build script crashes loudly if (3) and (4) drift apart, so
you'll catch a half-finished wiring on the first build.

---

## 3. User-side, no rebuild

If you want to add a language without touching the source tree
(e.g. you're not a Rust dev, or you just want a one-off custom
mapping for your odd keyboard):

1. Open the tray menu → **Open User Layouts Folder…** to ensure
   `<config-dir>/poltertype/layouts/` exists.
2. Drop `<stem>.toml` in there (schema as above).
3. Open the tray menu → **Open User Wordlists Folder…** for
   `<config-dir>/poltertype/wordlists/`.
4. Drop the wordlists there.
5. Restart Poltertype (the engine snapshots the layout database at
   start; new layouts need a fresh boot to populate the scancode-
   translation tables). Adding *words* to an already-loaded language
   only needs **Reload Settings** from the tray menu.
6. Optionally edit `config.toml` and add your new id to
   `[languages].active`.

Same TOML, same wordlist format. If your `id` matches a bundled
layout, your file wins — handy if you disagree with the bundled
mapping for your physical keyboard.

---

## 4. Plausibility-only languages

Adding *just* a layout TOML without any wordlist is a perfectly
valid lightweight mode: the dictionary detector silently abstains,
and the plausibility detector picks up via vowel-ratio /
consonant-cluster / script-fit signals. This works well for layouts
whose script differs from any other active layout (a Greek user
with Greek + English, say) — script alone is a strong signal. For
two layouts in the same script, you'll want a real wordlist.
