use katze_system2::kcs::{Interpreter, Program, SharedMemory};

#[test]
fn shared_subviews_keep_aliases_alive_and_require_releasing_reads_before_writes() {
    let storage = SharedMemory::new((0..32).collect()).unwrap();
    let program = Program::from_code(&[], vec![], 0).unwrap();
    let mut first = Interpreter::new(program.clone(), vec![]).unwrap();
    let mut second = Interpreter::new(program, vec![]).unwrap();
    first.bind_external(storage.slice(4, 20).unwrap());
    second.bind_external(storage.slice(8, 8).unwrap());
    first.write(0x8000_0004, b"abc\0").unwrap();
    assert_eq!(second.read_string(0x8000_0000).unwrap(), b"abc");
    let read = first.read(0x8000_0004, 4).unwrap();
    assert!(second.write(0x8000_0000, b"def\0").is_err());
    assert_eq!(read, b"abc\0");
    drop(read);
    second.write(0x8000_0000, b"def\0").unwrap();
    assert_eq!(first.read_string(0x8000_0004).unwrap(), b"def");
    assert!(storage.slice(31, 2).is_err());
    assert!(storage.slice(usize::MAX, 1).is_err());
    assert!(second.read(0x8000_0008, 1).is_err());
    assert!(second.write(0x8000_0007, b"xx").is_err());
    assert!(second.read(0x8000_0008, 0).unwrap().is_empty());
    let previous = first.replace_external(vec![9; 3]).unwrap();
    assert_eq!(&previous[4..8], b"def\0");
    assert_eq!(first.read(0x8000_0000, 3).unwrap(), [9; 3]);
    drop(storage);
    drop(first);
    assert_eq!(second.read_string(0x8000_0000).unwrap(), b"def");
}
