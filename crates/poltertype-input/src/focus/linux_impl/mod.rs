//! Linux foreground-app tracking.
//!
//! Two backends, mirroring the listener's session split:
//!
//! * **Hyprland** — `activewindow` over the same IPC socket the layout
//!   switcher talks to (`hyprctl` subprocess as the fallback).
//! * **X11** — `_NET_ACTIVE_WINDOW` on the root window, then
//!   `_NET_WM_PID` on that window. Plain EWMH; every mainstream X11 WM
//!   sets both.
//!
//! Both resolve the window's PID through `/proc/<pid>/exe`, so the
//! reported name is the executable basename — the exact analogue of
//! the Windows tracker's process-image basename — with the window
//! class as a fallback for sandboxed processes whose `/proc` entry we
//! can't read.
//!
//! GNOME / KDE on Wayland have no compositor-agnostic "active window"
//! query (by design — the same story as global input); they keep the
//! noop tracker until a per-DE backend (KWin script / GNOME shell
//! extension) exists.
//!
//! Every real query is a socket or X11 round-trip, and `focused_exe()`
//! sits on the engine's word-boundary path plus the 250 ms wordlist-
//! profile watcher — so the factory wraps whichever backend it picks
//! in a small TTL cache ([`cache::CachedFocusTracker`]).

mod cache;
mod consts;
mod hyprland;
mod hyprland_ipc;
mod pick;
mod proc_exe;
mod x11;

pub(crate) use pick::*;

#[cfg(test)]
mod tests;
