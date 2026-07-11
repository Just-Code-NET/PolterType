# AI subsystem

> **Status:** v0.1 ships the *architecture* — traits, plug-in
> machinery, configuration schema, key storage. Concrete model
> implementations are stubs that compile but don't actually call out
> to anything yet. v0.1.x will fill them in.

`poltertype` ships an opt-in AI/LLM subsystem that lets users:

* extend the layout-detection pipeline with smarter classifiers
  (local ONNX models, remote LLMs);
* run *word rewriters* — post-correction tricks like
  smart-capitalize, expand-acronym, slang→formal — without rebuilding
  the whole engine.

Everything below is **off by default**.

## Privacy posture

There are three independent gates between you and a network call:

1. **Cargo feature `ai`** in `poltertype-app`. Off by default; enabling adds
   the `poltertype-ai` crate to the build.
2. **Cargo feature `remote`** in `poltertype-ai`. Off by default; enabling
   adds `reqwest` + TLS so the `RemoteLlmDetector` can make HTTP
   calls. (Local detectors don't need this.)
3. **`[ai].allow_remote = true`** in `config.toml`. Off by default
   even in fully-built binaries; flips at runtime. Useful if you want
   to keep the binary capable but the network usage gated.

The tray tooltip surfaces the runtime state: `AI: on, remote: yes`,
`AI: on, remote: no`, etc., plus a per-day call counter so you can
see exactly how often the engine reaches out.

## Architecture

The `Detector` trait already lives in `poltertype-detect` (used by the
built-in `WordPlausibilityDetector`). Both shims for AI plugins live
there too:

```rust
pub trait Detector: Send + Sync {
    fn name(&self) -> &'static str;
    fn detect(&self, ctx: &DetectionContext<'_>) -> Option<DetectionVerdict>;
}

pub trait WordRewriter: Send + Sync {
    fn name(&self) -> &'static str;
    fn rewrite(&self, req: &RewriteRequest<'_>) -> RewriteVerdict;
}
```

Concrete v0.1 implementations live in `poltertype-ai`:

| Type | Crate path | Status |
|---|---|---|
| `LocalOnnxDetector` | `poltertype-ai::local` | stub (returns no verdict) |
| `RemoteLlmDetector` | `poltertype-ai::remote` | stub (no real HTTP yet) |
| `SmartCapitalize` rewriter | `poltertype-ai::rewriters` | working demo |

## Configuration (config.toml)

Detectors and rewriters are described declaratively in the user's
`config.toml`. `[ai]` itself only carries the master switches:

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

## API keys

Keys are looked up via `keyring::Entry::new("poltertype", <entry>)`,
which uses:

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

`api_key_ref = "keyring:anthropic"` then resolves to the stored
secret at request time.

## Why is the architecture in v0.1 if the implementations are stubs?

Because the *shape* of the plug-in API is the load-bearing decision.
Once `poltertype-app` is wired to iterate `[[ai.detectors]]`, swap in real
implementations is a matter of dropping in a new struct that
implements `Detector`. v0.1.x will iterate without breaking
configuration files written for v0.1.
