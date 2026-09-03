//! The Dock policy `tao` would otherwise apply on startup.

pub fn keep_out_of_dock<T>(event_loop: &mut tao::event_loop::EventLoop<T>) {
    use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
    event_loop.set_activation_policy(ActivationPolicy::Accessory);
}
