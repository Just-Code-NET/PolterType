//! The running hook thread's handle, kept until the listener stops.

use std::sync::atomic::AtomicBool;
use std::thread::JoinHandle;

pub(super) struct WorkerHandle {
    pub(super) join: JoinHandle<()>,
    pub(super) thread_id: u32,
    pub(super) stopping: std::sync::Arc<AtomicBool>,
}
