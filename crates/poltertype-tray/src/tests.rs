use crate::{Icon, TrayError};

#[test]
fn an_icon_takes_a_buffer_that_matches_its_dimensions() {
    let icon = Icon::from_rgba(vec![0; 22 * 22 * 4], 22, 22);
    assert!(icon.is_ok(), "a 22×22 RGBA buffer should be accepted");
}

#[test]
fn an_icon_refuses_a_buffer_that_does_not() {
    // The backends hand this straight to a PNG encoder or to `tray-icon`,
    // and both read the length the dimensions imply rather than the one
    // the buffer has.
    let icon = Icon::from_rgba(vec![0; 22 * 22 * 3], 22, 22);
    assert!(
        matches!(
            icon,
            Err(TrayError::IconSize {
                want: 1936,
                got: 1452,
                ..
            })
        ),
        "an RGB-sized buffer should be refused, got {icon:?}"
    );
}

#[test]
fn an_icon_with_no_pixels_at_all_is_still_a_size_error() {
    // A rasteriser that returned early is the realistic way to get
    // here, and zero bytes for a non-zero size must not pass.
    let icon = Icon::from_rgba(Vec::new(), 22, 22);
    assert!(
        matches!(icon, Err(TrayError::IconSize { got: 0, .. })),
        "an empty buffer should be refused, got {icon:?}"
    );
}
