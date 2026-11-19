use katze_system2::{
    Error, Result,
    host::{PlatformJob, PlatformJobRequest, PlatformJobServices},
    resource::XmlDocument,
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    rc::{Rc, Weak},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
};

pub struct Import {
    source: Source,
    directory: PathBuf,
}

enum Source {
    File(PathBuf, bool),
    Picker(ImportPicker),
}

#[derive(Clone, Default)]
pub struct ImportPicker(Rc<RefCell<PickerState>>);

#[derive(Default)]
struct PickerState {
    requests: VecDeque<ImportRequest>,
    last_directory: Option<PathBuf>,
}

pub struct ImportRequest(Weak<Cell<Selection>>);

enum Selection {
    Waiting,
    Cancelled,
    File(PathBuf, bool),
}

impl ImportRequest {
    pub fn active(&self) -> bool {
        self.0.strong_count() != 0
    }

    pub fn complete(self, file: Option<(PathBuf, bool)>) {
        if let Some(selection) = self.0.upgrade() {
            selection.set(file.map_or(Selection::Cancelled, |(path, overwrite)| {
                Selection::File(path, overwrite)
            }));
        }
    }
}

impl ImportPicker {
    pub fn next_request(&self) -> Option<ImportRequest> {
        let mut state = self.0.borrow_mut();
        while let Some(request) = state.requests.pop_front() {
            if request.active() {
                return Some(request);
            }
        }
        None
    }

    pub fn take_last_directory(&self) -> Option<PathBuf> {
        self.0.borrow_mut().last_directory.take()
    }
}

impl Import {
    pub(super) fn from_startup(root: &str, startup: &XmlDocument) -> Result<Option<Self>> {
        let Some(source) = std::env::var_os("KATZE_IMPORT_FILE") else {
            return Ok(None);
        };
        let directory = import_directory(root, startup)?
            .ok_or_else(|| Error::new(0, "图片导入缺少 CURSOR/default 或 KATZE_IMPORT_ROOT"))?;
        Ok(Some(Self {
            source: Source::File(
                source.into(),
                std::env::var_os("KATZE_IMPORT_OVERWRITE").is_some_and(|value| value == "1"),
            ),
            directory,
        }))
    }

    pub fn interactive(directory: PathBuf, picker: ImportPicker) -> Self {
        Self {
            source: Source::Picker(picker),
            directory,
        }
    }
}

pub fn import_directory(root: &str, startup: &XmlDocument) -> Result<Option<PathBuf>> {
    if let Some(path) = std::env::var_os("KATZE_IMPORT_ROOT") {
        return Ok(Some(PathBuf::from(path)));
    }
    startup
        .wide_text(b"CURSOR/default")?
        .filter(|path| !path.is_empty())
        .map(|path| super::configuration::native_path(&path).map(|path| Path::new(root).join(path)))
        .transpose()
}

impl PlatformJobServices for Import {
    fn start(
        &mut self,
        task: u32,
        request: PlatformJobRequest<'_>,
    ) -> Result<Option<Box<dyn PlatformJob>>> {
        if request != PlatformJobRequest::ImageImport {
            return Ok(None);
        }
        let job: Box<dyn PlatformJob> = match &self.source {
            Source::File(path, overwrite) => Box::new(Job::start(
                task,
                path.clone(),
                self.directory.clone(),
                *overwrite,
            )?),
            Source::Picker(picker) => {
                let selection = Rc::new(Cell::new(Selection::Waiting));
                picker
                    .0
                    .borrow_mut()
                    .requests
                    .push_back(ImportRequest(Rc::downgrade(&selection)));
                Box::new(PickJob {
                    task,
                    directory: self.directory.clone(),
                    selection,
                    picker: picker.clone(),
                    copying: None,
                })
            }
        };
        Ok(Some(job))
    }
}

struct PickJob {
    task: u32,
    directory: PathBuf,
    picker: ImportPicker,
    selection: Rc<Cell<Selection>>,
    copying: Option<(Job, PathBuf)>,
}

impl PlatformJob for PickJob {
    fn poll(&mut self) -> Option<u32> {
        if let Some((job, source_directory)) = &mut self.copying {
            let result = job.poll()?;
            if result != 0 {
                self.picker.0.borrow_mut().last_directory = Some(std::mem::take(source_directory));
            }
            return Some(result);
        }
        match self.selection.replace(Selection::Waiting) {
            Selection::Waiting => None,
            Selection::Cancelled => Some(0),
            Selection::File(source, overwrite) => {
                let source_directory = source.parent().unwrap_or(Path::new("")).to_owned();
                match Job::start(self.task, source, self.directory.clone(), overwrite) {
                    Ok(job) => {
                        self.copying = Some((job, source_directory));
                        None
                    }
                    Err(error) => {
                        eprintln!("{error}");
                        Some(0)
                    }
                }
            }
        }
    }
}

struct Job {
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<bool>>,
}

impl Job {
    fn start(task: u32, source: PathBuf, directory: PathBuf, overwrite: bool) -> Result<Self> {
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker = thread::Builder::new()
            .spawn(
                move || match copy(&source, &directory, overwrite, &worker_cancel) {
                    Ok(copied) => copied,
                    Err(error) => {
                        eprintln!("图片导入任务 {task}：{error}");
                        false
                    }
                },
            )
            .map_err(|error| Error::new(0, format!("图片导入工作线程启动失败：{error}")))?;
        Ok(Self {
            cancel,
            worker: Some(worker),
        })
    }
}

impl PlatformJob for Job {
    fn poll(&mut self) -> Option<u32> {
        if !self.worker.as_ref()?.is_finished() {
            return None;
        }
        Some(u32::from(self.worker.take()?.join().unwrap_or(false)))
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            self.cancel.store(true, Ordering::Relaxed);
            let _ = worker.join();
        }
    }
}

pub fn is_image_path(source: &Path) -> bool {
    source
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ["bmp", "png", "jpg"]
                .iter()
                .any(|kind| ext.eq_ignore_ascii_case(kind))
        })
}

fn copy(source: &Path, directory: &Path, overwrite: bool, cancel: &AtomicBool) -> io::Result<bool> {
    if !is_image_path(source) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "导入支持 BMP、PNG 和 JPG",
        ));
    }
    if !fs::metadata(source)?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "导入来源不是普通文件",
        ));
    }
    let mut source_file = File::open(source)?;
    if cancel.load(Ordering::Relaxed) {
        return Ok(false);
    }
    fs::create_dir_all(directory)?;
    let destination = directory.join(source.file_name().unwrap());
    if overwrite
        && fs::canonicalize(&destination)
            .is_ok_and(|dest| fs::canonicalize(source).is_ok_and(|source| source == dest))
    {
        return Ok(false);
    }
    let (path, mut output) = if overwrite {
        static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
        let (path, file) = loop {
            let seq = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = directory.join(format!(".image-import-{}-{seq}.tmp", std::process::id()));
            match File::create_new(&path) {
                Ok(file) => break (path, file),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        };
        (path, file)
    } else {
        let output = match File::create_new(&destination) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(false),
            Err(error) => return Err(error),
        };
        (destination.clone(), output)
    };
    let copied = (|| {
        let mut buffer = [0; 64 * 1024];
        while !cancel.load(Ordering::Relaxed) {
            let count = source_file.read(&mut buffer)?;
            if count == 0 {
                output.flush()?;
                drop(output);
                if cancel.load(Ordering::Relaxed) {
                    return Ok(false);
                }
                if overwrite {
                    fs::rename(&path, &destination)?;
                }
                return Ok(true);
            }
            output.write_all(&buffer[..count])?;
        }
        Ok(false)
    })();
    if !matches!(copied, Ok(true)) {
        let _ = fs::remove_file(path);
    }
    copied
}

#[cfg(test)]
mod tests {
    use super::*;
    use katze_system2::{
        runtime::{TaskRuntime, kind},
        task::Command,
    };

    #[test]
    fn import_creates_parents_and_replaces_only_when_allowed() -> io::Result<()> {
        struct Scratch(PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        let root = std::env::temp_dir().join(format!(
            "katze-import-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root)?;
        let root = Scratch(root);
        let source = root.0.join("image.PNG");
        let directory = root.0.join("nested/imports");
        let target = directory.join("image.PNG");
        let cancel = AtomicBool::new(true);
        fs::write(&source, b"selected bytes")?;
        assert!(!copy(&source, &directory, false, &cancel)?);
        assert!(!directory.exists());
        cancel.store(false, Ordering::Relaxed);
        assert!(copy(&source, &directory, false, &cancel)?);
        fs::write(&source, b"replacement bytes")?;
        assert!(!copy(&source, &directory, false, &cancel)?);
        assert_eq!(fs::read(&target)?, b"selected bytes");
        let mut runtime = TaskRuntime::with_resource_programs(0, Default::default()).unwrap();
        let picker = ImportPicker::default();
        runtime.set_platform_job_services(Import::interactive(directory.clone(), picker.clone()));
        let task = runtime.create(0, kind::IMAGE_IMPORT, b"", b"").unwrap();
        let command = |code| Command {
            target: task,
            sender: 0,
            code,
            data: [0; 3],
        };
        runtime.dispatch_command(0, command(576)).unwrap();
        assert_eq!(runtime.dispatch_command(0, command(577)).unwrap(), 0);
        picker
            .next_request()
            .unwrap()
            .complete(Some((source, true)));
        assert!(picker.take_last_directory().is_none());
        let start = std::time::Instant::now();
        while runtime.dispatch_command(0, command(577)).unwrap() == 0 {
            assert!(start.elapsed() < std::time::Duration::from_secs(5));
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(fs::read(&target)?, b"replacement bytes");
        assert_eq!(
            picker.take_last_directory().as_deref(),
            Some(root.0.as_path())
        );
        assert!(!copy(&target, &directory, true, &cancel)?);
        assert_eq!(fs::read_dir(&directory)?.count(), 1);

        let cancelled = runtime.create(0, kind::IMAGE_IMPORT, b"", b"").unwrap();
        let cancelled_command = |code| Command {
            target: cancelled,
            ..command(code)
        };
        runtime.dispatch_command(0, cancelled_command(576)).unwrap();
        picker.next_request().unwrap().complete(None);
        assert_eq!(
            runtime.dispatch_command(0, cancelled_command(577)).unwrap(),
            1
        );
        assert!(picker.take_last_directory().is_none());

        let deleted = runtime.create(0, kind::IMAGE_IMPORT, b"", b"").unwrap();
        runtime
            .dispatch_command(
                0,
                Command {
                    target: deleted,
                    ..command(576)
                },
            )
            .unwrap();
        runtime.destroy(deleted).unwrap();
        assert!(picker.next_request().is_none());
        assert_eq!(fs::read(&target)?, b"replacement bytes");
        Ok(())
    }
}
