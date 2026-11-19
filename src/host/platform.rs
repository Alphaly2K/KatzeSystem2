use crate::Result;

/// Requests for platform work that runs independently of the script VM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformJobRequest<'a> {
    /// The formatted request bytes, without a terminating zero.
    Plugin(&'a [u8]),
    /// Select and register an image in the host's import directory.
    ImageImport,
}

pub trait PlatformJob {
    /// Return None while pending, or the completed result (used by the plugin query).
    fn poll(&mut self) -> Option<u32>;
}

pub trait PlatformJobServices {
    /// None represents an unavailable plugin or an import that did not start.
    /// The returned job owns its work and must handle cancellation when dropped.
    fn start(
        &mut self,
        task_id: u32,
        request: PlatformJobRequest<'_>,
    ) -> Result<Option<Box<dyn PlatformJob>>>;
}

/// Window operations supplied by the host after a runtime update.
pub trait WindowServices {
    /// Toggle fullscreen and report the resulting mode (true for fullscreen).
    fn toggle_fullscreen(&mut self) -> Result<bool>;
    fn minimize(&mut self) -> Result<()>;
    /// Return true to accept the requested close.
    fn confirm_close(&mut self) -> bool;
}
