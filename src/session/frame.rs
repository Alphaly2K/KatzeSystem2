use crate::Error;
use std::fmt;

/// Clock values supplied by the host for a single update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameTime {
    /// Absolute game clock in logical ticks (normally 60 per second).
    pub tick: u32,
    /// Logical increment forwarded to tasks for this update.
    pub increment: u32,
    /// Wrapping millisecond clock for system queries and movies.
    pub system_time_ms: u32,
}

/// The outcome of advancing a session by one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameOutcome {
    Running {
        /// The host can read the current string tables to refresh its UI.
        interface_language_refreshed: bool,
    },
    /// The root task has exited. The host should stop advancing and presenting.
    Exited,
}

/// The stage that failed during a call to [`crate::session::GameRuntime::advance`].
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
