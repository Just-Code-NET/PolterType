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
