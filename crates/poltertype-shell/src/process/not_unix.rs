//! Windows: no signal, so this is an honest no-op.

/// Windows has no signal, and the console control events that stand in
/// for one cannot be sent from here: `GenerateConsoleCtrlEvent`
/// addresses a process group sharing the caller's console, and a GUI
/// image has none. Reaching one would mean `AttachConsole` onto the
/// child — process-wide state changed from the quit path of a process
/// holding a global keyboard hook.
///
/// So this is an honest no-op and the caller's kill ends the child,
/// meaning a Windows plug-in does **not** flush on the way out. See
/// `docs/DECISIONS.md`.
pub fn request_stop(_pid: u32) {}
