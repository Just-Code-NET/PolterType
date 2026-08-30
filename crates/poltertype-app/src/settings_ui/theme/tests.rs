use super::mark::handle;

/// One rasterisation per size, handed out again on every `view`
/// rebuild. A fresh `Handle` each time is a fresh id to the renderer,
/// so an uncached mark would be re-rasterised *and* re-uploaded on
/// every state change — on a window that is rendered entirely on the
/// CPU.
#[test]
fn the_mark_is_rasterised_once_per_size() {
    assert_eq!(handle(64).id(), handle(64).id());
    assert_ne!(handle(64).id(), handle(128).id());
}
