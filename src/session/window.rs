use super::GameRuntime;
use crate::{
    Result,
    host::{
        graphics::Rect,
        platform::{
            ClipboardRequest, MenuItemRequest, WindowCloseRequest, WindowRequest, WindowServices,
        },
    },
};

impl GameRuntime {
    /// Report the content rectangle in window client coordinates. `None`
    /// selects the logical surface size and an origin of zero.
    pub fn set_window_content_rect(&mut self, rectangle: Option<Rect>) {
        self.0.set_window_content_rect(rectangle);
    }

    /// Report the raw window mode for script queries. `None` reports no mode.
    pub fn set_windowed_mode(&mut self, mode: Option<u32>) {
        self.0.set_windowed_mode(mode);
    }

    /// Process window requests through synchronous host callbacks. Returns true
    /// when the host should close. Menu and system properties requests remain queued.
    pub fn process_window_requests(&mut self, window: &mut dyn WindowServices) -> Result<bool> {
        self.0.process_window_requests(window)
    }

    /// Inspect pending window actions in submission order.
    pub fn pending_window_requests(&self) -> &[WindowRequest] {
        self.0.pending_window_requests()
    }

    /// Take pending window actions in submission order for the host event loop.
    pub fn take_window_requests(&mut self) -> Vec<WindowRequest> {
        self.0.take_window_requests()
    }

    /// Take pending script close requests in submission order. The host can
    /// stop on a notification or ask for confirmation on a standard request.
    pub fn take_window_close_requests(&mut self) -> Vec<WindowCloseRequest> {
        self.0.take_window_close_requests()
    }

    /// Take pending updates to the enabled state of menu items, in submission order.
    pub fn take_menu_item_requests(&mut self) -> Vec<MenuItemRequest> {
        self.0.take_menu_item_requests()
    }

    /// Read the current script debug mode for a menu rebuild.
    pub fn debug_mode(&self) -> u32 {
        self.0.debug_mode()
    }

    /// Report the effective mode after handling a fullscreen toggle. Updates
    /// window preferences and marks the configuration for notification on advance.
    pub fn report_fullscreen(&mut self, fullscreen: bool) {
        self.0.report_fullscreen(fullscreen);
    }

    /// Reset the game's input idle counter before confirming a menu close.
    pub fn prepare_menu_close(&mut self) -> Result<()> {
        self.0.prepare_menu_close()
    }

    /// Broadcast a window notification to the live tasks.
    pub fn notify_window_tasks(&mut self) -> Result<()> {
        self.0.notify_window_tasks()
    }

    /// Take pending clipboard writes in submission order and original encoding.
    pub fn take_clipboard_requests(&mut self) -> Vec<ClipboardRequest> {
        self.0.take_clipboard_requests()
    }
}
