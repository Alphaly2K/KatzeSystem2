use katze_system2::{
    Result,
    expression::State,
    kcs::{
        CoreHost, Database, Host, Interpreter, NativeReturn, Program, ResourceHost,
        ResourceServices,
    },
    resource::ResourceMode,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};
use std::io::Write;

fn table() -> Vec<u8> {
    let columns = [
        (b"ID".as_slice(), 4usize),
        (b"INDEX", 32),
        (b"TITLE_S", 32),
        (b"TITLE", 32),
        (b"IND1", 32),
        (b"IND24", 32),
    ];
    let mut plain = (columns.len() as u32).to_le_bytes().to_vec();
    for (name, size) in columns {
        let mut column = [0; 24];
        column[..name.len()].copy_from_slice(name);
        column[20..].copy_from_slice(&(size as u32).to_le_bytes());
        plain.extend(column);
    }
    plain.extend(4u32.to_le_bytes());
    plain.extend(164u32.to_le_bytes());
    for (id, name, term) in [
        (3u32, b"alpha".as_slice(), b"one\\ntwo".as_slice()),
        (7, b"ALPHA", b"upper"),
        (9, b"\x82\xaa", b"kana"),
        (99, b"alpha", b"duplicate"),
    ] {
        let id = id.to_le_bytes();
        for ((_, size), cell) in
            columns
                .into_iter()
                .zip([id.as_slice(), name, b"short", b"title", term, b"last"])
        {
            let mut bytes = vec![0; size];
            bytes[..cell.len()].copy_from_slice(cell);
            plain.extend(bytes);
        }
    }
    pack(&plain)
}

fn pack(plain: &[u8]) -> Vec<u8> {
    let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    zlib.write_all(plain).unwrap();
    let packed = zlib.finish().unwrap();
    let mut file = b"KCDB".to_vec();
    file.extend((packed.len() as u32).to_le_bytes());
    file.extend((plain.len() as u32).to_le_bytes());
    file.extend(packed);
    file
}

struct Files(Option<Vec<u8>>);
impl ResourceServices for Files {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(category, b"database");
        assert_eq!(name, b"words");
        Ok(b"db/words".to_vec())
    }
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(name, b"db/words");
        assert_eq!(mode, ResourceMode::LooseFirst);
        Ok(self.0.clone())
    }
}

fn call(host: &mut impl Host, vm: &mut Interpreter, index: u32, args: &[u32]) -> NativeReturn {
    host.call(
        index,
        &args
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
        vm,
    )
    .unwrap()
}

fn value(result: NativeReturn) -> u32 {
    let NativeReturn::Value(value) = result else {
        panic!("native result expected")
    };
    value
}

fn data() -> Vec<u8> {
    let mut data = vec![0; 256];
    for (offset, text) in [
        (32, b"words".as_slice()),
        (64, b"<br>"),
        (96, b"]"),
        (128, b"["),
    ] {
        data[offset..offset + text.len()].copy_from_slice(text);
    }
    data
}

#[test]
fn database_natives_load_mark_import_and_release_without_losing_buffer_side_effects() {
    let mut state = State::new(0);
    let mut core = CoreHost::new(&mut state);
    let mut files = Files(Some(table()));
    let mut host = ResourceHost::new(&mut core, &mut files);
    let mut vm = Interpreter::new(Program::from_code(&[], data(), 64).unwrap(), vec![]).unwrap();
    let handle = value(call(&mut host, &mut vm, 869, &[32, 64, 96, 128]));
    let database = host.inner.database(handle).unwrap();
    assert_eq!(database.records().count(), 3);
    let alpha = database.record(b"alpha").unwrap();
    assert_eq!(alpha.id, 3);
    assert_eq!(alpha.terms[0].as_deref(), Some(b"one<br>two".as_slice()));
    assert_eq!(alpha.terms[23].as_deref(), Some(b"last".as_slice()));
    assert_eq!(
        database.formatting(),
        &[b"<br>".to_vec(), b"]".to_vec(), b"[".to_vec()]
    );
    vm.write(160, b"\x82\xa9\0").unwrap();
    call(&mut host, &mut vm, 871, &[handle, 33, 160]);
    call(&mut host, &mut vm, 871, &[handle, 1, 160]);
    let record = host
        .inner
        .database(handle)
        .unwrap()
        .record(b"\x82\xaa")
        .unwrap();
    assert_eq!((record.seen, record.pending, record.first_seen), (2, 2, 1));
    assert!(host.inner.database(handle).unwrap().is_dirty());
    call(&mut host, &mut vm, 878, &[handle]);
    assert!(!host.inner.database(handle).unwrap().is_dirty());
    assert_eq!(value(call(&mut host, &mut vm, 872, &[handle, 0])), 808);
    let address = value(call(&mut host, &mut vm, 833, &[1024]));
    call(&mut host, &mut vm, 872, &[handle, address + 8]);
    let bytes = host.inner.raw_buffer(address + 8, 808).unwrap();
    assert_eq!(&bytes[..4], &3u32.to_le_bytes());
    assert_eq!(&bytes[4..10], b"ALPHA\0");
    assert_eq!(&bytes[272..278], b"alpha\0");
    assert_eq!(&bytes[540..543], b"\x82\xa9\0");
    let mut saved = 3u32.to_le_bytes().to_vec();
    for (name, flags) in [
        (b"\x82\xaa".as_slice(), [3u32, 7, 9]),
        (b"\x82\xac", [1, 2, 3]),
        (b"\x82\xa9", [5, 6, 7]),
    ] {
        let mut entry = [0x5a; 268];
        entry[..name.len()].copy_from_slice(name);
        entry[name.len()] = 0;
        for (i, flag) in flags.into_iter().enumerate() {
            entry[256 + i * 4..260 + i * 4].copy_from_slice(&flag.to_le_bytes());
        }
        saved.extend(entry);
    }
    host.inner
        .raw_buffer_mut(address + 8, saved.len())
        .unwrap()
        .copy_from_slice(&saved);
    assert_eq!(
        call(&mut host, &mut vm, 873, &[handle, address + 8]),
        NativeReturn::Value(1)
    );
    let changed = host.inner.raw_buffer(address + 8, saved.len()).unwrap();
    assert_eq!(&changed[4..6], b"\x82\xa9");
    assert_eq!(&changed[272..274], b"\x82\xab");
    assert_eq!(changed[7], 0x5a);
    let database = host.inner.database(handle).unwrap();
    assert!(!database.is_dirty());
    let record = database.record(b"\x82\xa9").unwrap();
    assert_eq!((record.seen, record.pending, record.first_seen), (5, 6, 7));
    assert_eq!(database.record(b"ALPHA").unwrap().seen, 0);
    assert_eq!(
        call(&mut host, &mut vm, 873, &[handle, 0]),
        NativeReturn::Value(0)
    );
    call(&mut host, &mut vm, 871, &[0]);
    for index in [872u32, 873] {
        assert!(
            host.call(index, &[0; 8], &mut vm)
                .unwrap_err()
                .message
                .contains("未初始化返回值")
        );
    }
    call(&mut host, &mut vm, 870, &[handle]);
    assert!(host.inner.database(handle).is_err());
}

#[test]
fn database_parser_rejects_invalid_layouts_and_preserves_empty_loads() {
    let formatting = || std::array::from_fn(|_| Vec::new());
    assert_eq!(
        Database::from_bytes(b"bad", formatting())
            .unwrap()
            .records()
            .count(),
        0
    );
    assert!(Database::from_bytes(b"KCDB", formatting()).is_err());
    let mut plain = 0u32.to_le_bytes().to_vec();
    plain.extend(0u32.to_le_bytes());
    plain.extend(0u32.to_le_bytes());
    assert_eq!(
        Database::from_bytes(&pack(&plain), formatting())
            .unwrap()
            .records()
            .count(),
        0
    );
    plain[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(Database::from_bytes(&pack(&plain), formatting()).is_err());
}

struct Loader(Program);
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

#[test]
fn runtime_registers_database_loading_and_keeps_handles_across_budget_continuations() {
    let mut code = Vec::new();
    for value in [32u32, 64, 96, 128] {
        code.extend([0x25, 3]);
        code.extend(value.to_le_bytes());
    }
    code.extend([0x2b, 3]);
    code.extend(869u32.to_le_bytes());
    code.push(3);
    code.extend(16u32.to_le_bytes());
    code.extend([0x26, 3, 0, 0, 0, 0, 0x3b]);
    for bytes in [Some(table()), None] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Loader(Program::from_code(&code, data(), 64).unwrap())),
        );
        runtime.set_resources(Files(bytes.clone()));
        let id = runtime.create(0, kind::ORDINARY, b"", b"").unwrap();
        for now in 1..30 {
            runtime.update(now, 1).unwrap();
        }
        let vm = runtime.object(id).unwrap().interpreter().unwrap();
        let handle = u32::from_le_bytes(vm.read(0, 4).unwrap().as_ref().try_into().unwrap());
        assert_eq!(
            runtime.host().database(handle).unwrap().records().count(),
            if bytes.is_some() { 3 } else { 0 }
        );
    }
}
