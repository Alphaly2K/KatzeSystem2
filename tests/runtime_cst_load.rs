use katze_system2::{
    Result,
    expression::State,
    kcs::{Program, ResourceServices},
    resource::ResourceMode,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};
use std::{cell::RefCell, collections::BTreeMap, io::Write, rc::Rc};

fn script(compressed: bool) -> Vec<u8> {
    let records = [
        (0x21, "名前"),
        (0x20, "本文一"),
        (0x20, "本文二"),
        (2, ""),
        (0x20, "next"),
        (2, ""),
    ];
    let mut text = vec![];
    let mut offsets = vec![];
    for (kind, value) in records {
        offsets.push(text.len() as u32);
        text.extend([1, kind]);
        text.extend(encoding_rs::SHIFT_JIS.encode(value).0.as_ref());
        text.push(0);
    }
    let strings = 16 + offsets.len() as u32 * 4;
    let mut body: Vec<u8> = [strings + text.len() as u32, 2, 16, strings, 4, 0, 2, 4]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    body.extend(offsets.into_iter().flat_map(u32::to_le_bytes));
    body.extend(text);
    let size = body.len() as u32;
    if compressed {
        let mut encoder = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::fast());
        encoder.write_all(&body).unwrap();
        body = encoder.finish().unwrap();
    }
    let mut out = b"CatScene".to_vec();
    out.extend(
        [if compressed { body.len() as u32 } else { 0 }, size]
            .into_iter()
            .flat_map(u32::to_le_bytes),
    );
    out.extend(body);
    out
}

fn localization(first: &str) -> Vec<u8> {
    let mut out = b"CSTL\0\0\0\0\x02".to_vec();
    let string = |out: &mut Vec<u8>, text: &str| {
        out.push(text.len() as u8);
        out.extend(text.as_bytes());
    };
    for name in ["jp", "日"] {
        string(&mut out, name);
    }
    out.push(2);
    for row in [
        [(first, "J-one\x1fJ-two"), ("日-name", "日-text")],
        [("", "J-next"), ("", "日-next")],
    ] {
        for (name, body) in row {
            string(&mut out, name);
            string(&mut out, body);
        }
    }
    out
}

type Files = Rc<RefCell<BTreeMap<Vec<u8>, Vec<u8>>>>;
type Requests = Rc<RefCell<Vec<Vec<u8>>>>;
struct Sources(Files, Requests);
impl ResourceServices for Sources {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!((category, name), (b"scene".as_slice(), b"story".as_slice()));
        Ok(b"mapped/scene.cst".to_vec())
    }
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        self.1.borrow_mut().push(name.to_vec());
        Ok(self.0.borrow().get(name).cloned())
    }
}
struct Loader(Program);
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}
fn ins(code: &mut Vec<u8>, op: u8, args: &[u32]) {
    code.push(op);
    for value in args {
        code.push(3);
        code.extend(value.to_le_bytes());
    }
}
fn call(code: &mut Vec<u8>, index: u32, args: &[u32], output: Option<u32>) {
    for arg in args {
        ins(code, 0x25, &[*arg]);
    }
    ins(code, 0x2b, &[index, args.len() as u32 * 4]);
    if let Some(output) = output {
        ins(code, 0x26, &[output]);
    }
}
fn runtime(code: &[u8], budget: usize) -> (Runtime<KcsFactory<Loader>>, u32, Files, Requests) {
    let mut data = vec![0x5a; 512];
    for (at, value) in [
        (64, b"story\0".as_slice()),
        (80, b"jp\0"),
        (96, "日\0".as_bytes()),
        (112, b"\x93\xfa\0"),
    ] {
        data[at..at + value.len()].copy_from_slice(value);
    }
    let mut rt = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        KcsFactory {
            programs: Loader(Program::from_code(code, data, 64).unwrap()),
            external: katze_system2::memory::SharedMemory::new(vec![0; 256]).unwrap(),
        },
    );
    rt.set_budget(budget, 1 << 20);
    rt.host_mut().languages.prefixes = vec![b"jp".to_vec(), b"\x93\xfa".to_vec()];
    let files = Rc::new(RefCell::new(BTreeMap::from([
        (b"mapped/scene.cst".to_vec(), script(false)),
        (b"mapped/scene.cstl".to_vec(), localization("J-name")),
    ])));
    let requests = Rc::new(RefCell::new(vec![]));
    rt.set_resources(Sources(files.clone(), requests.clone()));
    let task = rt.create(0, kind::ORDINARY, b"cst", b"").unwrap();
    (rt, task, files, requests)
}

#[test]
fn localized_scene_and_name_table_use_resource_encoding_for_unicode_outputs() {
    struct TranslatedSources(Vec<u8>);
    impl ResourceServices for TranslatedSources {
        fn script_encoding(&self) -> &'static encoding_rs::Encoding {
            encoding_rs::GBK
        }
        fn resolve(&mut self, category: &[u8], _: &[u8]) -> Result<Vec<u8>> {
            Ok(if category == b"scene" {
                b"scene".to_vec()
            } else {
                b"names".to_vec()
            })
        }
        fn read(&mut self, name: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            Ok(match name {
                b"scene" => Some(self.0.clone()),
                b"names" => Some(
                    encoding_rs::GBK
                        .encode("0\t汉化姓名\tvoice\n")
                        .0
                        .into_owned(),
                ),
                _ => None,
            })
        }
    }
    let text = encoding_rs::GBK.encode("汉化正文").0.into_owned();
    let mut body: Vec<u8> = [15 + text.len() as u32, 1, 8, 12, 1, 0, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    body.extend([1, 0x20]);
    body.extend(&text);
    body.push(0);
    let mut scene = b"CatScene".to_vec();
    scene.extend(
        [0, body.len() as u32]
            .into_iter()
            .flat_map(u32::to_le_bytes),
    );
    scene.extend(body);
    let mut code = Vec::new();
    for (index, args, output) in [
        (323, vec![], 400),
        (325, vec![1, 64], 0),
        (327, vec![1, 160, 0, 0], 4),
        (1037, vec![1, 200, 0, 0], 8),
        (633, vec![64], 12),
        (637, vec![2, 0, 240], 16),
    ] {
        call(&mut code, index, &args, Some(output));
    }
    for budget in [1, 4096] {
        for unicode in [0, 1] {
            let (mut rt, task, _, _) = runtime(&code, budget);
            rt.host_mut().unicode_text = unicode;
            rt.set_resources(katze_system2::kcs::CachedResources::new(TranslatedSources(
                scene.clone(),
            )));
            rt.update(0, 1).unwrap();
            let vm = rt.object(task).unwrap().interpreter().unwrap();
            assert_eq!(
                vm.read_string(160).unwrap(),
                if unicode == 0 {
                    text.as_slice()
                } else {
                    "汉化正文".as_bytes()
                }
            );
            assert_eq!(vm.read_string(200).unwrap(), "汉化正文".as_bytes());
            assert_eq!(
                vm.read_string(240).unwrap(),
                if unicode == 0 {
                    encoding_rs::GBK.encode("汉化姓名").0.into_owned()
                } else {
                    "汉化姓名".as_bytes().to_vec()
                }
            );
        }
    }
}

#[test]
fn loading_preserves_missing_state_clears_failed_payload_and_rebinds_languages() {
    let mut code = vec![];
    call(&mut code, 323, &[], Some(400));
    for _ in 0..6 {
        call(&mut code, 325, &[1, 64], Some(0));
        call(&mut code, 65, &[], None);
    }
    for budget in [1, 4096] {
        let (mut rt, task, files, requests) = runtime(&code, budget);
        for stage in 0..6 {
            match stage {
                1 => {
                    files.borrow_mut().remove(b"mapped/scene.cst".as_slice());
                }
                2 => {
                    files
                        .borrow_mut()
                        .insert(b"mapped/scene.cst".to_vec(), b"wrong signature".to_vec());
                    files
                        .borrow_mut()
                        .insert(b"mapped/scene.cstl".to_vec(), localization("new name"));
                }
                3 => {
                    let mut invalid = script(false);
                    invalid.push(0);
                    files
                        .borrow_mut()
                        .insert(b"mapped/scene.cst".to_vec(), invalid);
                }
                4 => {
                    files
                        .borrow_mut()
                        .insert(b"mapped/scene.cst".to_vec(), script(true));
                    let mut local = localization("restored");
                    local.extend(b"tail");
                    assert!(katze_system2::cstl::Table::parse(&local).is_err());
                    files
                        .borrow_mut()
                        .insert(b"mapped/scene.cstl".to_vec(), local);
                }
                5 => {
                    let mut empty = b"CatScene".to_vec();
                    empty.extend(
                        [0u32, 16, 0, 0, 0, 0]
                            .into_iter()
                            .flat_map(u32::to_le_bytes),
                    );
                    files
                        .borrow_mut()
                        .insert(b"mapped/scene.cst".to_vec(), empty);
                    files.borrow_mut().remove(b"mapped/scene.cstl".as_slice());
                }
                _ => {}
            }
            requests.borrow_mut().clear();
            rt.update(stage, 1).unwrap();
            let context = rt.cst_context(1).unwrap();
            let result = u32::from_le_bytes(
                rt.object(task)
                    .unwrap()
                    .interpreter()
                    .unwrap()
                    .read(0, 4)
                    .unwrap()[..]
                    .try_into()
                    .unwrap(),
            );
            assert_eq!(result, u32::from(matches!(stage, 0 | 4 | 5)));
            assert_eq!(context.script().is_some(), stage != 3);
            assert_eq!(
                context.bound_languages().collect::<Vec<_>>(),
                if stage == 5 {
                    vec![]
                } else {
                    vec![b"jp".as_slice()]
                }
            );
            if stage == 1 {
                assert_eq!(
                    context.localization().unwrap().messages[0][0].name,
                    "J-name"
                );
            }
            if stage == 2 {
                assert_eq!(
                    context.localization().unwrap().messages[0][0].name,
                    "new name"
                );
            }
            if stage == 5 {
                assert_eq!(context.script().unwrap().record_kind(-1, -1), 0);
            }
            assert_eq!(requests.borrow().len(), if stage == 1 { 1 } else { 2 });
        }
    }
}

#[test]
fn record_queries_keep_raw_and_utf8_paths_separate_and_use_current_indices() {
    let mut code = vec![];
    call(&mut code, 323, &[], Some(400));
    call(&mut code, 325, &[1, 64], Some(404));
    for (index, args, output) in [
        (327, vec![1, 128, 0, 0], 0),
        (1037, vec![1, 160, 0, 0], 4),
        (1033, vec![1, 0x80000020, 0, 1, 80], 8),
        (1038, vec![1, 0x80000040, 0, 2, 80], 12),
        (1033, vec![1, 0x80000060, 0, 0, 96], 16),
        (1038, vec![1, 0x80000080, 0, 0, 112], 20),
        (327, vec![1, 224, 9, 0], 24),
        (1038, vec![0], 28),
    ] {
        call(&mut code, index, &args, Some(output));
    }
    call(&mut code, 65, &[], None);
    call(&mut code, 1033, &[1, 192, u32::MAX, u32::MAX, 80], Some(32));
    call(&mut code, 65, &[], None);
    for budget in [1, 4096] {
        let (mut rt, task, _, _) = runtime(&code, budget);
        rt.update(0, 1).unwrap();
        let vm = rt.object(task).unwrap().interpreter().unwrap();
        let results: Vec<_> = vm
            .read(0, 32)
            .unwrap()
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        assert_eq!(results, [4, 6, 5, 5, 4, 6, 0, 0]);
        assert_eq!(vm.read(128, 5).unwrap().as_ref(), b"\x96\xbc\x91\x4f\0");
        assert_eq!(vm.read(160, 7).unwrap().as_ref(), "名前\0".as_bytes());
        assert_eq!(vm.read(0x80000020, 6).unwrap().as_ref(), b"J-one\0");
        assert_eq!(vm.read(0x80000040, 6).unwrap().as_ref(), b"J-two\0");
        assert_eq!(vm.read(224, 4).unwrap().as_ref(), &[0x5a; 4]);
        assert_eq!(
            rt.cst_context(1).unwrap().languages().collect::<Vec<_>>(),
            [b"jp".as_slice(), b"\x93\xfa"]
        );
        rt.cst_context_mut(1).unwrap().set_position(0, 2);
        rt.update(1, 1).unwrap();
        let vm = rt.object(task).unwrap().interpreter().unwrap();
        assert_eq!(vm.read(192, 6).unwrap().as_ref(), b"J-two\0");
    }
}

#[test]
fn localized_option_query_joins_segments_and_clears_missing_results() {
    let mut code = vec![];
    call(&mut code, 323, &[], Some(400));
    call(&mut code, 325, &[1, 64], Some(404));
    for (args, output) in [
        (vec![1, 128, 0, 80], 0),
        (vec![1, 160, u32::MAX, 80], 4),
        (vec![1, 192, 0, 96], 8),
        (vec![1, 224, 8, 80], 12),
        (vec![1, 0xdeadbeef, 0, 0xdeadbeef], 16),
        (vec![0], 20),
    ] {
        call(&mut code, 1064, &args, Some(output));
    }
    call(&mut code, 65, &[], None);
    for budget in [1, 4096] {
        let (mut rt, task, _, _) = runtime(&code, budget);
        rt.update(0, 1).unwrap();
        let vm = rt.object(task).unwrap().interpreter().unwrap();
        let results: Vec<_> = vm
            .read(0, 24)
            .unwrap()
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        assert_eq!(results, [6, 10, 0, 0, 0, 0]);
        assert_eq!(vm.read_string(128).unwrap().as_ref(), b"J-next");
        assert_eq!(vm.read_string(160).unwrap().as_ref(), b"J-oneJ-two");
        assert_eq!(vm.read(192, 2).unwrap().as_ref(), &[0, 0x5a]);
        assert_eq!(vm.read(224, 2).unwrap().as_ref(), &[0, 0x5a]);
    }
}
