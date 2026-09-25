use crate::{LayoutError, LayoutId, LayoutSwitcher};

use super::probe::names_a_layout;

/// A backend that answers whatever the test wants it to.
struct Fake(Result<Vec<LayoutId>, LayoutError>);

impl LayoutSwitcher for Fake {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        Ok(LayoutId::new("en-US"))
    }
    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        match &self.0 {
            Ok(v) => Ok(v.clone()),
            Err(e) => Err(LayoutError::Unsupported(e.to_string())),
        }
    }
    fn switch_to(&self, _: &LayoutId) -> Result<(), LayoutError> {
        Ok(())
    }
    fn backend_name(&self) -> &'static str {
        "fake"
    }
}

fn fake(ids: &[&str]) -> Fake {
    Fake(Ok(ids.iter().map(|s| LayoutId::new(*s)).collect()))
}

/// The fcitx5 case: running, answering, and owning nothing. Ubuntu
/// autostarts it with language support, so this is the default
/// state on a machine that never configured an input method — and
/// before this guard it took the layout DB down to zero layouts on
/// every desktop but KDE and Cinnamon.
#[test]
fn a_backend_naming_no_layout_is_not_the_one_driving_the_session() {
    assert!(!names_a_layout(&fake(&[""])), "an empty id names nothing");
    assert!(!names_a_layout(&fake(&[])), "an empty list names nothing");
    assert!(!names_a_layout(&fake(&["  "])), "whitespace names nothing");
    assert!(
        !names_a_layout(&Fake(Err(LayoutError::Unsupported("no".into())))),
        "a backend that cannot be asked cannot be trusted to switch"
    );
}

#[test]
fn a_backend_that_names_one_is_accepted() {
    assert!(names_a_layout(&fake(&["en-US"])));
    assert!(
        names_a_layout(&fake(&["", "ru-RU"])),
        "one real layout among blanks is still a working backend"
    );
}

/// A backend whose answer the test moves by hand — the compositor
/// applying another window's layout underneath the cache.
struct Moving(std::sync::Mutex<LayoutId>);

impl LayoutSwitcher for Moving {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        Ok(self.0.lock().unwrap_or_else(|p| p.into_inner()).clone())
    }
    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        Ok(Vec::new())
    }
    fn switch_to(&self, _: &LayoutId) -> Result<(), LayoutError> {
        Ok(())
    }
    fn backend_name(&self) -> &'static str {
        "moving"
    }
}

/// Issue #72: a word's layout read in the instant after an Alt+Tab is
/// the previous window's, and re-reading it through the cache hands the
/// same stale answer back for the whole TTL. `current_fresh` has to go
/// past it — and leave the cache agreeing, or the next plain read
/// contradicts the one just made.
#[test]
fn a_fresh_read_goes_past_the_cache_and_updates_it() {
    let inner = std::sync::Arc::new(Moving(std::sync::Mutex::new(LayoutId::new("ru-RU"))));
    struct Shared(std::sync::Arc<Moving>);
    impl LayoutSwitcher for Shared {
        fn current(&self) -> Result<LayoutId, LayoutError> {
            self.0.current()
        }
        fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
            self.0.list_active()
        }
        fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError> {
            self.0.switch_to(id)
        }
        fn backend_name(&self) -> &'static str {
            "shared"
        }
    }
    let cached = super::cached_switcher::CachedSwitcher::new(Box::new(Shared(inner.clone())));

    let ru = Some(LayoutId::new("ru-RU"));
    let en = Some(LayoutId::new("en-US"));
    assert_eq!(cached.current().ok(), ru);
    *inner.0.lock().unwrap_or_else(|p| p.into_inner()) = LayoutId::new("en-US");
    assert_eq!(
        cached.current().ok(),
        ru,
        "inside the TTL the cache answers — the premise of the test"
    );
    assert_eq!(cached.current_fresh().ok(), en);
    assert_eq!(
        cached.current().ok(),
        en,
        "the cache must follow the fresh read"
    );
}
