//! Pure parsing of `qdbus --literal` output. No process spawning here,
//! so every shape below is unit-testable without a Plasma session.

/// Marker Qt's `argumentToString` emits for each `(sss)` struct — see
/// `qtbase/src/dbus/qdbusutil.cpp`. Strings are printed `"like this"`
/// and are **not** escaped, so the first quoted run after the marker is
/// the struct's first field.
const STRUCT_MARKER: &str = "[Argument: (sss)";

/// Short names (`us`, `ru`, `ua`) of every configured layout, in the
/// order Plasma indexes them — which is what `setLayout(uint)` and
/// `getLayout() -> uint` address.
///
/// Two shapes are accepted because the interface changed:
///
/// * Plasma ≥ 5.23 — `a(sss)` of `(shortName, displayName, longName)`,
///   printed as `[Argument: a(sss) {[Argument: (sss) "us", "", "English
///   (US)"], …}]`. Plain `qdbus` cannot render this at all: it prints
///   `qdbus: I don't know how to display an argument of type 'a(sss)'`
///   to stdout and still exits 0, which is how that sentence ended up
///   being read back as a layout id
///   ([#31](https://github.com/Just-Code-NET/PolterType/issues/31)).
/// * Plasma < 5.23 — a plain `as`, printed `{"us", "ru"}`.
///
/// Empty when nothing parsed, which callers must treat as a failure
/// rather than as "no layouts configured".
pub fn layout_short_names(literal: &str) -> Vec<String> {
    if literal.contains(STRUCT_MARKER) {
        return literal
            .split(STRUCT_MARKER)
            .skip(1)
            .filter_map(first_quoted)
            .collect();
    }
    quoted_runs(literal)
}

/// The first `"…"` run in `s`.
fn first_quoted(s: &str) -> Option<String> {
    let rest = s.split_once('"')?.1;
    let (value, _) = rest.split_once('"')?;
    Some(value.to_owned())
}

/// Every `"…"` run in `s`, in order.
fn quoted_runs(s: &str) -> Vec<String> {
    s.split('"').skip(1).step_by(2).map(str::to_owned).collect()
}
