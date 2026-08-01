# AI subsystem

> **Status (v0.10.0): a working socket, and nothing plugged into it.**
> PolterType ships the *interface* for a language model and never a
> model, a vendor SDK, or a default endpoint. Configure
> `[[ai.plugins]]` to point at an Ollama on your own machine, an API
> you hold the key to, or a gateway of your own, and the engine gains
> another voice in the layout decision. Configure nothing — the
> default — and there is no AI in PolterType at all.
>
> This is different from every previous release, where the backends
> existed as stubs that returned no opinion. They are gone. What
> replaced them is one detector that speaks three common HTTP shapes
> and asks the model exactly one question.

## The design, and why

Bundling a model would mean choosing a vendor on the user's behalf and
shipping megabytes most people never asked for. Bundling a client for
one provider is the same choice with extra steps. So PolterType
bundles neither: it provides a socket, and what answers is whatever
the user already trusts.

That is also what keeps the zero-telemetry posture intact. There is no
address in this subsystem that we chose. The only endpoint it ever
contacts is one the user typed into their own config file, and the
only credential it uses is one they stored in their own keychain.

## Configuration

```toml
[ai]
enabled      = true
allow_remote = false   # only needed for a non-loopback endpoint

# A model running on your own machine. No key, no network permission:
# nothing leaves the computer.
[[ai.plugins]]
type     = "llm"
id       = "local"
provider = "ollama"          # preset: fills in endpoint + format
model    = "llama3"

# A third-party API. Needs `allow_remote = true` above, and a key you
# stored in the OS keychain yourself.
[[ai.plugins]]
type        = "llm"
id          = "claude"
provider    = "anthropic"
model       = "claude-haiku-4-5-20251001"
api_key_ref = "keyring:anthropic"

# Anything else that speaks a shape we know — a llama.cpp server, an
# LM Studio, a vLLM, a company gateway. No preset needed.
[[ai.plugins]]
type     = "llm"
id       = "work-gateway"
endpoint = "https://llm.internal.example.com/v1/chat/completions"
format   = "openai-chat"
model    = "qwen2.5-7b"
```

| Field | Meaning |
|---|---|
| `type` | `llm` — the only kind today |
| `id` | your name for the entry; appears in logs and in the verdict reason |
| `provider` | optional preset filling in `endpoint` + `format`: `ollama`, `llama-cpp`, `lm-studio`, `openai`, `anthropic` |
| `endpoint` | full URL to POST to. Overrides the preset |
| `format` | `openai-chat`, `anthropic-messages`, `ollama-generate`. Overrides the preset |
| `model` | required — the model name to ask for |
| `api_key_ref` | `keyring:<entry>`. Optional; a local model needs none |
| `mode` | `background` (default) or `blocking` — see below |
| `max_latency_ms` | per-query budget. Default 2000; capped at 250 in `blocking` mode |
| `cache_size` | decided words remembered. Default 2048; `0` disables |

There is deliberately **no default endpoint**. An entry with neither
`endpoint` nor `provider` is refused with a message saying so.

## The two things that make this safe to turn on

### It cannot slow your typing down

`judge()` runs on the correction path — between you finishing a word
and the word being fixed. A round-trip there, even to localhost, is
the difference between a correction and a glitch.

So the default mode **never waits**. It answers from a cache of
already-decided words; on a miss it returns "no opinion" immediately
and queues the question so the *next* occurrence of that word is
decided. The first time you type a word the model contributes nothing,
which is exactly what happened before there was a backend. After that
it is free.

That trade works because of how people type: the same few thousand
words, over and over. A 2048-entry cache warms up within a session.

`mode = "blocking"` puts the call inline if you want it, and is capped
at 250 ms — past roughly a fifth of a second you have already started
the next word, and a "correction" arriving then is just corruption
arriving late. Asking for more is refused at startup, with the reason,
rather than silently clamped into lag you would have to diagnose.

### Local is not remote

`[ai].allow_remote` exists to gate **typed words leaving your
machine**. A request to `127.0.0.1` does not leave it, so a local
model does not need that switch — requiring it would make people
enable network access they are not using.

The distinction is decided in one place, `poltertype-ai::locality`,
and it is deliberately strict:

* only literal loopback addresses and the name `localhost` count;
* DNS is **not** resolved — a resolver answer can change between the
  check and the request, and a `local.corp.net` that happens to point
  at 127.0.0.1 today is exactly the kind of thing that should still
  require a yes;
* anything unparseable is treated as remote, because the answer that
  asks permission is the safe one to be wrong with.

## What is sent, and what is not

One request per newly-seen ambiguous word, containing:

* the candidate readings of that word, numbered;
* the model name you configured;
* a fixed one-sentence instruction asking which reading is real.

That is all. Not the surrounding sentence, not the document, not the
application you are typing in, and **not the layout ids** — those
would reveal which languages you have installed. The model is asked to
reply with a single number, and anything that is not a number naming a
candidate is treated as no opinion.

Nothing typed is ever logged: words reaching a `tracing` call go
through `redact_word` like everywhere else in the engine, and the
decision cache stores hashes of the question rather than the text.

## The gates, in order

Each is a real barrier, not a setting that looks like one:

1. **Cargo feature `ai`** in `poltertype-app`. Off by default;
   enabling links the `poltertype-ai` crate.
2. **Cargo feature `remote`** in `poltertype-ai`. Off by default.
   Without it no HTTP client is compiled in — `cargo tree` on a stock
   build shows no `reqwest` at all, which is checkable rather than
   merely documented. Enabling from an app build takes
   `--features ai,poltertype-ai/remote`.
3. **`[ai].enabled = true`** in `config.toml`. Off by default.
4. **`[ai].allow_remote = true`** — additionally, and only for a
   non-loopback endpoint.
5. **A key in your keychain**, if the endpoint needs one. A literal
   secret in `config.toml` is refused at construction, never used: a
   key in that file is a key in your backups, your dotfiles repo, and
   any log you attach to an issue.

A plug-in that is not permitted still *loads* — it just returns no
opinion, and says why once at startup. That way flipping a setting
takes effect on the next restart without editing the entry.

## What the updater has to do with this: nothing

The app has had exactly one network capability since v0.4.0 — the
updater, which fetches a release manifest from GitHub. It is a
different crate, a different HTTP client (`ureq`), and no user text
goes near it.

Do not treat its existence as precedent when touching this subsystem.
"The app already talks to the network" is not an argument for relaxing
any of the five gates above. The updater sends nothing about you; this
subsystem, when you switch it on, sends the words you type. Those are
different things and the difference is the whole point.

## Architecture

The `Detector` trait in `poltertype-detect` is the extension point.
The built-in detectors implement it and an AI detector is one more
implementation:

```rust
pub trait Detector: Send + Sync {
    fn name(&self) -> &'static str;
    fn judge(&self, ctx: &DetectionContext<'_>) -> Verdict;
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

Plug-ins are **appended** to the built-in detectors, never substituted
for them. The offline dictionary and plausibility detectors keep
working exactly as before; an LLM adds a voice to a decision it does
not own. If the model picks the layout you are already typing in, that
becomes a `Keep` — a vote to leave the word alone — rather than a
switch to where you already are.

| Piece | Where |
|---|---|
| `LlmDetector` | `poltertype-ai::detector` |
| Request/response shaping | `poltertype-ai::wire` — no HTTP types, so it is unit-tested on every host |
| The one place a socket opens | `poltertype-ai::transport`, behind `feature = "remote"` |
| Loopback-vs-remote | `poltertype-ai::locality` |
| Decision cache | `poltertype-ai::cache` |
| Config → detector | `poltertype-ai::factory` |

## API keys

Keys resolve via `keyring::Entry::new("poltertype", <entry>)`:

* Windows Credential Manager
* macOS Keychain
* Linux: GNOME Secret Service / KWallet (whichever is up)

Storing one, from your shell:

```bash
# Linux
secret-tool store --label "poltertype Anthropic" \
    service poltertype account anthropic
# macOS
security add-generic-password -s poltertype -a anthropic -w
# Windows
cmdkey /add:poltertype /user:anthropic /pass:<paste-key>
```

`api_key_ref = "keyring:anthropic"` then resolves to it at startup. If
the keychain cannot supply it — missing entry, locked keychain — the
plug-in loads and stays silent with one explanatory warning, rather
than disappearing with a message about config that config cannot fix.

## Word rewriters remain unimplemented

`WordRewriter` is a trait with no consumer: there is no rewriter stage
in `poltertype-core`, so an `[[ai.rewriters]]` block is **silently
ignored**. `SmartCapitalize` in `poltertype-ai::rewriters` is real
logic over a hardcoded word list that nothing calls, and it is not
AI-backed. Do not write an `[[ai.rewriters]]` entry expecting an
effect.
