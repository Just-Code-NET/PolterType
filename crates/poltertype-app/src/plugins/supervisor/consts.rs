//! Timeouts for waiting on a plug-in process, and the log tail read
//! back to explain why one died.

use std::time::Duration;

/// How long a plug-in gets to report its state. Short on purpose: this
/// blocks the thread that draws the menu.
pub(super) const STATE_TIMEOUT: Duration = Duration::from_millis(1_500);

/// How long a plug-in gets to produce a report. Longer than the state
/// read can afford, because nothing waits on the UI thread for it and
/// the answer may cost real work. Still bounded: a pane that says "it
/// did not answer" is honest, one that never renders is not.
pub(super) const REPORT_TIMEOUT: Duration = Duration::from_secs(6);

/// How long a row's own button gets. Much longer than a report, because
/// it is not a query: the action behind one may steal focus, wait for
/// another application to switch conversation, and type a sentence at
/// human speed. Bounded all the same — a button that can hang for ever
/// is a pane that can never say what happened.
pub(super) const ACTION_TIMEOUT: Duration = Duration::from_secs(90);

/// How much of the end of a plug-in's log to read, and how much of the
/// line found there to repeat. Both are about a notification body, not
/// about diagnosis — the file itself is the diagnosis.
pub(super) const LOG_TAIL_BYTES: u64 = 8 * 1024;
pub(super) const LOG_LINE_CHARS: usize = 200;
