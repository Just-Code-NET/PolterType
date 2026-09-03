//! AX/CF field ids and the query tuning they come with.

// AXValueType constants from HIServices/AXValue.h.
pub(super) const K_AXVALUE_TYPE_CGPOINT: u32 = 1;
pub(super) const K_AXVALUE_TYPE_CGSIZE: u32 = 2;
pub(super) const K_AXVALUE_TYPE_CGRECT: u32 = 3;
pub(super) const K_AXVALUE_TYPE_CFRANGE: u32 = 4;

/// Cap on how long one AX query may block the UI event loop. An app
/// that cannot answer within this is treated as one without a11y.
pub(super) const AX_MSG_TIMEOUT_SECS: f32 = 0.3;

/// A caret with zero height is no caret — it is the empty junk rect
/// several apps hand back instead. The cap filters the other extreme
/// (a whole-line "selection bounds" answer).
pub(super) const MIN_CARET_HEIGHT: f64 = 0.5;
pub(super) const MAX_CARET_HEIGHT: f64 = 120.0;

/// Retry budget for the focused-element query: it can race the target
/// app's own focus-change handling and answer `cannotComplete` /
/// `noValue` transiently (SuperDictate's resolver does the same).
pub(super) const FOCUS_RETRY_ATTEMPTS: usize = 3;
pub(super) const FOCUS_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(40);

/// Slack for the "caret belongs to its element" check: real carets
/// stick out of the field's frame by a few points (TextEdit's search
/// field reports one 9 pt above its frame), Chrome's junk is hundreds
/// of points away.
pub(super) const CARET_FRAME_SLACK: f64 = 24.0;
