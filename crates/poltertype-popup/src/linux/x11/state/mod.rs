//! The X11 thread's state: the connection, the mapped window (if any)
//! and the thread loop that drives them. One dedicated thread owns the
//! connection and the window; parked on the command channel while
//! hidden, ~16 ms tick with `poll_for_event` while mapped.
//!
//! One `impl` block per concern, one file per `impl` block:
//!
//! | File | Concern |
//! |---|---|
//! | [`x11_state`] | the struct, its fields, construction |
//! | [`types`] | plain data: `Atoms`, `VisualPick`, `WinView` |
//! | [`run_loop`] | the thread loop, command dispatch |
//! | [`show`] | creating and placing the window |
//! | [`paint`] | uploading the rendered pixmap |
//! | [`events`] | the X event loop, hover, deadline, teardown |

mod events;
mod paint;
mod run_loop;
mod show;
mod types;
mod x11_state;

pub(super) use run_loop::run;
