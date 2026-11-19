use katze_system2::{
    Error, Result,
    resource::XmlDocument,
    runtime::{PlatformJob, PlatformJobRequest, PlatformJobServices},
};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
};

pub(super) struct Import {
    source: PathBuf,
    directory: PathBuf,
    overwrite: bool,
}

impl Import {
    pub(super) fn from_startup(root: &str, startup: &XmlDocument) -> Result<Option<Self>> {
        let Some(source) = std::env::var_os("KATZE_IMPORT_FILE") else {
            return Ok(None);
        };
        let directory = if let Some(path) = std::env::var_os("KATZE_IMPORT_ROOT") {
            PathBuf::from(path)
        } else {
            let path = startup
                .wide_text(b"CURSOR/default")?
                .filter(|path| !path.is_empty())
                .ok_or_else(|| Error::new(0, "图片导入缺少 CURSOR/default 或 KATZE_IMPORT_ROOT"))?;
            Path::new(root).join(super::configuration::native_path(&path)?)
        };
        Ok(Some(Self {
            source: source.into(),
            directory,
            overwrite: std::env::var_os("KATZE_IMPORT_OVERWRITE").is_some_and(|value| value == "1"),
        }))
    }
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
        let source = self.source.clone();
        let directory = self.directory.clone();
        let overwrite = self.overwrite;
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
        Ok(Some(Box::new(Job {
            cancel,
            worker: Some(worker),
        })))
    }
}

struct Job {
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<bool>>,
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

fn copy(source: &Path, directory: &Path, overwrite: bool, cancel: &AtomicBool) -> io::Result<bool> {
    let accepted = source
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ["bmp", "png", "jpg"]
                .iter()
                .any(|kind| ext.eq_ignore_ascii_case(kind))
        });
    if !accepted {
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
        expression::State,
        runtime::{BaseObject, KcsFactory, Runtime, kind},
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
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(super::super::Programs {
                programs: Default::default(),
            }),
        );
        runtime.set_platform_job_services(Import {
            source,
            directory: directory.clone(),
            overwrite: true,
        });
        let task = runtime.create(0, kind::IMAGE_IMPORT, b"", b"").unwrap();
        let command = |code| Command {
            target: task,
            sender: 0,
            code,
            data: [0; 3],
        };
        runtime.dispatch_command(0, command(576)).unwrap();
        let start = std::time::Instant::now();
        while runtime.dispatch_command(0, command(577)).unwrap() == 0 {
            assert!(start.elapsed() < std::time::Duration::from_secs(5));
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(fs::read(&target)?, b"replacement bytes");
        assert!(!copy(&target, &directory, true, &cancel)?);
        assert_eq!(fs::read_dir(&directory)?.count(), 1);
        Ok(())
    }
}
