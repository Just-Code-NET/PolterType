//! Backend probing and pinning — how the Linux switcher picks which
//! desktop backend actually owns the layout.

use tracing::info;

use crate::{LayoutError, LayoutSwitcher};

use super::cached_switcher::CachedSwitcher;
use super::consts::BACKEND_ENV;
use super::{cinnamon, fcitx, gnome, hyprland, ibus, kde, sway, x11};

pub fn create_switcher() -> Result<Box<dyn LayoutSwitcher>, LayoutError> {
    if let Some(name) = pinned_backend_name() {
        return create_pinned_switcher(&name);
    }

    let mut tried: Vec<&'static str> = Vec::new();
    let mut mute: Vec<&'static str> = Vec::new();

    macro_rules! probe {
        ($name:literal, $init:expr) => {{
            let built: Option<Box<dyn LayoutSwitcher>> = $init;
            if let Some(s) = built {
                if names_a_layout(s.as_ref()) {
                    return Ok(Box::new(CachedSwitcher::new(s)));
                }
                mute.push($name);
            }
            tried.push($name);
        }};
    }

    probe!("hyprland", hyprland::try_init().map(boxed));
    probe!("kde", kde::try_init().map(boxed));
    // Before gsettings, not after: Cinnamon would pass the gsettings
    // probe and then fail to switch anything (#26).
    probe!("sway", sway::try_init().map(boxed));
    probe!("cinnamon", cinnamon::try_init());
    probe!("gnome", gnome::try_init().map(boxed));
    probe!("ibus", ibus::try_init().map(boxed));
    probe!("fcitx", fcitx::try_init().map(boxed));
    probe!("x11", x11::try_init().map(boxed));

    Err(LayoutError::Unsupported(format!(
        "no Linux layout-switching backend available; probed: {tried:?}, \
         initialised but naming no layout: {mute:?}"
    )))
}

fn boxed<S: LayoutSwitcher + 'static>(s: S) -> Box<dyn LayoutSwitcher> {
    Box::new(s)
}

/// Can this backend name a single layout? If not, it is not the thing
/// driving this session, whatever it answered to "are you running".
///
/// Fcitx5 is the case that named the rule. Ubuntu installs it with
/// language support and autostarts it, so `fcitx5-remote -t 1` exits 0
/// on a desktop where fcitx owns no input method at all — and
/// `fcitx5-remote -n` then answers with an empty line. The backend
/// activated on GNOME, Xfce, MATE, LXQt, Budgie, sway, labwc and every
/// bare WM, reported `active=[LayoutId("")] count=1`, and the layout DB
/// came up with **zero** layouts loaded: a log that reads
/// `layout switcher ready` on an app that can no longer correct
/// anything. Measured across a 17-session sweep, 2026-08-24.
///
/// This is the same rule the KDE backend already applies to itself and
/// the same lesson as [#26](https://github.com/Just-Code-NET/PolterType/issues/26):
/// probe by what a desktop *does*, not by what it ships.
pub(crate) fn names_a_layout(s: &dyn LayoutSwitcher) -> bool {
    match s.list_active() {
        Ok(list) if list.iter().any(|id| !id.as_str().trim().is_empty()) => true,
        Ok(list) => {
            info!(
                backend = s.backend_name(),
                ?list,
                "backend initialised but names no layout — standing down for the next one"
            );
            false
        }
        Err(e) => {
            info!(
                backend = s.backend_name(),
                %e,
                "backend initialised but could not list layouts — standing down for the next one"
            );
            false
        }
    }
}

/// The backend name [`BACKEND_ENV`] asks for, normalised — `None` when
/// the variable is unset, blank or `auto`.
fn pinned_backend_name() -> Option<String> {
    let raw = std::env::var(BACKEND_ENV).ok()?;
    let name = raw.trim().to_ascii_lowercase();
    (!name.is_empty() && name != "auto").then_some(name)
}

fn create_pinned_switcher(name: &str) -> Result<Box<dyn LayoutSwitcher>, LayoutError> {
    fn boxed<S: LayoutSwitcher + 'static>(s: Option<S>) -> Option<Box<dyn LayoutSwitcher>> {
        s.map(|s| Box::new(s) as Box<dyn LayoutSwitcher>)
    }

    let built = match name {
        "hyprland" => boxed(hyprland::try_init()),
        "kde" | "plasma" => boxed(kde::try_init()),
        "cinnamon" => cinnamon::init_without_session_check(),
        "gnome" | "gsettings" => boxed(gnome::init_without_desktop_check()),
        "ibus" => boxed(ibus::try_init()),
        "fcitx" | "fcitx5" => boxed(fcitx::try_init()),
        "sway" => boxed(sway::try_init()),
        "x11" | "xkb" => boxed(x11::try_init()),
        other => {
            return Err(LayoutError::Unsupported(format!(
                "{BACKEND_ENV}={other:?} names no backend; expected auto, hyprland, kde, \
                 cinnamon, sway, gnome, ibus, fcitx or x11"
            )));
        }
    };

    match built {
        Some(s) => {
            info!(backend = name, "layout backend pinned by {BACKEND_ENV}");
            Ok(Box::new(CachedSwitcher::new(s)))
        }
        None => Err(LayoutError::Unsupported(format!(
            "{BACKEND_ENV}={name} was asked for, but that backend does not initialise here"
        ))),
    }
}
