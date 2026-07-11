//! Layout-switching errors.

pub use poltertype_types::LayoutId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LayoutError {
    #[error("the active platform does not support programmatic layout switching: {0}")]
    Unsupported(String),
    #[error("OS error while querying / switching layout: {0}")]
    Os(String),
    #[error("requested layout {0} is not currently active in the system")]
    NotActive(LayoutId),
}
