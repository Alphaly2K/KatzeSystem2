use super::SharedSource;
use crate::{Error, Result, read, resource};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Receives unpacked directories and files. Paths contain components relative
/// to the destination selected by the host.
pub trait ExportDestination: Send {
    fn create_directory(&mut self, path: &[String]) -> Result<()>;
    fn write_file(&mut self, path: &[String], bytes: &[u8]) -> Result<()>;
}

/// A single export prepared by the runtime and scheduled by the host.
pub struct ExportWork {
    source: SharedSource,
    destination: Box<dyn ExportDestination>,
    cancelled: Arc<AtomicBool>,
}

impl ExportWork {
    pub(crate) fn new(
        source: SharedSource,
        destination: Box<dyn ExportDestination>,
        cancelled: Arc<AtomicBool>,
    ) -> Self {
        Self {
            source,
            destination,
            cancelled,
        }
    }

    /// Read and unpack the source. When the script task is dropped, pending
    /// work is skipped and active work stops between destination operations.
    pub fn run(mut self) -> Result<()> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Ok(());
        }
        let size =
            u32::try_from(self.source.len()?).map_err(|_| Error::new(0, "导出包长度溢出"))?;
        let mut bytes = vec![0; read::sized(size, 0)?];
        self.source.read_exact_at(0, &mut bytes)?;
        resource::export::unpack(&bytes, self.destination.as_mut(), &self.cancelled)
    }
}

/// Completion handle polled on the session thread.
pub trait ExportJob {
    /// Return `None` while pending. Polling must not wait for work to finish.
    fn poll(&mut self) -> Option<Result<()>>;
}

/// Selects a destination, schedules unpacking, and receives completion.
pub trait ExportServices {
    /// `None` cancels the export before the resource is opened.
    fn select_destination(&mut self, task_id: u32) -> Result<Option<Box<dyn ExportDestination>>>;

    /// The default executes on the session thread. To run work elsewhere,
    /// schedule [`ExportWork::run`] and return a handle for its result.
    fn start(&mut self, _task_id: u32, work: ExportWork) -> Result<Box<dyn ExportJob>> {
        Ok(Box::new(CompletedExport(Some(work.run()))))
    }

    fn finished(&mut self, _task_id: u32, _result: Result<()>) {}
}

struct CompletedExport(Option<Result<()>>);

impl ExportJob for CompletedExport {
    fn poll(&mut self) -> Option<Result<()>> {
        self.0.take()
    }
}
