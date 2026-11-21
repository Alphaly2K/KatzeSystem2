use katze_system2::{
    Result,
    expression::State,
    host::{
        resources::{ResourceMode, ResourceServices},
        storage::{ReadSource, SharedSource},
    },
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program, ResourceHost},
    memory::SharedMemory,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, TaskRuntime, kind},
};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Source(Arc<AtomicUsize>);
impl Drop for Source {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
impl ReadSource for Source {
    fn len(&self) -> Result<u64> {
        Ok(0)
    }
    fn read_at(&self, _: u64, _: &mut [u8]) -> Result<usize> {
        panic!("存在查询不读取声音内容")
    }
}

#[derive(Default)]
struct Files {
    requests: Rc<RefCell<Vec<Vec<u8>>>>,
    dropped: Arc<AtomicUsize>,
    archive: Option<&'static [u8]>,
}
impl ResourceServices for Files {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(category, self.archive.unwrap_or(b"sound"));
        Ok([b"audio/".as_slice(), name, b".ogg"].concat())
    }
    fn read(&mut self, _: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
        panic!("存在查询直接打开读取源")
    }
    fn open(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<SharedSource>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        self.requests.borrow_mut().push(name.to_vec());
        Ok((name != b"audio/missing.ogg")
            .then(|| Arc::new(Source(self.dropped.clone())) as SharedSource))
    }
}

#[test]
fn game_resources_route_sound_names_before_opening_sources() -> Result<()> {
    let mut code = Vec::new();
    for (name, output) in [(32, 0), (64, 4), (80, 8)] {
        instruction(&mut code, 0x25, &[u32::MAX]);
        instruction(&mut code, 0x25, &[name]);
        instruction(&mut code, 0x2b, &[284, 8]);
        instruction(&mut code, 0x26, &[output]);
    }
    instruction(&mut code, 0x3b, &[]);
    let mut data = vec![0; 96];
    for (at, name) in [
        (32, b"ToNe3\0".as_slice()),
        (64, b"empty.ogg\0"),
        (80, b"unmatched.ogg\0"),
    ] {
        data[at..at + name.len()].copy_from_slice(name);
    }
    let program = Program::from_code(&code, data, 64)?;
    let startup = br#"<?xml version="1.0" encoding="UTF-8"?><document>
        <VOICE><v00><head>tone</head><int>LeGaCy</int></v00></VOICE>
        </document>"#;
    let definition = br#"<?xml version="1.0" encoding="UTF-8"?><document>
        <MASTER><volume>80</volume></MASTER>
        <PART><tone><head>tone</head><int>voice</int></tone>
        <empty><head>empty</head></empty></PART>
        </document>"#;
    let settings = br#"<?xml version="1.0" encoding="UTF-8"?><document>
        <MASTER><volume>50</volume></MASTER>
        <PART><tone><head>other</head><int>other</int></tone></PART>
        </document>"#;
    for definition in [None, Some(definition.as_slice())] {
        let files = Files {
            archive: Some(if definition.is_some() {
                b"voice"
            } else {
                b"legacy"
            }),
            ..Files::default()
        };
        let requests = files.requests.clone();
        let mut rt =
            TaskRuntime::with_programs(Loader(program.clone()), 0, SharedMemory::default())?;
        rt.set_game_resources(files, startup, definition, Some(settings))?;
        assert!(
            rt.set_game_resources(Files::default(), b"<", None, None)
                .is_err()
        );
        let task = rt.create(0, kind::ORDINARY, b"main", b"")?;
        rt.update(1, 1)?;
        let vm = rt.object(task).unwrap().interpreter().unwrap();
        assert_eq!(vm.read(0, 12)?.as_ref(), &[1, 0, 0, 0].repeat(3));
        assert_eq!(
            *requests.borrow(),
            [
                b"audio/ToNe3.ogg".as_slice(),
                b"empty.ogg",
                b"unmatched.ogg"
            ]
        );
        assert_eq!(
            rt.host()
                .sound_config
                .as_ref()
                .and_then(|config| config.master_volume),
            definition.map(|_| 50)
        );
    }
    Ok(())
}

struct Loader(Program);
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

fn instruction(code: &mut Vec<u8>, op: u8, words: &[u32]) {
    code.push(op);
    for word in words {
        code.push(3);
        code.extend(word.to_le_bytes());
    }
}

#[test]
fn sound_existence_queries_format_names_open_sources_and_preserve_script_memory() {
    let mut code = Vec::new();
    for (name, output) in [(32, 0), (64, 4), (80, 8)] {
        instruction(&mut code, 0x25, &[u32::MAX]);
        instruction(&mut code, 0x25, &[name]);
        instruction(&mut code, 0x2b, &[284, 8]);
        instruction(&mut code, 0x26, &[output]);
    }
    instruction(&mut code, 0x3b, &[]);
    let mut data = vec![0; 128];
    for (at, name) in [
        (32, b"tone%u[G96]\0".as_slice()),
        (64, b"missing\0"),
        (80, b"empty\0"),
    ] {
        data[at..at + name.len()].copy_from_slice(name);
    }
    data[96..100].copy_from_slice(&3u32.to_le_bytes());
    let program = Program::from_code(&code, data.clone(), 128).unwrap();
    for budget in [1, 4096] {
        let files = Files::default();
        let requests = files.requests.clone();
        let dropped = files.dropped.clone();
        let mut rt = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Loader(program.clone())),
        );
        rt.set_budget(budget, 1 << 20);
        rt.set_resources(files);
        let task = rt.create(0, kind::ORDINARY, b"query", b"").unwrap();
        rt.update(1, 1).unwrap();
        let vm = rt.object(task).unwrap().interpreter().unwrap();
        assert_eq!(
            &vm.read(0, 12).unwrap()[..],
            [1u32, 0, 1]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert_eq!(&vm.read(32, 96).unwrap()[..], &data[32..]);
        assert_eq!(
            *requests.borrow(),
            [
                b"audio/tone3.ogg".to_vec(),
                b"audio/missing.ogg".to_vec(),
                b"audio/empty.ogg".to_vec()
            ]
        );
        assert_eq!(dropped.load(Ordering::SeqCst), 2);
    }
    let mut state = State::new(0);
    let mut inner = CoreHost::new(&mut state);
    let mut files = Files::default();
    let dropped = files.dropped.clone();
    let mut host = ResourceHost::new(&mut inner, &mut files);
    let mut vm = Interpreter::new(program, vec![]).unwrap();
    let args: Vec<_> = [u32::MAX, 32]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    assert_eq!(
        host.call(284, &args, &mut vm).unwrap(),
        NativeReturn::Value(1)
    );
    assert_eq!(&vm.read(32, 96).unwrap()[..], &data[32..]);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}
