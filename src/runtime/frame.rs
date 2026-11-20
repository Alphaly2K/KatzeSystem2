use super::{ObjectFactory, Runtime, TaskLifecycle, route_command_mode};
use crate::{
    Error, Result,
    input::InputDelivery,
    task::{Command, CommandBuffers},
};
use std::{collections::BTreeSet, fmt};

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

impl<F: ObjectFactory> Runtime<F> {
    /// Advance one host frame. Apply pending language changes before advancing the task tree.
    /// On a language error the failed request and remaining requests are retained for retry.
    /// Completed interface refreshes are still reported after a failed advance.
    /// After the root has exited, subsequent calls return [`FrameOutcome::Exited`].
    pub fn advance(&mut self, time: FrameTime) -> Result<FrameOutcome> {
        self.advance_frame(time)
            .map_err(AdvanceError::into_diagnostic)
    }

    pub(crate) fn advance_frame(
        &mut self,
        time: FrameTime,
    ) -> std::result::Result<FrameOutcome, AdvanceError> {
        if self.tree.node(self.tree.root_id()).is_none() {
            return Ok(FrameOutcome::Exited);
        }
        self.apply_pending_language_changes()
            .map_err(AdvanceError::LanguageRefresh)?;
        self.host.system_time_ms = time.system_time_ms;
        let active = self.update_frame(time.tick, time.increment)?;
        let interface_language_refreshed = std::mem::take(&mut self.interface_language_refreshed);
        Ok(if active {
            FrameOutcome::Running {
                interface_language_refreshed,
            }
        } else {
            FrameOutcome::Exited
        })
    }

    /// 完成一轮树更新后广播配置变更并清除标记。
    /// 通知失败后先续完原轮次，不重复推进时钟和主体。
    pub fn update(&mut self, now: u32, increment: u32) -> Result<bool> {
        self.update_frame(now, increment)
            .map_err(AdvanceError::into_diagnostic)
    }

    fn update_frame(
        &mut self,
        now: u32,
        increment: u32,
    ) -> std::result::Result<bool, AdvanceError> {
        self.resources.images.set_cache_limit(
            (self
                .host
                .platform_settings
                .as_ref()
                .map_or(32, |s| s.etc_imgcache) as usize)
                .saturating_mul(1 << 20),
        );
        self.resources
            .advance_movies(self.host.system_time_ms)
            .map_err(AdvanceError::MovieUpdate)?;
        if self.configuration_notification.is_some() {
            return self.finish_configuration_notification();
        }
        let mut lifecycle = TaskLifecycle {
            numbers: &mut self.numbers,
            host: &mut self.host,
            resources: &mut self.resources,
            objects: &mut self.objects,
            running: &mut self.running,
            factory: &mut self.factory,
            budget: self.budget,
            limit: self.limit,
        };
        let active = self
            .tree
            .update(now, increment, &mut lifecycle)
            .map_err(AdvanceError::TaskUpdate)?;
        if active && self.host.system_settings_dirty != 0 {
            self.configuration_notification = Some(BTreeSet::new());
            return self.finish_configuration_notification();
        }
        Ok(active)
    }

    fn finish_configuration_notification(&mut self) -> std::result::Result<bool, AdvanceError> {
        let mut pending = self.configuration_notification.take().unwrap();
        let root = self.tree.root_id();
        if self.tree.node(root).is_none() {
            return Ok(false);
        }
        let result = route_command_mode(
            self.store(),
            root,
            Command {
                target: root,
                sender: 0,
                code: 19,
                data: [0; 3],
            },
            &mut CommandBuffers::default(),
            &mut 0,
            InputDelivery::Broadcast,
            Some(&mut pending),
        );
        if let Err(error) = result {
            self.configuration_notification = Some(pending);
            return Err(AdvanceError::ConfigurationNotification(error));
        }
        self.host.system_settings_dirty = 0;
        Ok(true)
    }
}
