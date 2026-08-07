use super::*;

#[test]
fn gnome_and_its_derivatives_sync_the_schema() {
    for desktop in [
        "GNOME",
        "ubuntu:GNOME",
        "Budgie:GNOME",
        "GNOME-Classic:GNOME",
    ] {
        assert!(
            shell_syncs_input_sources(desktop),
            "{desktop} should be treated as a schema-syncing shell"
        );
    }
}

#[test]
fn unity_and_pantheon_sync_the_schema() {
    assert!(shell_syncs_input_sources("Unity:Unity7:ubuntu"));
    assert!(shell_syncs_input_sources("Pantheon"));
}

#[test]
fn cinnamon_does_not_sync_the_schema() {
    // Both spellings distros use.
    assert!(!shell_syncs_input_sources("X-Cinnamon"));
    assert!(!shell_syncs_input_sources("X-Cinnamon:Cinnamon"));
}

#[test]
fn a_desktop_we_have_never_heard_of_does_not_sync_the_schema() {
    assert!(!shell_syncs_input_sources(""));
    assert!(!shell_syncs_input_sources("MATE"));
    assert!(!shell_syncs_input_sources("XFCE"));
}

#[test]
fn without_ibus_the_schema_is_authoritative_everywhere() {
    // Nothing is mediating, so whoever populated `sources` is the one
    // reading it back — including desktops we do not recognise.
    assert!(gsettings_is_authoritative("X-Cinnamon", false));
    assert!(gsettings_is_authoritative("XFCE", false));
    assert!(gsettings_is_authoritative("GNOME", false));
}

#[test]
fn ibus_on_gnome_leaves_the_schema_authoritative() {
    // gnome-shell drives IBus from the schema; writing it is the switch.
    assert!(gsettings_is_authoritative("ubuntu:GNOME", true));
}

#[test]
fn ibus_on_cinnamon_takes_the_schema_out_of_the_running() {
    // The reported case: the write lands in dconf and nothing acts on it.
    assert!(!gsettings_is_authoritative("X-Cinnamon", true));
}
