use katze_system2::{
    Error, Result,
    runtime::{ExportDestination, ExportServices},
};
use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
};

#[derive(Clone)]
pub struct Directory(PathBuf);

impl Directory {
    pub fn new(path: PathBuf) -> Result<Self> {
        let path = path.canonicalize().map_err(io_error)?;
        if !path.is_dir() {
            return Err(Error::new(0, "导出目标需要现有目录"));
        }
        Ok(Self(path))
    }

    fn locate(&self, parts: &[String]) -> Result<PathBuf> {
        let mut path = self.0.clone();
        for part in &parts[..parts.len() - 1] {
            path.push(part);
            if !path.symlink_metadata().map_err(io_error)?.is_dir() {
                return Err(Error::new(0, "导出目录项不是普通目录"));
            }
        }
        path.push(parts.last().unwrap());
        Ok(path)
    }
}

impl ExportServices for Directory {
    fn select_destination(&mut self, _: u32) -> Result<Option<Box<dyn ExportDestination>>> {
        Ok(Some(Box::new(self.clone())))
    }

    fn finished(&mut self, task: u32, result: Result<()>) {
        if let Err(error) = result {
            eprintln!("导出任务 {task}：{error}");
        }
    }
}

impl ExportDestination for Directory {
    fn create_directory(&mut self, parts: &[String]) -> Result<()> {
        let path = self.locate(parts)?;
        match fs::create_dir(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if path.symlink_metadata().map_err(io_error)?.is_dir() {
                    Ok(())
                } else {
                    Err(io_error(error))
                }
            }
            Err(error) => Err(io_error(error)),
        }
    }

    fn write_file(&mut self, parts: &[String], bytes: &[u8]) -> Result<()> {
        let path = self.locate(parts)?;
        let mut file = File::create_new(&path).map_err(io_error)?;
        if let Err(error) = file.write_all(bytes) {
            drop(file);
            let _ = fs::remove_file(path);
            return Err(io_error(error));
        }
        Ok(())
    }
}

fn io_error(error: std::io::Error) -> Error {
    Error::new(0, format!("导出目录或文件访问失败：{error}"))
}
