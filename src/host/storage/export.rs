use super::{SharedSource, source::read_exact_while};
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

    /// Read and unpack the source. Deleting the script task cancels pending
    /// work. Active work observes cancellation between source reads and
    /// destination operations. Calls already in progress can finish.
    pub fn run(mut self) -> Result<()> {
        let cancelled = || self.cancelled.load(Ordering::Relaxed);
        if cancelled() {
            return Ok(());
        }
        let size =
            u32::try_from(self.source.len()?).map_err(|_| Error::new(0, "导出包长度溢出"))?;
        if cancelled() {
            return Ok(());
        }
        let mut bytes = vec![0; read::sized(size, 0)?];
        let complete = read_exact_while(&*self.source, 0, &mut bytes, 64 * 1024, || !cancelled())?;
        if complete && !cancelled() {
            resource::export::unpack(&bytes, self.destination.as_mut(), &self.cancelled)?;
        }
        Ok(())
    }
}

/// Completion handle polled and dropped on the session thread. Dropping an
/// active job after cancellation should release the handle without waiting
/// for any in-progress I/O.
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

    /// Called when a live export task observes completion or a startup error.
    fn finished(&mut self, _task_id: u32, _result: Result<()>) {}
}

struct CompletedExport(Option<Result<()>>);

impl ExportJob for CompletedExport {
    fn poll(&mut self) -> Option<Result<()>> {
        self.0.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::storage::ReadSource;

    #[test]
    fn cancelled_work_stops_after_a_short_host_read() -> Result<()> {
        struct InterruptedSource(Arc<AtomicBool>);

        impl ReadSource for InterruptedSource {
            fn len(&self) -> Result<u64> {
                Ok(256 * 1024)
            }

            fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize> {
                assert_eq!(offset, 0);
                assert!(output.len() < self.len()? as usize);
                output[0] = 0;
                self.0.store(true, Ordering::Relaxed);
                Ok(1)
            }
        }

        struct NoWrites;
        impl ExportDestination for NoWrites {
            fn create_directory(&mut self, _: &[String]) -> Result<()> {
                panic!("cancelled export attempted to create a directory")
            }

            fn write_file(&mut self, _: &[String], _: &[u8]) -> Result<()> {
                panic!("cancelled export attempted to write a file")
            }
        }

        let cancelled = Arc::new(AtomicBool::new(false));
        let source = Arc::new(InterruptedSource(cancelled.clone()));
        ExportWork::new(source, Box::new(NoWrites), cancelled.clone()).run()?;
        assert!(cancelled.load(Ordering::Relaxed));
        Ok(())
    }
}
