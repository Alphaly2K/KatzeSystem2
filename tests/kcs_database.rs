use katze_system2::{
    Result,
    expression::State,
    kcs::{
        CoreHost, Database, Host, HostState, Interpreter, NativeReturn, Program, ResourceHost,
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

fn dictionary_runtime(budget: usize) -> Runtime<KcsFactory<Loader>> {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(64),
        KcsFactory::new(Loader(Program::from_code(&[], vec![], 0).unwrap())),
    );
    runtime.set_budget(budget, 10000);
    let mut numbers = State::new(0);
    let mut core = CoreHost::new(&mut numbers);
    core.host = HostState::with_handles(runtime.host().handle_allocator());
    let database =
        Database::from_bytes(&table(), [b"<br>".to_vec(), b"]".to_vec(), b"[".to_vec()]).unwrap();
    let handle = core.create_database(database).unwrap();
    let mut vm = Interpreter::new(
        Program::from_code(&[], b"alpha\0\x82\xaa\0".to_vec(), 64).unwrap(),
        vec![],
    )
    .unwrap();
    for bit in [0, 23, 30] {
        call(&mut core, &mut vm, 871, &[handle, bit, 0]);
    }
    call(&mut core, &mut vm, 871, &[handle, 0, 6]);
    *runtime.host_mut() = core.host;
    runtime.numbers_mut().shared[0] = handle as i32;
    runtime
}

fn update_dictionary(
    runtime: &mut Runtime<KcsFactory<Loader>>,
    scene: &mut katze_system2::runtime::fes::Scene,
) -> Result<()> {
    for _ in 0..256 {
        if runtime.update_fes_scene(0, scene, 100)? {
            return Ok(());
        }
    }
    panic!("dictionary scene did not finish");
}

#[test]
fn dictionary_queries_share_native_marks_across_main_system_and_event_handlers() {
    use katze_system2::{fes, runtime::fes::Scene};
    let source = b"#DEFINE\nBASE obj\n#START\ndic_appear $0 1 alpha\ndic_max_appear $0 2 alpha\ndic_id $0 3 alpha\ndic_ind $0 4 alpha\ndic_title $0 5 6 alpha\ndic_update $0 7 8 alpha\ndic_find $0 9 10 0 A z\ndic_title $0 11 11 alpha\ndic_update $0 12 12 alpha\ndic_title $0 13 14 absent\ndic_ind $0 15 absent\ndic_find $0 16 17 99 A z\nwait 999\n#_FES_SLEEP_\ndic_update_clear $0 alpha\ndic_update $0 18 19 alpha\ndic_appear $0 20 alpha\n#obj.PUSH_L\ndic_title $0 21 21 $str99;\ndic_update_clear $0\ndic_update $0 22 23 $str99;\ndic_appear $0 24 $str99;\n";
    for budget in [1, 4096] {
        let mut runtime = dictionary_runtime(budget);
        runtime
            .numbers_mut()
            .strings
            .insert(99, b"\x82\xa9".to_vec());
        let mut scene = Scene::new(
            fes::Program::from_text(source).unwrap(),
            runtime.host().handle_allocator(),
        )
        .unwrap();
        update_dictionary(&mut runtime, &mut scene).unwrap();
        assert_eq!(&runtime.numbers().shared[1..4], &[0x40800001, 0x800001, 2]);
        assert_eq!(&runtime.numbers().shared[7..10], &[1, 0x40800001, 1]);
        assert_eq!(runtime.numbers().shared[12], 0x40800001);
        assert_eq!(runtime.numbers().shared[16], 0);
        for (index, text) in [
            (4, b"[one<br>two][last]".as_slice()),
            (5, b"title"),
            (6, b"short"),
            (10, b"ALPHA"),
            (11, b"short"),
            (13, b""),
            (14, b""),
            (15, b""),
            (17, b""),
        ] {
            assert_eq!(runtime.numbers().strings[&index], text);
        }
        for _ in 0..256 {
            if runtime
                .dispatch_fes_system_block(0, &mut scene, "_FES_SLEEP_")
                .unwrap()
            {
                break;
            }
        }
        assert_eq!(&runtime.numbers().shared[18..21], &[0, 0, 0x40800001]);
        let target = fes::ObjectReference {
            name: "obj".into(),
            bank: 0,
        };
        let mut finished = false;
        for _ in 0..256 {
            if runtime
                .dispatch_fes_event(0, &mut scene, Some(&target), 2)
                .unwrap()
                .is_some()
            {
                finished = true;
                break;
            }
        }
        assert!(finished);
        assert_eq!(runtime.numbers().strings[&21], b"short");
        assert_eq!(&runtime.numbers().shared[22..25], &[0, 0, 1]);
        assert!(
            runtime
                .host()
                .database(runtime.numbers().shared[0] as u32)
                .unwrap()
                .is_dirty()
        );
    }
}

#[test]
fn dictionary_missing_arguments_keep_numeric_effects_and_zero_find_skips_outputs() {
    use katze_system2::{fes, runtime::fes::Scene};
    let source = b"#START\ndic_find 0 ($10+=100) ($10+=100) ($10+=100) A z\ndic_title 9999 ($11+=1)\ndic_update $0 ($12+=1) ($13+=1)\ndic_appear $0 ($14+=1)\ndic_id\nwait 999\n#_FES_SLEEP_\ndic_appear 9999 ($15+=1) alpha\n";
    for budget in [1, 4096] {
        let mut runtime = dictionary_runtime(budget);
        let mut scene = Scene::new(
            fes::Program::from_text(source).unwrap(),
            runtime.host().handle_allocator(),
        )
        .unwrap();
        update_dictionary(&mut runtime, &mut scene).unwrap();
        assert_eq!(&runtime.numbers().shared[10..16], &[0, 1, 1, 1, 1, 0]);
        let mut error = None;
        for _ in 0..256 {
            match runtime.dispatch_fes_system_block(0, &mut scene, "_FES_SLEEP_") {
                Err(found) => {
                    error = Some(found);
                    break;
                }
                Ok(false) => (),
                Ok(true) => panic!("invalid database accepted"),
            }
        }
        assert!(error.unwrap().message.contains("数据库句柄无效"));
        assert_eq!(runtime.numbers().shared[15], 1);
    }
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

#[test]
fn raw_kcdb_queries_preserve_field_width_and_vm_bytes() {
    let columns = [
        (b"Num".as_slice(), &b"\x9a\xbc\xde"[..]),
        (b"Blob", &b"a\0b\xffc"[..]),
        (b"Num", &b"\x77"[..]),
    ];
    let mut plain = (columns.len() as u32).to_le_bytes().to_vec();
    for (name, cell) in columns {
        let mut column = [0; 24];
        column[..name.len()].copy_from_slice(name);
        column[20..].copy_from_slice(&(cell.len() as u32).to_le_bytes());
        plain.extend(column);
    }
    plain.extend(1u32.to_le_bytes());
    plain.extend(9u32.to_le_bytes());
    for (_, cell) in columns {
        plain.extend(cell);
    }

    let mut state = State::new(0);
    let mut core = CoreHost::new(&mut state);
    let mut files = Files(Some(pack(&plain)));
    let mut host = ResourceHost::new(&mut core, &mut files);
    let mut data = data();
    data[64..72].copy_from_slice(b"Blob\0\0\0\0");
    data[96..100].copy_from_slice(b"num\0");
    let mut external = vec![0xa5; 16];
    external[..4].copy_from_slice(b"Num\0");
    let mut vm = Interpreter::new(Program::from_code(&[], data, 64).unwrap(), external).unwrap();
    let handle = value(call(&mut host, &mut vm, 791, &[32]));
    assert_ne!(handle, 0);
    assert_eq!(
        value(call(&mut host, &mut vm, 793, &[handle, 0x8000_0000, 0])),
        0x00de_bc9a
    );
    for (name, row) in [(64, 0), (96, 0), (0x8000_0000, 1)] {
        assert_eq!(
            value(call(&mut host, &mut vm, 793, &[handle, name, row])),
            0
        );
    }
    assert_eq!(
        value(call(&mut host, &mut vm, 794, &[handle, 64, 0, 0x8000_0004])),
        5
    );
    assert_eq!(
        vm.read(0x8000_0004, 8).unwrap().as_ref(),
        b"a\0b\xffc\xa5\xa5\xa5"
    );
    assert_eq!(
        value(call(
            &mut host,
            &mut vm,
            794,
            &[handle, u32::MAX, 0, u32::MAX]
        )),
        0
    );
    assert!(matches!(
        call(&mut host, &mut vm, 792, &[handle]),
        NativeReturn::Continue
    ));
    let args: Vec<_> = [handle, 64, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    assert!(host.call(793, &args, &mut vm).is_err());
}

#[test]
fn runtime_reads_a_raw_kcdb_handle_across_native_calls() {
    let mut code = Vec::new();
    let native = |code: &mut Vec<u8>, index: u32, byte_count: u32| {
        code.extend([0x2b, 3]);
        code.extend(index.to_le_bytes());
        code.push(3);
        code.extend(byte_count.to_le_bytes());
    };
    code.extend([0x25, 3, 32, 0, 0, 0]);
    native(&mut code, 791, 4);
    code.extend([0x26, 3, 0, 0, 0, 0]);
    code.extend([0x25, 0x13, 0, 0, 0, 0]);
    code.extend([0x25, 3, 64, 0, 0, 0]);
    code.extend([0x25, 3, 1, 0, 0, 0]);
    native(&mut code, 793, 12);
    code.push(0x3b);
    let mut data = data();
    data[64..69].copy_from_slice(b"ID\0\0\0");
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        KcsFactory::new(Loader(Program::from_code(&code, data, 64).unwrap())),
    );
    runtime.set_resources(Files(Some(table())));
    runtime.set_budget(1, 1024);
    let id = runtime.create(0, kind::ORDINARY, b"", b"").unwrap();
    runtime.update(0, 1).unwrap();
    let vm = runtime.object(id).unwrap().interpreter().unwrap();
    assert_eq!(vm.stack_bytes(), 7u32.to_le_bytes());
}

fn quiz_table(prefix: u8) -> Vec<u8> {
    let names = [
        b"G_A".as_slice(),
        b"G_B",
        b"G_C",
        b"ID",
        b"IMG",
        b"Q",
        b"A1",
        b"A2",
        b"A3",
        b"A4",
    ];
    let mut plain = (names.len() as u32).to_le_bytes().to_vec();
    for name in names {
        let mut column = [0; 24];
        column[..name.len()].copy_from_slice(name);
        column[20..].copy_from_slice(&4u32.to_le_bytes());
        plain.extend(column);
    }
    let rows = [
        ([3, 5, 7, 19, 1], b'z'),
        ([3, 5, 7, 3, 2], b'a'),
        ([3, 5, 7, 19, 3], b'd'),
        ([3, 5, 2, 19, 4], b'o'),
        ([5, 5, 7, u32::MAX - 1, 5], b'n'),
    ];
    plain.extend((rows.len() as u32).to_le_bytes());
    plain.extend(40u32.to_le_bytes());
    for (words, q) in rows {
        for word in words {
            plain.extend(word.to_le_bytes());
        }
        plain.extend([prefix, q, 0, 0]);
        for answer in b"abcd" {
            plain.extend([*answer, 0, 0, 0]);
        }
    }
    pack(&plain)
}

#[test]
fn quiz_packs_merge_without_reordering_existing_indices_until_rebuild() {
    #[derive(Default)]
    struct QuizFiles(Vec<Vec<u8>>);
    impl ResourceServices for QuizFiles {
        fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            assert_eq!(category, b"database");
            Ok(name.to_vec())
        }

        fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
            assert_eq!(mode, ResourceMode::LooseFirst);
            self.0.push(name.to_vec());
            Ok(match name {
                b"db/A.kcdb" => Some(quiz_table(b'A')),
                b"db/B.kcdb" => Some(quiz_table(b'B')),
                _ => None,
            })
        }
    }

    let mut numbers = State::new(0);
    let mut core = CoreHost::new(&mut numbers);
    let mut files = QuizFiles::default();
    let mut host = ResourceHost::new(&mut core, &mut files);
    let mut data = vec![0xa5; 256];
    for (at, name) in [
        (16, &b"db/A.kcdb\0"[..]),
        (32, b"else/a.other\0"),
        (48, b"db/B.kcdb\0"),
        (64, b"absent/pack.kcdb\0"),
    ] {
        data[at..at + name.len()].copy_from_slice(name);
    }
    let mut vm =
        Interpreter::new(Program::from_code(&[], data, 64).unwrap(), vec![0xa5; 32]).unwrap();
    let handle = value(call(&mut host, &mut vm, 742, &[]));
    assert_eq!(value(call(&mut host, &mut vm, 744, &[handle, 16])), 1);
    assert_eq!(value(call(&mut host, &mut vm, 744, &[handle, 32])), 0);
    assert_eq!(value(call(&mut host, &mut vm, 745, &[handle])), 4);
    assert_eq!(value(call(&mut host, &mut vm, 746, &[handle, 0, 128])), 0);
    assert_eq!(vm.read(128, 1).unwrap().as_ref(), b"\xa5");
    call(&mut host, &mut vm, 764, &[handle]);
    call(&mut host, &mut vm, 749, &[handle, 0]);
    call(&mut host, &mut vm, 766, &[handle, 0]);
    assert_eq!(value(call(&mut host, &mut vm, 744, &[handle, 48])), 1);
    assert_eq!(value(call(&mut host, &mut vm, 744, &[handle, 64])), 0);
    assert_eq!(value(call(&mut host, &mut vm, 745, &[handle])), 8);
    assert_eq!(
        value(call(&mut host, &mut vm, 776, &[handle, 0, 128])),
        u32::MAX - 1
    );
    assert_eq!(vm.read(128, 3).unwrap().as_ref(), b"A\0\xa5");

    call(&mut host, &mut vm, 764, &[handle]);
    for (index, prompt) in [b"Bn", b"An", b"Bo", b"Ao", b"Ba", b"Bz", b"Aa", b"Az"]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            value(call(
                &mut host,
                &mut vm,
                746,
                &[handle, index as u32, 0x8000_0000]
            )),
            1
        );
        assert_eq!(vm.read(0x8000_0000, 2).unwrap().as_ref(), prompt);
    }
    assert_eq!(value(call(&mut host, &mut vm, 747, &[handle, 8])), u32::MAX);
    assert_eq!(
        value(call(&mut host, &mut vm, 748, &[handle, 1, 3, 0x8000_0000])),
        1
    );
    assert_eq!(vm.read(0x8000_0000, 4).unwrap().as_ref(), b"d\0\0\xa5");
    assert_eq!(value(call(&mut host, &mut vm, 771, &[handle, 1])), 1);
    assert_eq!(value(call(&mut host, &mut vm, 772, &[handle, 1])), 1);
    assert_eq!(value(call(&mut host, &mut vm, 771, &[handle, 0])), 0);
    assert_eq!(
        value(call(&mut host, &mut vm, 776, &[0, 0, u32::MAX])),
        u32::MAX
    );
    assert!(matches!(
        call(&mut host, &mut vm, 743, &[handle]),
        NativeReturn::Continue
    ));
    assert!(host.call(745, &handle.to_le_bytes(), &mut vm).is_err());
    assert_eq!(
        files.0,
        [b"db/A.kcdb".as_slice(), b"db/B.kcdb", b"absent/pack.kcdb"]
    );
}
