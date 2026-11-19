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

/// The stage that failed during a call to [`super::GameRuntime::advance`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AdvanceError {
    /// The failed and remaining language requests are retained; task updates have not run.
    LanguageRefresh(Error),
    /// Updating movie playback failed before the task update.
    MovieUpdate(Error),
    /// Updating the task tree failed. Earlier work in the update may have taken effect.
    TaskUpdate(Error),
    /// Delivery is pending. A later advance resumes it without rerunning that task update.
    ConfigurationNotification(Error),
}

impl AdvanceError {
    /// The original runtime or host diagnostic, with its original offset.
    pub fn diagnostic(&self) -> &Error {
        match self {
            Self::LanguageRefresh(error)
            | Self::MovieUpdate(error)
            | Self::TaskUpdate(error)
            | Self::ConfigurationNotification(error) => error,
        }
    }

    pub(crate) fn into_diagnostic(self) -> Error {
        match self {
            Self::LanguageRefresh(error)
            | Self::MovieUpdate(error)
            | Self::TaskUpdate(error)
            | Self::ConfigurationNotification(error) => error,
        }
    }
}

impl fmt::Display for AdvanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stage = match self {
            Self::LanguageRefresh(_) => "refreshing language tables",
            Self::MovieUpdate(_) => "updating movies",
            Self::TaskUpdate(_) => "updating tasks",
            Self::ConfigurationNotification(_) => "notifying tasks of configuration changes",
        };
        write!(f, "{stage} failed: {}", self.diagnostic())
    }
}

impl std::error::Error for AdvanceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.diagnostic())
    }
}
