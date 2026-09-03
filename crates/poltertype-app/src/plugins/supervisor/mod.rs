//! Running plug-in programs, and stopping them again.
//!
//! A plug-in is a separate process, never code loaded into this one —
//! see `docs/ARCHITECTURE.md` § Plug-ins for why, and for the four
//! things this deliberately does not do (no restart loop, no shell, no
//! inherited stdin, no filtering of a plug-in's own output).
//!
//! A **service** runs for as long as PolterType does; a **command** is
//! a one-shot invocation behind a menu entry. A service's output goes
//! to `logs/plugin-<id>.log`, truncated at every start, and the tail of
//! that file is what [`Supervisor::reap`] quotes to the user.
//!
//! | File | Concern |
//! |---|---|
//! | [`state`] | the struct, its fields, construction |
//! | [`lifecycle`] | starting, reaping and stopping services |
//! | [`commands`] | the plug-in command API: declared commands, state, reports |
//! | [`process`] | spawning a child, and waiting on one with a deadline |
//! | [`consts`] | timeouts and log-tail sizes the command API waits with |
//! | [`types`] | `Running`, one supervised service |

mod commands;
mod consts;
mod lifecycle;
mod process;
mod state;
mod types;

pub use commands::{
    read_report, read_rows, read_state, run_command, run_command_for_row,
    run_command_for_row_waiting,
};
pub use state::Supervisor;

#[cfg(test)]
mod tests;
