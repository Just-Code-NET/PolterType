# Code signing policy

> Who may sign a PolterType release, with which key, and what happens
> to that key. Published because a signing policy that only its authors
> can read is not a policy — and because SignPath Foundation requires
> one from projects it signs for.
>
> Last updated: 2026-08-16 (v0.17.2 — manifest signatures became
> mandatory; nothing else on this page changed).

PolterType asks for an unusual amount of trust: it reads every
keystroke on the machine and can type. A signature is how a user checks
that the binary doing that is the one we built. This document says what
each signature covers and, just as importantly, what it does not.

## What is signed today

| Artifact | Signature | Status |
|---|---|---|
| Release manifest (`latest.json`) | detached ed25519, key on the maintainer's machine | **live since v0.7.0; required since v0.17.2** — an unsigned manifest is refused, not warned about |
| Windows `.msi` | OV certificate (SignPath Foundation HSM) | **applied for** |
| macOS `.app` | ad-hoc (`codesign --sign -`), no identity | **live since v0.14.4** — required for Accessibility to work at all, see below |
| macOS `.dmg` / notarisation | Apple Developer ID | **not held** — no free path, see below |
| Linux `.AppImage` | none | intentional — the manifest signature covers the update path |
| Linux `.deb` / `.rpm` repositories | GPG, key held by us | **when repositories exist** |

Nothing above is aspirational: a row says "live" only if a user can
verify it today.

## Roles

SignPath asks for Author, Reviewer and Approver to be assigned. Being
honest about a small project matters more than inventing a hierarchy:

**PolterType currently has one maintainer, who holds all three roles.**
Concretely, and checkably: exactly one GitHub account has write access
to the repository and the ability to publish a release. Other accounts
in the `Just-Code-NET` organisation hold read access and cannot push,
tag, or publish.

That is a real limitation, not an oversight, and it is the reason the
controls below lean on *mechanism* rather than on separation of duties:

- releases are built by CI from a public repository, not on a
  developer's machine, so the binary is reproducible from a commit
  anyone can read;
- every release is created as a **draft** and requires a deliberate
  human action to publish;
- the manifest signing key is not reachable from CI, so a compromise of
  the CI system cannot produce a signed manifest.

When a second maintainer joins, Reviewer moves to them and this section
is rewritten before the next signed release, not after.

## Accounts and authentication

Every account that can publish a release or approve a signing request
has multi-factor authentication enabled. That covers the GitHub
organisation `Just-Code-NET` and the SignPath organisation once it
exists.

## Keys and where they live

### Update-manifest key (ed25519) — live

Signs `latest.json`, which is what the in-app updater reads before it
trusts any download URL.

- **Private key:** on the maintainer's machine, mode `0600`. It is
  **not** a CI secret and never enters a GitHub Actions runner. This is
  deliberate: the attacker this signature defends against is someone
  who can publish a GitHub release, and that person can read the
  repository's secrets. A key stored there would sign for them too.
- **Public key:** `crates/poltertype-update/release-signing-key.pub`,
  compiled into every binary. Rotating it therefore requires shipping a
  release — which is the point: the binary the user already chose to
  run is what decides which key to trust.
- **Signing happens** on the draft release, before publication, via
  `cargo xtask manifest sign`. See `docs/RELEASING.md` §7.
- **Mandatory since v0.17.2.** `poltertype-update::consts::
  REQUIRE_SIGNATURE` is `true`: a wrong signature is refused and so is
  a missing one. It could only flip once a signed manifest had been the
  published `latest.json` for a full release cycle — every release from
  v0.7.0 to v0.17.1 was signed, so the condition had held eighteen
  times over. Builds older than v0.17.2 carry their own copy of the
  constant, still `false`, and are unaffected.

  **The cost is on us, not on users:** publishing a release and
  forgetting to sign its manifest is now an outage for every updater on
  v0.17.2+, until somebody signs and re-uploads `latest.json`. Signing
  stays manual for the reason above, so `docs/RELEASING.md` §7 is the
  only thing preventing it.

### Windows OV certificate — applied for

Would sign the `.msi`. The certificate is issued to **SignPath
Foundation**, not to us, and the private key stays in their HSM — we
never hold it and cannot export it. Signing is requested by the release
workflow and approved by a human in SignPath.

### macOS ad-hoc signature — live

`installers/macos/build-dmg.sh` runs `codesign --force --sign -` over
the finished `.app`. An ad-hoc signature proves nothing about who built
the app — there is no identity in it — so it is worth being exact about
why it is here: **without it the app does not work on Intel Macs.**

macOS grants Accessibility to a *code identity*, and unsigned code has
none. Until v0.14.4 the universal binary carried a signature on its
arm64 slice only (the linker adds one there, because arm64 macOS will
not run unsigned code) while the cross-compiled x86_64 slice — the one
Intel Macs actually run — had none. The event tap attached and silently
received nothing (issue #28). The build now signs the bundle and
**fails if either slice is unsigned**, since the old failure was
invisible in every artifact we looked at.

Two consequences worth knowing:

- The designated requirement of an ad-hoc signature pins the code
  hash, so **the Accessibility grant does not survive an update** — the
  bytes change, the identity changes, macOS asks again. A Developer ID
  pins the certificate instead and would survive.
- The DMG is not signed. Signing it buys nothing without an identity;
  it becomes a step the day notarisation does.

### macOS Developer ID — not held

There is no free path: Apple Developer Program membership ($99/year) is
required for a Developer ID certificate and for notarisation, with no
open-source exemption. Until it is bought, macOS users get the
right-click → Open route, which the README, the site and the Homebrew
cask all say plainly.

The build script is ready for one: set `SIGN_IDENTITY` to a
`Developer ID Application: …` identity and it signs with that instead,
adding the hardened runtime and a secure timestamp that notarisation
requires. Importing the certificate into the runner's keychain, and
the `notarytool` submission after it, are the parts still missing.

**One thing must change the day notarised builds ship:** the updater
strips `com.apple.quarantine` from the installed bundle
(`crates/poltertype-update/src/apply/macos.rs`). That is defensible
only while the app is unsigned and Gatekeeper would otherwise reject a
build the user already trusted. With a notarised build it becomes a
gratuitous weakening and must be removed.

### Linux repository key (GPG) — when repositories exist

`apt` and `dnf` refuse or warn on unsigned repositories, so hosting one
means holding a GPG key. Note what this is *not*: there is no
certificate authority anywhere in the Linux story and nothing to buy —
the key is one we generate, and the whole cost is looking after it.

Two options when the time comes, and they differ in who holds the key:

- **[Open Build Service](https://build.opensuse.org/)** signs with its
  own project key. We never hold a key; users trust OBS. Least work,
  least control.
- **A repository we host** means our key, published as a fingerprint on
  poltertype.com, with the same custody rules as the ed25519 key above:
  offline, never in CI.

AppImages support an embedded GPG signature, but almost nothing
verifies it, so it would be a signature that exists to be described
rather than checked. The manifest signature is what actually protects
the update path.

## How a release gets signed

1. A `v*` tag starts the release workflow, which builds every installer
   from the public repository on GitHub-hosted runners.
2. The workflow creates a **draft** release. Nothing is visible to
   users yet, and the in-app updater cannot see a draft.
3. *(Once SignPath is approved)* the workflow submits the `.msi` for
   signing; a human approves the request in SignPath and the signed
   artifact replaces the unsigned one.
4. The maintainer signs `latest.json` locally and uploads it.
5. The maintainer verifies the artifacts, then publishes. **Publishing
   is the act that ships the update to every existing user** — the
   updater installs it on the next restart.

Step 5 cannot be automated away, and that is the design.

## Privacy

The full policy lives at <https://poltertype.com/privacy>. Signing
tells a reviewer, and any user who checks, what this software does with
their data; the short version:

- **Typed text is never logged, and not as a matter of policy.** Since
  v0.6.3 every word that could appear in a log line or a decision
  reason passes through `poltertype_types::logsafe::redact_word`, whose
  gate is `cfg!(debug_assertions) && POLTERTYPE_UNSAFE_LOG_WORDS=1`.
  In a release build the first half is false at compile time, so **no
  configuration, environment variable or setting can make a released
  binary print what the user typed.**
- **The word buffer is RAM-only and short-lived.** It is not persisted,
  not sent anywhere, and abandoned on an idle timeout.
- **There is no telemetry, and never has been.**
- **Exactly one network request exists in a default build:** the
  updater fetching a release manifest from GitHub and, if there is a
  newer version, the installer. It sends no body, no query string and
  no identifier; GitHub sees an IP and a User-Agent naming the version,
  as it would for any download. `[updates].enabled = false` switches it
  off entirely.
- **The optional AI subsystem is off by default, and its backends are
  stubs that make no request** (`docs/AI.md`); remote AI would
  additionally require `ai.allow_remote = true`.

## Reporting a problem

A signed binary that misbehaves is worse than an unsigned one, because
the signature is what earned the user's trust. **Security reports go
through GitHub's private vulnerability reporting**, not the public
issue tracker — see [`SECURITY.md`](../SECURITY.md) for the form, what
is in scope, and what we can realistically promise about response
times.

---

## Appendix: the SignPath CI step, for when it is approved

Not committed to `release.yml` yet, and deliberately so: the step needs
an organisation id and policy slug that do not exist until the
application is accepted, and a release workflow that fails is a failed
release. Add it in the same change that adds the credentials.

Prerequisites, from
[SignPath's GitHub documentation](https://docs.signpath.io/trusted-build-systems/github):

- install the SignPath GitHub App on the repository;
- `SIGNPATH_API_TOKEN` as a repository **secret**,
  `SIGNPATH_ORGANIZATION_ID` as a repository **variable**;
- GitHub-hosted runners only — self-hosted runners are not permitted
  for open-source projects, which suits us since we use hosted runners
  already;
- the workflow must live in the repository being signed.

One thing to fix while wiring it up: the action consumes the
`artifact-id` output of `actions/upload-artifact`, and SignPath's
documentation calls for **v7 or newer**. `release.yml` is still on
`@v4` in all three jobs, and the MSI upload step has no `id:` to
reference. Both change together:

```yaml
      - name: Upload unsigned MSI
        id: upload-unsigned-msi          # ← new: the action needs this
        uses: actions/upload-artifact@v7 # ← bumped from v4
        with:
          name: msi-x86_64-unsigned
          path: target/dist/poltertype-*-x86_64-pc-windows-msvc.msi
          if-no-files-found: error

      - name: Submit the MSI for signing
        uses: signpath/github-action-submit-signing-request@v2
        with:
          api-token: ${{ secrets.SIGNPATH_API_TOKEN }}
          organization-id: ${{ vars.SIGNPATH_ORGANIZATION_ID }}
          project-slug: poltertype
          signing-policy-slug: release-signing
          github-artifact-id: ${{ steps.upload-unsigned-msi.outputs.artifact-id }}
          wait-for-completion: true
          output-artifact-directory: target/dist/signed

      - name: Upload the signed MSI
        uses: actions/upload-artifact@v7
        with:
          name: msi-x86_64
          path: target/dist/signed/poltertype-*-x86_64-pc-windows-msvc.msi
          if-no-files-found: error
```

The release job keeps consuming the `msi-x86_64` artifact and needs no
change — it receives the signed file under the name it already expects,
which also means the `latest.json` checksum is computed over the signed
bytes. That ordering is not optional: a manifest that promises the hash
of an unsigned MSI would fail verification on every machine that
downloads the signed one.

`wait-for-completion: true` blocks the job until a human approves the
request in SignPath. That is intended — it puts the approval inside the
release, where it is visible, rather than in a mailbox.
