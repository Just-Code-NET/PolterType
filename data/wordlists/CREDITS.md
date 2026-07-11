# Wordlist sources & licensing

The dictionaries shipped in this directory power the
`DictionaryDetector` in `poltertype-detect`. Each source has a different
license; we list them here so contributors and downstream packagers
can audit the project's overall license posture.

The PolterType binary is MIT-licensed — embedding these wordlists
is permissible because every source allows verbatim redistribution
under non-restrictive terms (MIT-compatible) or under terms that
attach only to the data, not to consuming code (MPL).

## en_us.txt

* **Source:** [`dwyl/english-words`](https://github.com/dwyl/english-words),
  file `words_alpha.txt`.
* **License:** Public Domain / Unlicense — see the source repo's
  `LICENSE.md`.
* **Processing:** dropped blank lines and non-`[a-z]+` entries,
  sorted, de-duplicated.
* **Size:** ~370k entries.

## uk_ua.txt

* **Source:** [LibreOffice/dictionaries](https://github.com/LibreOffice/dictionaries),
  file `uk_UA/uk_UA.dic`. Itself derived from
  [brown-uk/dict_uk](https://github.com/brown-uk/dict_uk).
* **License:** **MPL 1.1** (Mozilla Public License). Authors:
  Andriy Rysin et al., 2007–. The dictionary stays under MPL even
  after embedding; consuming code (the PolterType engine) remains
  MIT.
* **Processing:** stripped Hunspell affix flags (`/...`), dropped
  `+cs=...` case-sensitive metadata, lowercased, kept only entries
  composed of Ukrainian letters + apostrophe + hyphen, sorted,
  de-duplicated.
* **Size:** ~333k entries.

## ru_ru.txt, de_de.txt, es_es.txt, fr_fr.txt

* **Source:** [LibreOffice/dictionaries](https://github.com/LibreOffice/dictionaries):
  * `ru_RU/ru_RU.dic`
  * `de/de_DE_frami.dic`
  * `es/es_ES.dic`
  * `fr_FR/fr.dic`
* **License:** Each upstream dictionary ships its own licence. As of
  2026 LibreOffice's bundled Hunspell dictionaries are most often
  GPL-2-or-later or LGPL/MPL — review the per-language `README*.txt`
  next to the `.dic` for the exact terms before redistributing a
  built PolterType binary outside personal use. The licence applies
  to **the wordlist data** only; the PolterType engine remains MIT.
* **Processing:** the same generic Hunspell pipeline as `uk_ua.txt`
  (strip affix flags, lowercase, keep letter-only stems).
* **Size:** varies — typically 100–300k stems each, expanded to
  millions of inflected forms by Hunspell at runtime, but we keep
  only the stems for FST size.
* **Status (2026-05-06):** layout TOML and short-stop word list
  shipped; the bulk dictionaries are populated by `cargo xtask
  wordlists fetch`. Until run, the FST is empty and detection falls
  back to plausibility scoring (vowel ratio + script fit) — which
  works well for cross-script pairs (RU vs EN) and adequately for
  same-script pairs (DE vs EN, …) on prose, less so on short tokens.

## Refreshing

Use the `cargo xtask wordlists fetch` command to re-download and
re-process every source. The script verifies the upstream URLs return
200 and applies the same processing rules described above.

## User overrides

The runtime supports drop-in user wordlists at
`<config-dir>/poltertype/wordlists/<layout-id>.txt`. The format is
one lowercase word per line, blank lines / `#`-comments ignored.
Entries from the user file are merged into the embedded set at
startup — useful for adding domain-specific vocabulary (proper
nouns, project terms, slang) without rebuilding the binary.
