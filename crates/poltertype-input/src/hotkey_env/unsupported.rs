//! Answers for platforms with no hotkey-backend quirks to report.

pub(super) fn observed_not_consumed() -> bool {
    false
}

pub(super) fn wait_for_hotkey_backend(_window: std::time::Duration) -> bool {
    true
}
