use super::{ClipboardRequest, ObjectFactory, Runtime, route_command_mode};
use crate::{
    Result,
    graphics::Rect,
    input::InputDelivery,
    kcs::{WindowAction, WindowRequest},
    task::{Command, CommandBuffers},
};

/// Window operations supplied by the host after a runtime update.
pub trait WindowServices {
    /// Toggle fullscreen and report the resulting mode (true for fullscreen).
    fn toggle_fullscreen(&mut self) -> Result<bool>;
    fn minimize(&mut self) -> Result<()>;
    /// Return true to accept the requested close.
    fn confirm_close(&mut self) -> bool;
}

impl<F: ObjectFactory> Runtime<F> {
    /// Report the content rectangle in window client coordinates. With `None`,
    /// queries use the logical surface size and an origin of zero.
    pub fn set_window_content_rect(&mut self, rect: Option<Rect>) {
        self.host.window_content_rect = rect;
    }

    /// Report the raw window mode for script queries. `None` reports no mode.
    /// This value is independent of the saved fullscreen setting.
    pub fn set_windowed_mode(&mut self, mode: Option<u32>) {
        self.host.windowed_mode = mode;
    }

    /// Take pending clipboard writes in submission order, keeping the original
    /// encoding. The host decides how to decode and deliver them to its system.
    pub fn take_clipboard_requests(&mut self) -> Vec<ClipboardRequest> {
        std::mem::take(&mut self.host.clipboard_requests)
    }

    /// Process script window requests. Pass the input task used for menu close.
    /// Returns true when the host should stop the session. Close notifications
    /// do not require confirmation. System properties and menu rebuild requests
    /// stay queued for the host. On error, later requests remain queued; the
    /// failing request is consumed because its effects may already have started.
    pub fn process_window_requests_with_input(
        &mut self,
        input: u32,
        window: &mut dyn WindowServices,
    ) -> Result<bool> {
        for request in std::mem::take(&mut self.host.window_close_requests) {
            if request.notify || window.confirm_close() {
                return Ok(true);
            }
        }

        let mut requests = std::mem::take(&mut self.host.window_requests).into_iter();
        let mut deferred = Vec::new();
        let result = loop {
            let Some(request) = requests.next() else {
                break Ok(false);
            };
            match self.apply_window_action(input, window, request.action) {
                Ok(Some(false)) => {}
                Ok(Some(true)) => break Ok(true),
                Ok(None) => deferred.push(request),
                Err(error) => {
                    deferred.extend(requests);
                    break Err(error);
                }
            }
        };
        deferred.append(&mut self.host.window_requests);
        self.host.window_requests = deferred;
        result
    }

    /// Window actions still awaiting handling, in submission order.
    pub fn pending_window_requests(&self) -> &[WindowRequest] {
        &self.host.window_requests
    }

    /// Remove pending actions for a host that handles its own menus or properties.
    pub fn take_window_requests(&mut self) -> Vec<WindowRequest> {
        std::mem::take(&mut self.host.window_requests)
    }

    /// Broadcast the window notification to the live task tree.
    pub fn notify_window_tasks(&mut self) -> Result<()> {
        let root = self.tree.root_id();
        route_command_mode(
            self.store(),
            root,
            Command {
                target: root,
                sender: 0,
                code: 21,
                data: [0; 3],
            },
            &mut CommandBuffers::default(),
            &mut 0,
            InputDelivery::Broadcast,
            None,
        )?;
        Ok(())
    }

    fn apply_window_action(
        &mut self,
        input: u32,
        window: &mut dyn WindowServices,
        action: WindowAction,
    ) -> Result<Option<bool>> {
        match action {
            WindowAction::ToggleFullscreen => {
                let fullscreen = window.toggle_fullscreen()?;
                if let Some(settings) = self.host.system_settings.as_mut() {
                    settings.fullscreen = u32::from(fullscreen);
                }
                self.host.system_settings_dirty = 1;
            }
            WindowAction::Minimize => window.minimize()?,
            WindowAction::NotifyTasks => self.notify_window_tasks()?,
            WindowAction::CloseFromMenu => {
                self.dispatch_command(
                    self.tree.root_id(),
                    Command {
                        target: input,
                        sender: 0,
                        code: 159,
                        data: [0, 1, 0],
                    },
                )?;
                return Ok(Some(window.confirm_close()));
            }
            WindowAction::OpenSystemProperties | WindowAction::RebuildMenu => return Ok(None),
        }
        Ok(Some(false))
    }
}
