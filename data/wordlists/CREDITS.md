# Wordlist sources & licensing

The dictionaries shipped in this directory power the
`DictionaryDetector` in `poltertype-detect`. Each source has a different
license; we list them here so contributors and downstream packagers
can audit the project's overall license posture.

The PolterType binary is MIT-licensed, and stays MIT: every source
here permits verbatim redistribution, and each licence attaches to
**the wordlist data**, not to the code that reads it. What differs
between sources is what redistributing *the bundle* obliges you to do
— which ranges from "nothing" (Public Domain, BSD, MPL) to
AGPL-3.0-or-later for Hebrew. The table below is the per-language
detail; read it before shipping a build to anyone else.

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

## The other LibreOffice-derived lists

Everything below comes from
[LibreOffice/dictionaries](https://github.com/LibreOffice/dictionaries)
through one pipeline: download `.dic` + `.aff`, expand the affix rules
into surface forms (`xtask/src/hunspell/`), lowercase, keep entries
made of letters plus apostrophe and hyphen, sort, de-duplicate, gzip.
The exact URLs are in `xtask/src/consts.rs::HUNSPELL_SOURCES`; re-run
them with `cargo xtask wordlists fetch`.

**Every licence in this table attaches to the wordlist data only.**
The PolterType engine stays MIT. What the licence constrains is
redistribution of a *built binary that embeds these lists* — read the
row before shipping PolterType anywhere but your own machine. Each
entry was checked against the upstream licence file on 2026-08-01;
the generic "probably GPL, go and look" hedge that stood here before
was wrong about at least Russian, which is BSD.

| Stem | Upstream `.dic` | Licence | Forms |
|---|---|---|---|
| `ru_ru` | `ru_RU/ru_RU.dic` | BSD-style, 4-clause (A. I. Lebedev) | 1.4 M |
| `de_de` | `de/de_DE_frami.dic` | GPL-2 or GPL-3 | 789 k |
| `es_es` | `es/es_ES.dic` | GPL-3+ / LGPL-3+ / MPL-1.1+ (disjoint; pick one) | 652 k |
| `fr_fr` | `fr_FR/dictionaries/fr.dic` | MPL-2.0 (Dicollecte / Grammalecte) | 2.2 M |
| `pl_pl` | `pl_PL/pl_PL.dic` | GPL / LGPL / MPL / Apache-2.0 / CC-SA (pick one) | 4.0 M |
| `cs_cz` | `cs_CZ/cs_CZ.dic` | GPL, with GNU FDL 1.1 portions | 3.1 M |
| `el_gr` | `el_GR/el_GR.dic` | MPL-1.1 / GPL-2.0 / LGPL-2.1 (pick one) | 827 k |
| `he_il` | `he_IL/he_IL.dic` | **AGPL-3.0-or-later** (Hspell) — see below | 469 k |
| `tr_tr` | `tr_TR/tr_TR.dic` | MPL-2.0 | 5.8 M |
| `bg_bg` | `bg_BG/bg_BG.dic` | GPL-2 | 2.1 M |
| `it_it` | `it_IT/it_IT.dic` | GPL-3 | 3.3 M |
| `pt_pt` | `pt_PT/pt_PT.dic` | GPL-2-or-later | 1.3 M |
| `pt_br` | `pt_BR/pt_BR.dic` | LGPL-3 / MPL | 8.1 M |

### Hebrew is the row to look at twice

Hspell is **AGPL-3.0-or-later** — the strictest licence in this tree
and the only one with a network clause. It still attaches to the data
rather than to the engine, and PolterType builds no network service
out of it, so bundling it in a locally-installed desktop app is
within the terms. But it is a materially heavier obligation than
anything else here, and a downstream packager or a company shipping a
modified build needs to know it is present.

If it ever becomes awkward, `he_il` is the cheapest language in the
set to drop: nothing else bundled uses the Hebrew script, so the
plausibility detector separates he-IL from every other layout on
script alone and the dictionary is a refinement, not the load-bearing
signal.

### Hebrew also ships stems, not surface forms

`he_IL.aff` is not an inflection table — it is 3335 **prefix** rules
and zero suffix rules, encoding the clitic particles (ב ל כ מ ש ו ה,
and the legal pairs of them) as affixes. Expanding it faithfully
produces 60.6 M forms: a 141 MB `.txt.gz` here and a far larger FST
inside every installer. So `he_il` alone is processed with
`ExpandMode::StemsOnly` (`xtask/src/enums.rs`) and ships its 469 k
stems. A Hebrew word typed with a clitic prefix therefore misses the
dictionary and falls through to plausibility — which, for a script
nothing else here shares, is enough.

### Polish carries the wordlist on its own

`pl_pl.toml` maps to exactly the same characters as `en_us.toml`: the
standard Polish "programmer's" layout is US QWERTY with the diacritics
on AltGr, which PolterType does not track. So there is no pl-PL ↔
en-US correction to make, and the Polish wordlist exists for the other
direction — stopping Polish prose from being dragged toward whichever
other layout the user has active. See the header of
`data/layout-mappings/pl_pl.toml`.

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
