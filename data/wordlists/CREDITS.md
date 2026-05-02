# Wordlist sources & licensing

The dictionaries shipped in this directory power the
`DictionaryDetector` in `kb-detect`. Each source has a different
license; we list them here so contributors and downstream packagers
can audit the project's overall license posture.

The kb-switcher binary is MIT-licensed — embedding these wordlists
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
  after embedding; consuming code (the kb-switcher engine) remains
  MIT.
* **Processing:** stripped Hunspell affix flags (`/...`), dropped
  `+cs=...` case-sensitive metadata, lowercased, kept only entries
  composed of Ukrainian letters + apostrophe + hyphen, sorted,
  de-duplicated.
* **Size:** ~333k entries.

## Refreshing

Use the `xtask wordlists fetch` command to re-download and re-process
both sources. The script verifies the upstream URLs return 200 and
applies the same processing rules described above.

## User overrides

The runtime supports drop-in user wordlists at
`<config-dir>/kb-switcher/wordlists/<layout-id>.txt`. The format is
one lowercase word per line, blank lines / `#`-comments ignored.
Entries from the user file are merged into the embedded set at
startup — useful for adding domain-specific vocabulary (proper
nouns, project terms, slang) without rebuilding the binary.
