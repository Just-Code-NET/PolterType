//! What kind of version change was requested.

pub(crate) enum Change {
    Bump,
    Set(String),
}
