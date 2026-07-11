# Cutting a release

> Step-by-step checklist for bumping the version and shipping a
> tagged release. Once you push a `v*` tag, the GitHub Actions
> workflow at [`.github/workflows/release.yml`][release-yml]
> builds the three platform installers in parallel and attaches
> them to a draft release.

The whole flow takes ~5 minutes of local work plus ~15 minutes of
CI time. Most steps are mechanical — the goal of this doc is to
make sure nothing gets skipped, since a misnumbered tag or a
forgotten changelog entry means re-cutting the release.

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

## 2. Pick the next version

Poltertype follows [SemVer](https://semver.org/). The pre-release
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

## 3. Bump the version (~30 seconds with the helper)

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
  single edit propagates to all 7 crates.

* `CHANGELOG.md` heading at the top:
  ```markdown
  ## [Unreleased] — 0.2.1
  ```

* `Cargo.lock` — never edit by hand; just run
  `cargo check --workspace` after the other two changes.

## 4. Verify the bump (~1 min)

Three quick checks before you commit:

```bash
# All workspace crates show the new version (should print 7
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

## 5. Commit + tag + push (~1 min)

The release commit should contain only the three files above.
No code, no doc fixes, no formatting drive-bys. If you forgot
something, do it as a separate commit FIRST and then bump.

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "release: v0.2.1"
git tag v0.2.1
git push origin main --tags
```

`git push --tags` triggers the release workflow on the tag.
The branch push (`main`) is what makes the version-bump commit
visible to anyone tracking `main`.

## 6. After the tag (~15 min, mostly waiting)

GitHub Actions does the rest. Watch the run:

```bash
gh run watch                        # if you have gh CLI
# or open https://github.com/<owner>/<repo>/actions
```

The workflow:

1. Builds three installers in parallel — Windows `.msi` (WiX
   3.x), macOS universal `.dmg` (Intel + Apple Silicon merged
   with `lipo`), Linux `.AppImage` (x86_64, `linuxdeploy`).
2. Creates a **draft** release on GitHub with the three
   installers attached and SHA-256 checksums in the body.
3. Stops there — you publish the release manually after sanity-
   checking the artefacts.

When the run completes:

* Open the draft on the [Releases page][releases].
* Edit the body if you want to add release notes (the auto-
  generated body is just the checksums).
* Hit **Publish release**.

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

## 7. Common mistakes and how to recover

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

# 2. Make sure CHANGELOG has a `[Unreleased]` block describing
#    what's new since the last release. The xtask updates the
#    heading version, but doesn't write the body for you.

# 3. Bump (writes Cargo.toml, CHANGELOG.md heading, Cargo.lock).
cargo xtask version bump

# 4. Eyeball the diff, then commit + tag + push. The bump command
#    prints the exact sequence as a copy-paste hint.
NEW=$(cargo xtask version)
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "release: v$NEW"
git tag "v$NEW"
git push origin HEAD --tags

# 5. Watch CI, publish the draft.
gh run watch
```

[release-yml]: ../.github/workflows/release.yml
[releases]: https://github.com/Just-Code-NET/poltertype/releases
[keep-a-changelog]: https://keepachangelog.com/en/1.1.0/
