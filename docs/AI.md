# AI subsystem

> **Status (v0.4.2): designed, not wired.** The extension traits are
> real and the built-in detectors use them. The `poltertype-ai` crate
> exists, compiles, and holds *stubs* — and the binary does not
> construct or call any of them. **No shipped build makes an
> AI-related network call, with or without the feature flags.**
>
> Read that claim narrowly. Since v0.4.0 the app *does* make one
> network call, in every build, on by default: the updater's check
> against GitHub Releases. It has nothing to do with this subsystem —
> it lives in `poltertype-update`, uses a different HTTP client
> (`ureq`, not `reqwest`), and sends nothing about the user. See
> [DECISIONS.md](DECISIONS.md) and the README's "Staying up to date".
> The point of this document is that **AI** adds no network call; it
> is no longer true that the binary makes none at all.
>
> This document describes the intended design and marks, in each
> section, what is actually implemented today. Nothing below is a
> promise about current behaviour unless it says "implemented".

The plan is an opt-in AI/LLM subsystem that would let users:

* extend the layout-detection pipeline with smarter classifiers
  (local ONNX models, remote LLMs);
* run *word rewriters* — post-correction tricks like
  smart-capitalize, expand-acronym, slang→formal — without rebuilding
  the whole engine.

Everything here is **off by default**, and today it is inert.

## What exists today

| Piece | Where | State |
|---|---|---|
| `Detector` / `WordRewriter` traits | `poltertype-detect::traits` | **implemented**, and `Detector` is what the built-in detectors run on |
| `DictionaryDetector`, `WordPlausibilityDetector` | `poltertype-detect` | **implemented** — these are the *only* detectors the engine runs |
| `LocalOnnxDetector` | `poltertype-ai::local` | **stub** — logs a warning, returns `NoOpinion`. No ONNX runtime is even a dependency. |
| `RemoteLlmDetector` | `poltertype-ai::remote` | **stub** — with `remote` on it builds an HTTP client and never uses it. Returns `NoOpinion`. |
| `SmartCapitalize` rewriter | `poltertype-ai::rewriters` | **implemented, unreachable** — real logic over a hardcoded 7-word list; nothing calls it. Not AI-backed. |
| `resolve_api_key()` | `poltertype-ai::keys` | **implemented, no callers** |
| `[ai] enabled` / `allow_remote` | `poltertype-core::settings` | **parsed, inert** — both default `false` and no runtime code reads them |

The gap that matters: **`poltertype-ai` is never imported by
`poltertype-app` or `poltertype-core`.** It appears in
`poltertype-app/Cargo.toml` as an optional dependency and nowhere
else. The engine's detector list is constructed by hand in
`poltertype-app::main`. Until that list is built from configuration,
none of the above can run, however the flags are set.

There is no rewriter stage in the engine at all — `WordRewriter` is a
trait with no consumer.

## Privacy posture

**Today: no build can make an AI network call.** `reqwest` is an
optional dependency of `poltertype-ai` alone, and the one type that
holds a client never issues a request. That is a stronger guarantee
than the design below, and it is the one that currently holds.

It is, however, a claim about *this subsystem* — not about the
process. The app has had exactly one network capability since v0.4.0:
the updater (`poltertype-update`, `ureq`, on by default, GitHub
Releases only). Nothing routes user text through it and it cannot be
used to reach an LLM. When wiring the AI subsystem up, do not treat
the updater's existence as precedent — the gates below still apply in
full, and "the app already talks to the network" is not an argument
for skipping any of them.

The design keeps three independent gates between a user and a network
call. Gates 1 and 2 are real (they are Cargo features); gate 3 is
parsed but not yet enforced anywhere, because there is nothing to
enforce it against:

1. **Cargo feature `ai`** in `poltertype-app`. Off by default; enabling
   adds the `poltertype-ai` crate to the build. (Note it does *not*
   forward the crate's own `remote` feature — see below.)
2. **Cargo feature `remote`** in `poltertype-ai`. Off by default;
   enabling adds `reqwest` + `rustls` so a `RemoteLlmDetector` *could*
   make HTTP calls. Local detectors don't need it. Enabling it from an
   app build takes `--features ai,poltertype-ai/remote`.
3. **`[ai].allow_remote = true`** in `config.toml`. Off by default.
   Intended to gate network use at runtime in a binary that is
   otherwise capable. **Not yet read by any code path.**

When the subsystem is wired, the tray tooltip should surface the
runtime state (whether AI is on, whether remote is permitted, and how
often the engine has reached out). It does not do so today — the
tooltip renders only the app name, the active layout, and a paused
marker.

## Architecture

The `Detector` trait lives in `poltertype-detect` and is the real
extension point — the built-in detectors implement it, and an AI
detector would be one more implementation:

```rust
pub trait Detector: Send + Sync {
    fn name(&self) -> &'static str;
    fn judge(&self, ctx: &DetectionContext<'_>) -> Verdict;
}

pub trait WordRewriter: Send + Sync {
    fn name(&self) -> &'static str;
    fn rewrite(&self, req: &RewriteRequest<'_>) -> RewriteVerdict;
}
```

`Verdict` is three-way, which is the load-bearing detail:

```rust
pub enum Verdict {
    NoOpinion,                  // defer to the next detector
    Keep { reason: String },    // veto a switch outright
    Switch(DetectionVerdict),   // request a layout change
}
```

The engine runs detectors in priority order and stops at the first
non-`NoOpinion`. `Keep` is what lets the dictionary say "this is a
real word, don't ask anyone else" — the main defence against false
positives.

## Planned configuration (not implemented)

The intended shape is declarative: detectors and rewriters described
in the user's `config.toml`, with `[ai]` carrying only the master
switches.

> **None of the `[[ai.detectors]]` / `[[ai.rewriters]]` schema below
> exists yet.** The settings struct today is exactly
> `AiSettings { enabled, allow_remote }`. Because settings parse with
> `#[serde(default)]` and no `deny_unknown_fields`, blocks like these
> are *silently ignored* rather than rejected — do not write them into
> a config expecting an effect.

```toml
[ai]
enabled = false
allow_remote = false

[[ai.detectors]]
type = "local-onnx"
id   = "fasttext-lid-176"
model_path = "models/lid.176.onnx"

[[ai.detectors]]
type = "remote-llm"
id   = "anthropic-haiku"
provider = "anthropic"
model = "claude-haiku-4-5-20251001"
api_key_ref = "keyring:anthropic"
max_latency_ms = 600

[[ai.rewriters]]
type = "smart-capitalize"
id   = "default"
require_confirmation = false
```

Wiring this up means: a settings schema for the two arrays, a factory
mapping each `type` string to a struct, and a detector list in
`poltertype-app::main` built from configuration instead of by hand.

## API keys

The lookup helper is implemented (it has no callers yet, because
nothing makes a request). Keys resolve via
`keyring::Entry::new("poltertype", <entry>)`, which uses:

* Windows Credential Manager
* macOS Keychain
* Linux: GNOME Secret Service / KWallet (whichever is up)

Storing a key (one-time, from your shell):

```bash
# macOS / Linux
secret-tool store --label "poltertype Anthropic" \
    service poltertype account anthropic
# Windows: cmdkey /add:poltertype /user:anthropic /pass:<paste-key>
```

`api_key_ref = "keyring:anthropic"` is then meant to resolve to the
stored secret at request time. Keys never live in `config.toml`.

## Why the traits landed before the implementations

Because the *shape* of the plug-in API is the load-bearing decision,
and it is settled: a detector is anything that turns a
`DetectionContext` into a three-way `Verdict`, and the engine already
runs a priority-ordered list of them. Swapping in a real
implementation is a matter of dropping in a struct that implements
`Detector` — no engine surgery.

What is *not* settled, and is what the remaining work consists of, is
the wiring: config schema, a factory, and the runtime enforcement of
`allow_remote`. Until that exists, treat this document as a design
note rather than a feature description.
