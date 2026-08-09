//! `LlmDetector` — the socket a user plugs their own model into.
//!
//! It knows how to ask a question and read an answer; *what* answers is
//! whatever the user pointed it at, with a key only they hold.
//!
//! Three properties the rest of this file exists to hold up:
//!
//! * **Off unless asked for, twice.** `[ai].enabled` builds it, and a
//!   non-loopback host additionally needs `[ai].allow_remote`. A
//!   detector that may not run returns no opinion rather than failing
//!   to construct.
//! * **It cannot slow typing down.** The default mode never waits: it
//!   answers from the cache and queues the miss. `blocking` exists, is
//!   capped, and is the user putting a model in the path of their own
//!   keystrokes on purpose.
//! * **It never logs what was typed** — every word reaching a `tracing`
//!   call goes through `redact_word` first.

use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "remote")]
use std::sync::mpsc::Receiver;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex};

use poltertype_detect::{DetectionContext, DetectionVerdict, Detector, Verdict};
use tracing::{info, warn};

use crate::AiError;
use crate::cache::{Decision, DecisionCache};
use crate::consts::QUEUE_DEPTH;
use crate::enums::{Locality, QueryMode, WireFormat};
#[cfg(feature = "remote")]
use crate::transport::Call;

/// Everything the detector needs, already validated by the factory.
pub struct LlmSettings {
    pub id: String,
    pub endpoint: String,
    pub format: WireFormat,
    pub model: String,
    pub api_key: Option<String>,
    /// A key was configured but the keychain could not supply it. The
    /// detector loads and stays silent rather than calling an endpoint
    /// that will certainly reject it.
    pub key_unavailable: bool,
    pub max_latency_ms: u64,
    pub mode: QueryMode,
    pub cache_size: usize,
    pub locality: Locality,
    /// `[ai].allow_remote`. Only consulted for a remote endpoint.
    pub allow_remote: bool,
}

impl LlmSettings {
    /// Whether this detector is allowed to make its call at all.
    pub fn permitted(&self) -> bool {
        cfg!(feature = "remote")
            && !self.key_unavailable
            && match self.locality {
                Locality::Loopback => true,
                Locality::Remote => self.allow_remote,
            }
    }
}

/// A question handed to the background worker.
///
/// Only read by the worker, which only exists with an HTTP client —
/// so without the feature this is a type nothing consumes.
#[cfg_attr(not(feature = "remote"), allow(dead_code))]
struct Job {
    key: u64,
    candidates: Vec<String>,
}

pub struct LlmDetector {
    settings: Arc<LlmSettings>,
    cache: Arc<Mutex<DecisionCache>>,
    queue: Option<SyncSender<Job>>,
    /// Set after the first failed call so the worker complains once
    /// rather than once per word.
    reported_failure: Arc<AtomicBool>,
    #[cfg(feature = "remote")]
    client: Option<reqwest::blocking::Client>,
}

impl LlmDetector {
    pub fn new(settings: LlmSettings) -> Result<Self, AiError> {
        let cache_size = settings.cache_size;
        let settings = Arc::new(settings);
        let cache = Arc::new(Mutex::new(DecisionCache::new(cache_size)));
        let reported_failure = Arc::new(AtomicBool::new(false));

        #[cfg(feature = "remote")]
        let client = if settings.permitted() {
            Some(
                reqwest::blocking::Client::builder()
                    .timeout(std::time::Duration::from_millis(
                        settings.max_latency_ms.max(50),
                    ))
                    .build()
                    .map_err(AiError::Remote)?,
            )
        } else {
            None
        };

        // The worker only exists in background mode, and only when the
        // detector is actually allowed to call. No permission, no
        // thread.
        let queue = if settings.mode == QueryMode::Background && settings.permitted() {
            let (tx, rx) = sync_channel::<Job>(QUEUE_DEPTH);
            #[cfg(feature = "remote")]
            spawn_worker(
                rx,
                Arc::clone(&settings),
                Arc::clone(&cache),
                Arc::clone(&reported_failure),
                client.clone(),
            );
            #[cfg(not(feature = "remote"))]
            drop(rx);
            Some(tx)
        } else {
            None
        };

        let built = Self {
            settings,
            cache,
            queue,
            reported_failure,
            #[cfg(feature = "remote")]
            client,
        };
        built.announce();
        Ok(built)
    }

    /// Say once, at construction, what this detector will do. `judge`
    /// stays silent — it runs per word, and a detector that logs on
    /// the correction path costs more than it gives.
    fn announce(&self) {
        let s = &self.settings;
        if !cfg!(feature = "remote") {
            warn!(
                id = %s.id,
                "LLM plug-in loaded but this build has no HTTP client (`remote` cargo feature \
                 off) — it will return no opinion"
            );
            return;
        }
        if s.key_unavailable {
            warn!(
                id = %s.id,
                "LLM plug-in has no usable API key — it will return no opinion. See the keychain \
                 warning above."
            );
            return;
        }
        if s.locality == Locality::Remote && !s.allow_remote {
            warn!(
                id = %s.id,
                endpoint = %s.endpoint,
                "LLM plug-in points at a non-loopback endpoint but `[ai].allow_remote = false` \
                 — it will return no opinion until that is switched on. Typed words would leave \
                 this machine, so the switch is deliberate."
            );
            return;
        }
        info!(
            id = %s.id,
            endpoint = %s.endpoint,
            format = ?s.format,
            model = %s.model,
            mode = ?s.mode,
            local = s.locality == Locality::Loopback,
            "LLM plug-in active"
        );
        if s.locality == Locality::Remote {
            warn!(
                id = %s.id,
                endpoint = %s.endpoint,
                "this plug-in sends the words you type to a third party you configured. \
                 Nothing else in PolterType does that."
            );
        }
    }

    /// Cache lookup shared by both modes.
    fn cached(&self, key: u64) -> Option<Decision> {
        self.cache.lock().ok()?.get(key)
    }

    fn remember(&self, key: u64, decision: Decision) {
        if let Ok(mut c) = self.cache.lock() {
            c.insert(key, decision);
        }
    }
}

impl Detector for LlmDetector {
    fn name(&self) -> &'static str {
        "llm"
    }

    fn judge(&self, ctx: &DetectionContext<'_>) -> Verdict {
        // Every early return is silent: this is the correction path.
        if !self.settings.permitted() || ctx.candidates.len() < 2 {
            return Verdict::NoOpinion;
        }

        let candidates: Vec<String> = ctx.candidates.iter().map(|(_, t)| t.clone()).collect();
        let key = DecisionCache::key(&candidates);

        if let Some(decision) = self.cached(key) {
            return to_verdict(decision, ctx, &self.settings.id);
        }

        match self.settings.mode {
            QueryMode::Background => {
                // Queue and get out of the way. A full queue means the
                // endpoint is slower than the user types, in which
                // case dropping is right — a stale answer to a word
                // typed a minute ago helps nobody.
                if let Some(q) = &self.queue {
                    let _ = q.try_send(Job {
                        key,
                        candidates: candidates.clone(),
                    });
                }
                Verdict::NoOpinion
            }
            QueryMode::Blocking => {
                let decision = self.ask_now(&candidates);
                match decision {
                    Ok(d) => {
                        self.remember(key, d);
                        to_verdict(d, ctx, &self.settings.id)
                    }
                    Err(e) => {
                        report_once(&self.reported_failure, &self.settings.id, &e);
                        Verdict::NoOpinion
                    }
                }
            }
        }
    }
}

impl LlmDetector {
    #[cfg(feature = "remote")]
    fn ask_now(&self, candidates: &[String]) -> Result<Decision, AiError> {
        let Some(client) = &self.client else {
            return Ok(None);
        };
        crate::transport::ask(
            client,
            &Call {
                endpoint: &self.settings.endpoint,
                format: self.settings.format,
                model: &self.settings.model,
                api_key: self.settings.api_key.as_deref(),
                candidates,
            },
        )
    }

    #[cfg(not(feature = "remote"))]
    fn ask_now(&self, _candidates: &[String]) -> Result<Decision, AiError> {
        Ok(None)
    }
}

/// Turn a remembered index into a verdict against the live context.
///
/// The index is into the candidate list, which is rebuilt per word —
/// so a cached answer is only reused when the candidate list matches,
/// which is what the cache key guarantees.
fn to_verdict(decision: Decision, ctx: &DetectionContext<'_>, id: &str) -> Verdict {
    let Some(idx) = decision else {
        return Verdict::NoOpinion;
    };
    let Some((layout, _)) = ctx.candidates.get(idx) else {
        return Verdict::NoOpinion;
    };
    if layout == ctx.current_layout {
        // The model picked the reading the user is already producing:
        // that is a vote to leave the word alone, not a switch.
        return Verdict::Keep {
            reason: format!("llm[{id}]: current layout reads as real text"),
        };
    }
    Verdict::Switch(DetectionVerdict {
        best_layout: layout.clone(),
        confidence: 0.75,
        reason: format!("llm[{id}]"),
    })
}

/// Complain about a broken endpoint once per run, not once per word.
fn report_once(flag: &AtomicBool, id: &str, e: &AiError) {
    if !flag.swap(true, Ordering::Relaxed) {
        warn!(id = %id, %e, "LLM plug-in call failed; it will stay quiet from here on");
    }
}

#[cfg(feature = "remote")]
fn spawn_worker(
    rx: Receiver<Job>,
    settings: Arc<LlmSettings>,
    cache: Arc<Mutex<DecisionCache>>,
    reported_failure: Arc<AtomicBool>,
    client: Option<reqwest::blocking::Client>,
) {
    let Some(client) = client else { return };
    let name = format!("poltertype-llm-{}", settings.id);
    let spawned = std::thread::Builder::new().name(name).spawn(move || {
        for job in rx {
            // Re-check: another job may have answered this question
            // while this one waited in the queue.
            if cache.lock().is_ok_and(|c| c.get(job.key).is_some()) {
                continue;
            }
            let result = crate::transport::ask(
                &client,
                &Call {
                    endpoint: &settings.endpoint,
                    format: settings.format,
                    model: &settings.model,
                    api_key: settings.api_key.as_deref(),
                    candidates: &job.candidates,
                },
            );
            match result {
                Ok(decision) => {
                    if let Ok(mut c) = cache.lock() {
                        c.insert(job.key, decision);
                    }
                }
                Err(e) => report_once(&reported_failure, &settings.id, &e),
            }
        }
    });
    if let Err(e) = spawned {
        warn!(%e, "could not start the LLM worker thread; the plug-in will stay quiet");
    }
}

#[cfg(test)]
mod tests;
