//! Neither Unix nor Windows: no way to reach another process from here.

pub fn stop_ui_children(_pid: u32) {}

pub fn restart_app() -> bool {
    false
}
