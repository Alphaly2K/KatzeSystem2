use crate::Error;
use std::fmt;

/// Failure to start a prepared session.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StartupError {
    MissingResources,
    MissingInput,
    MissingCanvas,
    /// Creating the initial tasks or loading the entry program failed.
    StartTasks(Error),
}

impl fmt::Display for StartupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingResources => f.write_str("game startup requires a resource service"),
            Self::MissingInput => f.write_str("game startup requires an input device"),
            Self::MissingCanvas => f.write_str("game startup requires a configured canvas"),
            Self::StartTasks(error) => write!(f, "starting game tasks failed: {error}"),
        }
    }
}

impl std::error::Error for StartupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::StartTasks(error) => Some(error),
            _ => None,
        }
    }
}
