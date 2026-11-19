use super::file_source::FileSource;
use katze_system2::{Error, message::Fonts};
use std::{collections::BTreeSet, fs::File, io::ErrorKind, path::PathBuf, sync::Arc};

pub fn load_fonts(resource_root: impl Into<PathBuf>) -> Fonts {
    let root = resource_root.into();
    let mut fonts = Fonts::new();
    fonts.set_resource_loader(move |name| {
        let (name, _, invalid) = encoding_rs::SHIFT_JIS.decode(name);
        if invalid || name.contains('\0') {
            return Err(Error::new(0, "字体资源名称不是有效 CP932 路径"));
        }
        match File::open(root.join(name.replace('\\', "/"))) {
            Ok(file) => Ok(Some(Arc::new(FileSource::from_file(file)?))),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Error::new(0, format!("字体资源读取失败：{error}"))),
        }
    });

    let timing = std::env::var_os("KATZE_TRACE_TIMING").map(|_| std::time::Instant::now());
    let mut database = fontdb::Database::new();
    database.load_system_fonts();
    let paths: BTreeSet<_> = database
        .faces()
        .filter_map(|face| match &face.source {
            fontdb::Source::File(path) => Some(path.clone()),
            _ => None,
        })
        .collect();
    let mut faces = 0;
    for path in paths {
        faces += fonts
            .add_fallback_source(move || Ok(Arc::new(FileSource::open(&path)?)))
            .unwrap_or(0);
    }
    if let Some(started) = timing {
        eprintln!(
            "[font-discovery] faces={faces} ms={:.2}",
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
    fonts
}
