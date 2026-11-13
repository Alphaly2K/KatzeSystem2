use katze_system2::{
    Result,
    io::{FileSource, ReadSource, SourceReader},
};
use std::io::{Read, Seek, SeekFrom, Write};

struct Chunked(&'static [u8]);
impl ReadSource for Chunked {
    fn len(&self) -> Result<u64> {
        ReadSource::len(self.0)
    }

    fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize> {
        let count = output.len().min(3);
        self.0.read_at(offset, &mut output[..count])
    }
}

#[test]
fn readers_keep_independent_positions_short_reads_and_failed_seek_state() {
    let source: katze_system2::io::SharedSource = std::sync::Arc::new(Chunked(b"0123456789abcdef"));
    let mut first = SourceReader::new(source.as_ref());
    let mut second = SourceReader::from_shared(source.clone());
    let mut bytes = [0; 6];
    first.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"012345");
    second.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"012345");
    first.seek(SeekFrom::End(-3)).unwrap();
    let mut tail = [0; 3];
    first.read_exact(&mut tail).unwrap();
    assert_eq!(&tail, b"def");
    assert!(first.seek(SeekFrom::Current(-17)).is_err());
    assert_eq!(first.stream_position().unwrap(), 16);
    assert_eq!(first.read(&mut bytes).unwrap(), 0);
    assert_eq!(second.stream_position().unwrap(), 6);
    assert_eq!(source.read_exact_at(14, &mut bytes).unwrap_err().offset, 16);
    assert_eq!(&bytes[..2], b"ef");
    source.read_exact_at(u64::MAX, &mut []).unwrap();
}

#[test]
fn file_source_reads_concurrently_without_sharing_reader_positions() {
    use std::sync::{
        Barrier,
        atomic::{AtomicUsize, Ordering},
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Scratch(std::path::PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let path = std::env::temp_dir().join(format!(
        "katze-source-{}-{}.bin",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    let scratch = Scratch(path);
    let bytes: Vec<_> = (0..4096).map(|i| i as u8).collect();
    file.write_all(&bytes).unwrap();
    drop(file);
    let source = FileSource::open(&scratch.0).unwrap();
    assert_eq!(source.len().unwrap(), bytes.len() as u64);
    let barrier = Barrier::new(2);
    std::thread::scope(|scope| {
        for worker in 0..2 {
            let source = &source;
            let bytes = &bytes;
            let barrier = &barrier;
            scope.spawn(move || {
                barrier.wait();
                for iteration in 0..64 {
                    let offset = iteration * 37 + worker * 11;
                    let mut actual = [0; 12];
                    source.read_exact_at(offset as u64, &mut actual).unwrap();
                    assert_eq!(&actual, &bytes[offset..offset + actual.len()]);
                }
            });
        }
    });
}
