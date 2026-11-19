use katze_system2::{
    Error, Result,
    kcs::ScriptSettings,
    resource::{FolderPaths, XmlDocument},
    settings::{ConfigurationFile, ConfigurationServices},
};
use std::path::{Path, PathBuf};

pub struct ConfigurationFiles {
    pub directory: PathBuf,
    pub sound_name: Vec<u16>,
    pub folders: FolderPaths,
}

impl ConfigurationFiles {
    pub fn new(root: &str, startup: &XmlDocument) -> Result<Self> {
        let (user_data, common_data) = data_directories();
        let mut folders = FolderPaths {
            user_data: path_units(&user_data),
            common_data: path_units(&common_data),
            brand: startup
                .wide_text(b"APP/brand")?
                .unwrap_or_else(|| wide("ういんどみる")),
            title: startup
                .wide_text(b"APP/title")?
                .unwrap_or_else(|| wide("CS2")),
            save: path_units(&user_data.join("ういんどみる/cs2/imglib")),
            imglib: Vec::new(),
        };
        if let Some(folder) = startup.wide_text(b"FOLDER/save")? {
            folders.save = folders.expand(&folder)?;
        }
        let directory = match std::env::var_os("KATZE_SAVE_ROOT") {
            Some(path) => PathBuf::from(path),
            None => Path::new(root).join(native_path(&folders.save)?),
        };
        folders.save = path_units(&directory);
        let sound_name = startup.wide_text(b"SOUNDCONFIG")?.unwrap_or_default();
        Ok(Self {
            directory,
            sound_name,
            folders,
        })
    }

    fn path(&self, file: ConfigurationFile) -> Result<Option<PathBuf>> {
        let name = match file {
            ConfigurationFile::Main => PathBuf::from("setup.xml"),
            ConfigurationFile::Sound if self.sound_name.is_empty() => return Ok(None),
            ConfigurationFile::Sound => native_path(&self.sound_name)?,
        };
        Ok(Some(self.directory.join(name)))
    }

    pub fn read(&self, file: ConfigurationFile) -> Result<Option<Vec<u8>>> {
        let Some(path) = self.path(file)? else {
            return Ok(None);
        };
        let Ok(bytes) = std::fs::read(path) else {
            return Ok(None);
        };
        if XmlDocument::parse(&bytes).is_err() {
            return Ok(None);
        }
        Ok(Some(bytes))
    }

    pub fn read_script_settings(&self) -> Result<ScriptSettings> {
        use std::io::{ErrorKind, Read};
        let path = self.directory.join("script-settings.dat");
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(ScriptSettings::default());
            }
            Err(error) => return Err(Error::new(0, format!("无法打开脚本设置：{error}"))),
        };
        let mut bytes = Vec::new();
        file.take(ScriptSettings::MAX_SNAPSHOT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| Error::new(0, format!("无法读取脚本设置：{error}")))?;
        ScriptSettings::from_bytes(&bytes)
    }
}

impl ConfigurationServices for ConfigurationFiles {
    fn write(&mut self, _: u32, file: ConfigurationFile, bytes: &[u8]) -> Result<bool> {
        let Some(path) = self.path(file)? else {
            return Ok(false);
        };
        let _ = std::fs::create_dir_all(&self.directory);
        Ok(std::fs::write(path, bytes).is_ok())
    }

    fn write_script_settings(&mut self, _: u32, bytes: &[u8]) -> Result<bool> {
        use std::io::Write;
        if std::fs::create_dir_all(&self.directory).is_err() {
            return Ok(false);
        }
        let temporary = self
            .directory
            .join(format!(".script-settings-{}.tmp", std::process::id()));
        let Ok(mut file) = std::fs::File::create_new(&temporary) else {
            return Ok(false);
        };
        let written = file.write_all(bytes).is_ok();
        drop(file);
        if written
            && std::fs::rename(&temporary, self.directory.join("script-settings.dat")).is_ok()
        {
            return Ok(true);
        }
        let _ = std::fs::remove_file(temporary);
        Ok(false)
    }
}

pub(super) fn native_path(value: &[u16]) -> Result<PathBuf> {
    let value = String::from_utf16(value).map_err(|_| Error::new(0, "配置目录包含无效 UTF-16"))?;
    #[cfg(not(target_os = "windows"))]
    if value.as_bytes().get(1) == Some(&b':') {
        return Err(Error::new(
            0,
            "Windows 驱动器目录需要通过 KATZE_SAVE_ROOT 指定平台目录",
        ));
    }
    Ok(PathBuf::from(value.replace('\\', "/")))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

fn path_units(path: &Path) -> Vec<u16> {
    wide(&path.to_string_lossy())
}

fn data_directories() -> (PathBuf, PathBuf) {
    #[cfg(target_os = "windows")]
    {
        (
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_default(),
            std::env::var_os("ProgramData")
                .or_else(|| std::env::var_os("ALLUSERSPROFILE"))
                .map(PathBuf::from)
                .unwrap_or_default(),
        )
    }
    #[cfg(target_os = "macos")]
    {
        (
            std::env::var_os("HOME")
                .map(|path| PathBuf::from(path).join("Library/Application Support"))
                .unwrap_or_default(),
            PathBuf::from("/Library/Application Support"),
        )
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        (
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".local/share"))
                })
                .unwrap_or_default(),
            std::env::var_os("XDG_DATA_DIRS")
                .and_then(|paths| std::env::split_paths(&paths).next())
                .unwrap_or_else(|| PathBuf::from("/usr/local/share")),
        )
    }
}
