# Cutting a release

> Step-by-step checklist for bumping the version and shipping a
> tagged release. Once you push a `v*` tag, the GitHub Actions
> workflow at [`.github/workflows/release.yml`][release-yml]
> builds the four platform installers in parallel and attaches
> them to a draft release.

The whole flow takes ~15 minutes of local work plus ~15 minutes of
CI time. Most steps are mechanical — the goal of this doc is to
make sure nothing gets skipped, since a misnumbered tag or a
forgotten changelog entry means re-cutting the release.

**The one step people skip is step 2, syncing the docs.** It is
the only step whose omission nothing detects: the tag is fine, CI
is green, the installers work, and the documentation quietly
drifts out of step with the code. Treat it as a blocker, not a
chore.

## 1. Pre-flight (~2 min)

Run these before touching `Cargo.toml`. Each one catches a
distinct way the release could be broken:

```bash
# 1. Working tree is clean — no half-finished edits about to
#    sneak into the release commit.
git status

# 2. You're on `main` (or whichever branch you ship from) and
#    up-to-date with origin.
git status -b
git pull --ff-only

# 3. The full test + lint sweep passes locally. CI runs this
#    too, but catching it now saves a 15-minute round-trip.
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

If any of those fail, fix them in a normal commit BEFORE the
release commit. The release commit itself should only contain
the version bump + changelog entry — nothing else.

## 2. Sync the docs — MANDATORY (~10 min)

**No tag ships while the docs still describe the previous
release.** This step is not a nicety and it is not "later" — it is
a release blocker, exactly like a failing test. Do it now, in a
normal commit, *before* the version bump.

Documentation rot is silent. Nothing fails, CI stays green, and
the app keeps working — so the only thing that catches it is a
human reading a doc that no longer matches the code. By then it
has usually been wrong for several releases.

### The rule

Every release, walk this table. For each row, either update the
file or convince yourself it genuinely didn't change.

| Doc | Goes stale when… |
|---|---|
| `README.md` | anything user-facing changes — **especially the Status line, the Goals bullets, the install table, and the hotkey table** |
| `README.<lang>.md` | the translations carry their own copy of the install table and the caveats, so **anything that changes a row of the English one changes theirs**. They hold no version numbers by design, which bounds this to structural changes: an installer added or renamed, a permission step altered, a caveat that stops being true. A translation nobody speaks is the easiest doc to leave rotting — if there is no one to update it, delete it rather than ship a lie in a language we cannot read |
| the maintainer's known-gaps list | kept out of tree, but it is a release blocker all the same: re-stamp its version heading and re-verify every bullet; a gap that closed must come out, and whatever it says must match `README.md` |
| `docs/PLAN.md` | the `Last updated:` line, the phase checkboxes, and the settings schema in §3.5 (a new `[section]` in `config.toml` belongs there) |
| `docs/DECISIONS.md` | you made a call worth defending later — append an entry; don't rewrite history |
| `docs/CODE_SIGNING.md` | a signing key is added, rotated or retired, or a platform's signing status changes — it is a **published** policy and a stale one misleads users, not just us |
| `docs/DATA_LAYOUT.md` | the app writes a new file or directory on the user's disk |
| `docs/PERMISSIONS.md` | the app takes a new OS capability — **including network** |
| `docs/AI.md` | the AI subsystem's wiring status changes |
| `CONTRIBUTING.md` | a crate is added or removed, or the build/check commands change |
| `poltertype-web` | any user-visible claim changes. **Separate repo, separate commit** — but the site may only promise what the app actually does |

### Watch the compound claims

A stale version number is embarrassing. A stale statement about
**what the app can do** is worse, because a reader has no way to
tell it from a true one.

The dangerous shape is the *compound* claim — a sentence that
bundles several guarantees together. Half of it can stop being
true while the other half stays true forever, and the true half
keeps the sentence looking healthy. Nobody re-reads a bullet they
already believe.

If a release changes what the app *can do*, grep for every place
the old wording lives and fix each one:

```bash
# Does any doc still describe a capability the code has moved past?
grep -rn "no network\|no telemetry\|never.*network\|no build makes" \
    README.md CONTRIBUTING.md SECURITY.md docs/*.md
```

> **A worked example — and read it precisely, because the precise
> version is the lesson.** v0.4.0 added the updater: the first
> network call the app had ever made, on by default. The code was
> careful, `DECISIONS.md` was thorough, and **nothing about the
> app's privacy posture changed** — the updater fetches a manifest
> from a public repo, sends no body, no query string and no
> identifier, and PolterType collects no telemetry today for
> exactly the same reason it collected none in v0.1: we don't, and
> we won't.
>
> What went stale was narrower, and purely factual. `README.md`
> advertised "**no telemetry, no network**". The first half was
> still true. The second half was not, from the moment the tag
> was pushed — and it stayed in the README for three releases,
> forty lines above a correct description of the very updater it
> contradicted. `AI.md` had the same problem in a different shape:
> "no shipped build makes a network call" was written to mean *the
> AI subsystem opens no socket*, which is still true, but as
> phrased it was a claim about the whole binary, which was not.
>
> Neither was a broken promise to users. Both were the docs
> falling behind the code — which is the only kind of rot this
> step exists to catch, and it is quite enough. Say what the app
> does, keep saying it accurately, and don't let a true clause
> smuggle a stale one past you.

### Verify before moving on

```bash
# 1. Nothing still claims the last version is current.
#    (Replace 0.4.2 with the version you are ABOUT to leave behind.)
grep -rn "0\.4\.2\|v0\.4\.2" README.md docs/*.md CONTRIBUTING.md

# 2. Every crate in the workspace appears in the docs that list crates.
ls crates/                                   # compare against
grep -n "poltertype-" CONTRIBUTING.md | grep -c crates

# 3. Skim the diff of the release you are cutting, and ask of each
#    changed file: "does any doc describe this behaviour?"
git diff --stat "$(git describe --tags --abbrev=0)"..HEAD
```

Doc fixes go in their **own commit**, before the release commit
— `docs: refresh for <what changed>`. The release commit stays
clean (Cargo.toml + Cargo.lock + CHANGELOG.md, nothing else).

## 3. Pick the next version

PolterType follows [SemVer](https://semver.org/). The pre-release
phase is over — `v0.1.0` was the first stable tag and the current
line is `0.x` stable. The shape of the next version depends on
what's queued:

| What's in `[Unreleased]` since the last tag | Next tag |
|---|---|
| Bug fixes only | patch — `v0.2.1` |
| User-facing additions, backwards-compatible | minor — `v0.3.0` |
| Breaking change to config schema / data layout | minor while pre-1.0 — `v0.3.0`, and say so loudly in the changelog |
| API/behaviour considered settled | `v1.0.0` (a deliberate call, not a mechanical bump) |

Check what tag was last shipped:

```bash
git tag --sort=-v:refname | head -5
```

The next tag is derived from that one. If you see `v0.2.0` as the
latest and you're shipping fixes, the next is `v0.2.1`.

> **Don't reuse a tag.** If you accidentally release the wrong
> code under a tag, cut a NEW patch version rather than retagging.
> Retagging breaks anyone who already downloaded the broken
> installer — they'll silently keep using the old binaries with
> the new version number, which is the worst kind of bug.

## 4. Bump the version (~30 seconds with the helper)

The fastest way is the dedicated xtask command, which updates the
three lock-step files (`Cargo.toml`, `CHANGELOG.md`, `Cargo.lock`)
in one go:

```bash
# Auto-bump: patch when there's no pre-release suffix, which is the
# normal case now (0.2.0 → 0.2.1). On a pre-release it would bump the
# trailing counter instead (0.1.0-beta.5 → 0.1.0-beta.6).
cargo xtask version bump

# Or set an exact version — needed for every minor/major jump, since
# `bump` only ever moves the patch digit.
cargo xtask version set 0.3.0

# Preview without writing files (works on either subcommand):
cargo xtask version bump --dry-run

# Just print the current version:
cargo xtask version
```

The command:

1. Reads the current version from `Cargo.toml`.
2. Computes the next version (or uses the `set` argument).
3. Validates the shape is a recognised pre-release / stable form
   — typos in `set` fail loudly here, not after the tag has hit CI.
4. Rewrites `Cargo.toml` (only the `[workspace.package].version`
   line — dep-pin `version = "..."` entries elsewhere in the file
   are left alone).
5. Rewrites the `## [Unreleased] — <ver>` heading in
   `CHANGELOG.md` if present (logged + skipped if missing — see
   "Manual edit" below for that case).
6. Runs `cargo check --workspace` to refresh `Cargo.lock`.
7. Prints the suggested `git add`/`commit`/`tag` sequence so you
   can copy-paste it.

If the file shapes ever drift and the command can't find what to
edit, the command refuses (with a clear error) rather than guess.
You then either fix the file shape or edit by hand using the
"Manual edit" section below.

### Auto-bump rule

The `bump` subcommand applies one rule:

* **No pre-release** (the current situation) → increment patch.
  `0.2.0` becomes `0.2.1`.
* **Pre-release present** (`-alpha.N`, `-beta.N`, `-rc.N`, …) →
  increment the trailing counter. `0.1.0-beta.5` becomes
  `0.1.0-beta.6`.

Anything else — a minor or major bump, or going back into a
pre-release — is not auto-detectable from the current version alone,
so use `set`. Since `bump` only moves the patch digit, **every
feature release needs `set`**:

```bash
# Feature release.
cargo xtask version set 0.3.0

# Cut a release candidate ahead of a big one.
cargo xtask version set 0.3.0-rc.1

# Call it done.
cargo xtask version set 1.0.0
```

### Make sure CHANGELOG actually has the entry

The xtask only updates the version inside the heading
`## [Unreleased] — <ver>`. **It doesn't write the changelog body
for you** — that's the user's job, and skipping it is the most
common release mistake.

Before the bump command, your CHANGELOG should look like:

```markdown
# Changelog

## [Unreleased] — 0.2.0    ← old version

### Smart commands — text-trigger expansions
…detailed notes about what changed since the last release…
```

The heading just labels which version that block belongs to.
The format we follow is loose [Keep a Changelog][keep-a-changelog]:

* **Added** — new features.
* **Changed** — behaviour changes that aren't bugs.
* **Fixed** — bug fixes.
* **Removed** — things that went away.

You don't need rigid sections — the existing entries are mostly
long-form narrative. The important thing is the user can read
the entry and understand what changed.

### Manual edit (fallback)

If the xtask refuses or you want to do it by hand:

* `Cargo.toml` line ~15:
  ```toml
  [workspace.package]
  version       = "0.2.1"   # ← bump this
  ```
  Every workspace crate reads `version.workspace = true`, so this
  single edit propagates to all 8 crates.

* `CHANGELOG.md` heading at the top:
  ```markdown
  ## [Unreleased] — 0.2.1
  ```

* `Cargo.lock` — never edit by hand; just run
  `cargo check --workspace` after the other two changes.

## 5. Verify the bump (~1 min)

Three quick checks before you commit:

```bash
# All workspace crates show the new version (should print 8
# lines, all matching).
grep -E '^name = "poltertype' Cargo.lock -A 1 | grep '^version'

# Cargo.toml says the new version.
grep '^version' Cargo.toml

# CHANGELOG heading matches.
grep '^## \[' CHANGELOG.md | head -1
```

If any of those three disagree, the release will look
inconsistent (e.g. CI installer named `v0.2.1` but
`poltertype --version` reports `0.2.0` because Cargo.toml
wasn't bumped). Fix before committing.

## 6. Commit + tag + push (~1 min)

The release commit should contain only the three files above.
No code, no doc fixes, no formatting drive-bys. If you forgot
something, do it as a separate commit FIRST and then bump —
that includes the doc sync from step 2, which by now should
already be pushed.

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "release: v0.2.1"
git tag v0.2.1
git push origin main --tags
```

`git push --tags` triggers the release workflow on the tag.
The branch push (`main`) is what makes the version-bump commit
visible to anyone tracking `main`.

## 7. After the tag (~15 min, mostly waiting)

GitHub Actions does the rest. Watch the run:

```bash
gh run watch                        # if you have gh CLI
# or open https://github.com/<owner>/<repo>/actions
```

The workflow:

1. Builds four installers in parallel — Windows `.msi` (WiX
   3.x), macOS universal `.dmg` (Intel + Apple Silicon merged
   with `lipo`), Linux `.AppImage` for x86_64 and aarch64
   (`linuxdeploy`, each built natively on a runner of its own
   architecture).
2. Writes `latest.json` — the manifest the **in-app updater**
   polls (version, release-notes URL, and a URL + SHA-256 + size
   per platform). Generated from the exact artefacts being
   uploaded, so the checksums cannot drift out of step with the
   files. A missing artefact fails the job rather than shipping a
   manifest with a hole in it.
3. Creates a **draft** release on GitHub with the three
   installers plus `latest.json` attached, and SHA-256 checksums
   in the body.
4. Stops there — you publish the release manually after sanity-
   checking the artefacts.

### Sign the manifest — MANDATORY (~1 min)

The manifest CI attached is **unsigned**. The updater checks an
ed25519 signature over it against a public key compiled into the
app; that signature is what a compromised GitHub account cannot
forge, which is only true because the private key is not an
Actions secret and never touches a runner. So this step is
yours, and it happens on the draft, before publishing:

**Run these from the repository root**, not from `/tmp`. Both tools
need it: `gh` infers the repository from the git remote, and
`cargo xtask` is an alias defined in this repo's `.cargo/config.toml`
— outside it, cargo reports `no such command: xtask`. The manifest
itself goes to `/tmp` so it can never be committed by accident.

```bash
cd /path/to/poltertype
gh release download v0.2.1 --pattern latest.json --dir /tmp --clobber
cargo xtask manifest sign /tmp/latest.json --key ~/.config/poltertype-signing/release.key
gh release upload v0.2.1 /tmp/latest.json --clobber
```

`sign` re-reads what it wrote and verifies it against the public
key the app ships, so a mismatched key fails here rather than on
a user's machine. To look before you sign,
`cargo xtask manifest payload /tmp/latest.json` prints the exact
bytes the signature covers, and `cargo xtask manifest verify
/tmp/latest.json` re-checks an already-signed file.

The workflow run's summary page carries these same commands, so
you don't have to remember they exist.

**Key custody.** The private key is a 32-byte seed, base64, in
`~/.config/poltertype-signing/release.key` (mode 0600) — or
wherever you point `--key`, or in `$POLTERTYPE_SIGNING_KEY` if you
keep it in a password manager instead of on disk. It is the one
secret in this project with no recovery path: losing it means
shipping a release with a new public key baked in and asking
users to accept an unsigned interim manifest, and leaking it
means an attacker can sign updates. Back it up somewhere that is
neither this repository nor GitHub.

`cargo xtask manifest keygen` creates a fresh keypair (and
refuses to overwrite an existing one). Rotating means: keygen →
put the new public key in
`crates/poltertype-update/release-signing-key.pub` → ship a
release built with it → only then sign with the new private key.
Signing before that release is out means every existing install
sees a signature it cannot check.

> **Signatures are not yet mandatory.**
> `poltertype-update`'s `REQUIRE_SIGNATURE` is `false`, so an
> unsigned manifest still works — that is deliberate, since users
> on older builds would otherwise be stranded. It is also why
> forgetting to sign fails silently. Flip the constant to `true`
> only once a signed manifest has been the published `latest.json`
> for a full release cycle, and note the flip in the changelog:
> from then on, a forgotten signature is an outage for everyone's
> updater.

When the run completes:

* Open the draft on the [Releases page][releases].
* Sign and re-upload `latest.json` (above).
* Edit the body if you want to add release notes (the auto-
  generated body is just the checksums).
* Hit **Publish release**.

> **Publishing is what ships the update to every existing user.**
> This is new as of 0.4.0 and worth internalising: the updater
> fetches `latest.json` through GitHub's
> `releases/latest/download/` redirector, which resolves only to
> **published, non-prerelease** releases. So a draft is invisible
> to users — but the moment you hit Publish, every installed copy
> of PolterType will (within its check interval, 24 h by default)
> download this build and install it on the user's next restart.
>
> Two practical consequences:
>
> * **Sanity-check the artefacts *before* publishing, not after.**
>   Un-publishing does not un-install.
> * **Mark a release as a pre-release if it isn't for everyone.**
>   GitHub's `latest` redirector skips pre-releases, so an rc can
>   be published for testers without pushing it at the whole user
>   base.

If a job fails: the workflow file is at
[`.github/workflows/release.yml`][release-yml]. Most failures
are platform-specific (a brittle WiX rule, a missing macOS dep)
— fix in a normal commit + push. The tag stays; CI doesn't
re-run on commits, only on tag pushes. To re-trigger, delete the
tag locally and remotely, then re-tag and re-push:

```bash
git tag -d v0.2.1
git push origin :refs/tags/v0.2.1
git tag v0.2.1
git push origin v0.2.1
```

This is the one place we re-tag — it's safe before publish
because the draft hasn't been seen by users yet.

## 8. Common mistakes and how to recover

**Shipped with stale docs.** The most common one, and the only
one on this list that nothing will ever warn you about. There is
no clean recovery and no need for one: fix the docs in a normal
`docs:` commit and push it — don't re-tag, don't re-release. The
cost isn't the fix, it's the weeks the wrong thing sat there
being read. Which is the whole reason step 2 exists.

**Forgot to bump CHANGELOG.** Add a follow-up commit with the
changelog entry, push it, but DON'T re-tag — the next release
will pick it up and the missing-from-this-tag entry will
surface in retrospect ("oh, that landed in 0.2.1"). Optionally edit
the GitHub release body to add the missing notes inline.

**Cargo.toml version mismatches the tag.** If the tag is
`v0.2.1` but `Cargo.toml` says `0.2.0`, the
Windows MSI's `poltertype --version` will print the wrong
number. The installer's filename is correct (it comes from the
tag), but the binary is lying. Fix: revert the broken release
(delete the tag, delete the draft), bump Cargo.toml correctly,
re-tag.

**Tag pushed but installers are wrong / missing files.** Don't
edit the tag in place. Either:

* If the draft hasn't been published yet: delete the tag + draft,
  fix the bug in a normal commit, re-tag with the SAME version
  (it never went out, so reuse is fine).
* If the draft IS published: cut the next patch (`v0.2.2`) instead.
  Anyone who downloaded the broken one already has it — moving them
  forward is the only way to ship a fix without leaving stale
  binaries in the wild.

**You typed the wrong version.** Same as above: if not
published, delete + retry. If published, ship the next number.

## TL;DR — the happy path

```bash
# 1. Pre-flight.
git status && git pull --ff-only
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# 2. SYNC THE DOCS — mandatory, own commit, before the bump.
#    Walk the table in step 2. At minimum: README Status line +
#    Goals bullets, the known-gaps list's version heading,
#    PLAN.md's `Last updated` + settings schema, a
#    DECISIONS.md entry, and the site if any user-facing claim
#    moved. If the release changed what the app CAN do, grep for
#    the old promise everywhere before you tag.
git diff --stat "$(git describe --tags --abbrev=0)"..HEAD   # what changed?
git commit -m "docs: refresh for <what changed>"

# 3. Make sure CHANGELOG has a `[Unreleased]` block describing
#    what's new since the last release. The xtask updates the
#    heading version, but doesn't write the body for you.

# 4. Bump (writes Cargo.toml, CHANGELOG.md heading, Cargo.lock).
cargo xtask version bump

# 5. Eyeball the diff, then commit + tag + push. The bump command
#    prints the exact sequence as a copy-paste hint.
NEW=$(cargo xtask version)
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "release: v$NEW"
git tag "v$NEW"
git push origin HEAD --tags

# 6. Watch CI, publish the draft.
gh run watch
```

[release-yml]: ../.github/workflows/release.yml
[releases]: https://github.com/Just-Code-NET/poltertype/releases
[keep-a-changelog]: https://keepachangelog.com/en/1.1.0/
