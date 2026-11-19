use crate::Result;

/// Receives unpacked directories and files. Paths contain components relative
/// to the destination selected by the host.
pub trait ExportDestination: Send {
    fn create_directory(&mut self, path: &[String]) -> Result<()>;
    fn write_file(&mut self, path: &[String], bytes: &[u8]) -> Result<()>;
}

/// Selects a destination and receives the result of a background export.
pub trait ExportServices {
    /// `None` cancels the export. The runtime calls this on the session thread
    /// and may move the destination to its worker thread.
    fn select_destination(&mut self, task_id: u32) -> Result<Option<Box<dyn ExportDestination>>>;

    fn finished(&mut self, _task_id: u32, _result: Result<()>) {}
}
