//! Linux foreground-app tracking, two backends mirroring the listener's
//! session split:
//!
//! * **Hyprland** — `activewindow` over the same IPC socket the layout
//!   switcher uses, with an `hyprctl` subprocess as fallback.
//! * **X11** — `_NET_ACTIVE_WINDOW` then `_NET_WM_PID`. Plain EWMH;
//!   every mainstream X11 WM sets both.
//!
//! Both resolve the PID through `/proc/<pid>/exe`, so the reported name
//! is the executable basename — the exact analogue of the Windows
//! tracker — with the window class as a fallback for sandboxed
//! processes whose `/proc` entry we cannot read.
//!
//! GNOME and KDE on Wayland have no compositor-agnostic active-window
//! query, so `focused_exe()` and window geometry stay `None` there.
//! They do get [`caret_only`], because AT-SPI is a session-bus service
//! that answers regardless of compositor — and the caret is a *better*
//! tooltip anchor than the window rect, not a lesser one.
//!
//! Every real query is a round-trip and `focused_exe()` sits on the
//! word-boundary path, so the factory wraps whichever backend it picks
//! in [`cache::CachedFocusTracker`]. The shared [`atspi_caret`] watcher
//! is event-driven and sits outside that cache entirely.

mod atspi_caret;
mod atspi_focus;
mod cache;
mod caret_only;
mod consts;
mod hyprland;
mod hyprland_ipc;
mod pick;
mod proc_exe;
mod x11;

pub(crate) use pick::*;

#[cfg(test)]
mod tests;
