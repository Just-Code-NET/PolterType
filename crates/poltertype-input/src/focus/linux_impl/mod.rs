//! Linux foreground-app tracking, three backends mirroring the
//! listener's session split: [`hyprland`] (`activewindow` IPC),
//! [`x11`] (EWMH), and [`caret_only`] for GNOME and KDE on Wayland,
//! which have no compositor-agnostic active-window query and get both
//! halves off the a11y bus instead.
//!
//! The first two resolve the PID through `/proc/<pid>/exe`, so the
//! reported name is the executable basename — the exact analogue of the
//! Windows tracker — with the window class as a fallback for sandboxed
//! processes whose `/proc` entry we cannot read.
//!
//! Every real query is a round-trip and `focused_exe()` sits on the
//! word-boundary path, so the factory wraps whichever backend it picks
//! in [`cache::CachedFocusTracker`]. The shared [`atspi_caret`] watcher
//! is event-driven and sits outside that cache entirely.

mod atspi_caret;
mod atspi_focus;
mod atspi_owner;
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
