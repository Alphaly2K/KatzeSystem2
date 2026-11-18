use katze_system2::{
    Result,
    kcs::WindowAction,
    runtime::{ObjectFactory, Runtime},
    task::Command,
};

pub trait Services {
    fn toggle_fullscreen(&mut self) -> Result<bool>;
    fn minimize(&mut self) -> Result<()>;
    fn confirm_close(&mut self) -> bool;
}

pub fn pump<F: ObjectFactory>(
    runtime: &mut Runtime<F>,
    input: u32,
    window: &mut impl Services,
) -> Result<bool> {
    for request in std::mem::take(&mut runtime.host_mut().window_close_requests) {
        if request.notify || window.confirm_close() {
            return Ok(true);
        }
    }
    for request in std::mem::take(&mut runtime.host_mut().window_requests) {
        match request.action {
            WindowAction::ToggleFullscreen => {
                let fullscreen = window.toggle_fullscreen()?;
                if let Some(settings) = runtime.host_mut().system_settings.as_mut() {
                    settings.fullscreen = u32::from(fullscreen);
                }
                runtime.host_mut().system_settings_dirty = 1;
            }
            WindowAction::Minimize => window.minimize()?,
            WindowAction::NotifyTasks => runtime.notify_window_tasks()?,
            WindowAction::CloseFromMenu => {
                runtime.dispatch_command(
                    0,
                    Command {
                        target: input,
                        sender: 0,
                        code: 159,
                        data: [0, 1, 0],
                    },
                )?;
                if window.confirm_close() {
                    return Ok(true);
                }
            }
            WindowAction::OpenSystemProperties | WindowAction::RebuildMenu => {
                runtime.host_mut().window_requests.push(request);
            }
        }
    }
    Ok(false)
}
