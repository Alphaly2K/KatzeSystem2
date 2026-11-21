use katze_system2::{
    Result,
    expression::State,
    host::resources::{ResourceMode, ResourceServices},
    kcs::Program,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};
use std::{cell::RefCell, collections::BTreeMap, io::Write, rc::Rc};

fn script(compressed: bool) -> Vec<u8> {
    scene(
        &[
            &[(0x21, "名前"), (0x20, "本文一"), (0x20, "本文二"), (2, "")],
            &[(0x20, "next"), (2, "")],
        ],
        compressed,
    )
}

fn scene(groups: &[&[(u8, &str)]], compressed: bool) -> Vec<u8> {
    let mut text = vec![];
    let mut offsets = vec![];
    let mut ranges = vec![];
    for group in groups {
        ranges.extend([group.len() as u32, offsets.len() as u32]);
        for &(kind, value) in *group {
            offsets.push(text.len() as u32);
            text.extend([1, kind]);
            text.extend(encoding_rs::SHIFT_JIS.encode(value).0.as_ref());
            text.push(0);
        }
    }
    let indices = ranges.len() as u32 * 4;
    let strings = indices + offsets.len() as u32 * 4;
    let mut body: Vec<u8> = [
        strings + text.len() as u32,
        groups.len() as u32,
        indices,
        strings,
    ]
    .into_iter()
    .chain(ranges)
    .chain(offsets)
    .flat_map(u32::to_le_bytes)
    .collect();
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
fn history_browser_scrolls_across_scenes_and_preserves_ordered_outputs() -> Result<()> {
    struct Scenes;
    impl ResourceServices for Scenes {
        fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            assert_eq!(category, b"scene");
            Ok([name, b".cst"].concat())
        }
        fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
            assert_eq!(mode, ResourceMode::LooseFirst);
            Ok(match name {
                b"one.cst" => Some(scene(
                    &[
                        &[(0x20, "a0")],
                        &[(0x30, "only")],
                        &[(0x21, "speaker")],
                        &[(0x20, "a3")],
                    ],
                    false,
                )),
                b"two.cst" => Some(scene(
                    &[
                        &[
                            (0x21, "Bob"),
                            (0x20, "b0"),
                            (0x30, "c0"),
                            (0x20, "b1"),
                            (0x30, "c1"),
                        ],
                        &[(0x30, "only")],
                        &[(0x20, "b2")],
                        &[(0x20, "b3")],
                    ],
                    false,
                )),
                b"two.cstl" => Some(localization("translated")),
                _ => None,
            })
        }
    }
    let mut code = vec![];
    call(&mut code, 323, &[], Some(0));
    call(&mut code, 325, &[1, 208], Some(4));
    call(&mut code, 65, &[], None);
    call(&mut code, 379, &[1, 2], Some(8));
    call(&mut code, 384, &[2, 0, 384, 388], Some(12));
    call(&mut code, 380, &[2], None);
    call(&mut code, 329, &[1, 192, 3], None);
    call(&mut code, 329, &[1, 208, 3], None);
    call(&mut code, 379, &[1, 1], Some(16));
    call(&mut code, 384, &[3, 0, 256, 260], Some(20));
    call(&mut code, 381, &[3, 1], Some(24));
    call(&mut code, 380, &[3], None);
    call(&mut code, 379, &[1, 3], Some(28));
    let browser = 4;
    for (i, (native, args)) in [
        (1032, [browser, 0, 272, 0x8000_0000, 80].as_slice()),
        (382, &[browser, 0, 288, 288]),
        (383, &[browser, 0, 304, 1]),
        (383, &[browser, 0, 320, 2]),
        (383, &[browser, 0, u32::MAX, 0]),
        (382, &[browser, 3, 336, 0x8000_0040]),
        (381, &[browser, 0]),
        (384, &[browser, 0, 264, 268]),
        (381, &[browser, 0]),
        (381, &[browser, 0]),
        (381, &[browser, 0]),
        (384, &[browser, 0, 392, 396]),
        (381, &[browser, 7]),
        (381, &[browser, 1]),
        (381, &[browser, 1]),
        (381, &[browser, 1]),
        (384, &[browser, 2, 400, 400]),
    ]
    .into_iter()
    .enumerate()
    {
        call(
            &mut code,
            native,
            args,
            Some(32 + i as u32 * 4 + if i >= 8 { 64 } else { 0 }),
        );
    }
    call(&mut code, 65, &[], None);
    call(&mut code, 331, &[1], None);
    call(&mut code, 329, &[1, 224, 3], None);
    call(&mut code, 382, &[browser, 0, 352, 368], Some(184));
    call(&mut code, 324, &[1], None);
    call(&mut code, 384, &[browser, 0, 404, 408], Some(188));
    call(&mut code, 380, &[browser], None);
    code.push(0x3b);

    let (mut runtime, task, _, _) = runtime(&code, 4096);
    runtime.set_resources(Scenes);
    let vm = runtime.object_mut(task).unwrap().interpreter_mut().unwrap();
    for (at, value) in [
        (192, b"one\0".as_slice()),
        (208, b"two\0"),
        (224, b"missing\0"),
    ] {
        vm.write(at, value)?;
    }
    runtime.update(0, 1)?;
    runtime.cst_context_mut(1).unwrap().set_position(2, 0);
    runtime.update(1, 1)?;
    let ctx = runtime.cst_context(1).unwrap();
    assert_eq!(ctx.position(), [2, 0]);
    assert_eq!(ctx.script().unwrap().inputs().len(), 4);
    assert_eq!(ctx.bound_languages().collect::<Vec<_>>(), [b"jp"]);
    fn words(vm: &katze_system2::kcs::Interpreter, at: u32, n: usize) -> Vec<u32> {
        vm.read(at, n * 4)
            .unwrap()
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
            .collect()
    }
    let vm = runtime.object(task).unwrap().interpreter().unwrap();
    assert_eq!(
        words(vm, 0, 16),
        [1, 1, 2, 0, 3, 1, 0, 4, 1, 1, 1, 0, 1, 0, 1, 1]
    );
    assert_eq!(words(vm, 128, 9), [1, 1, 0, 1, 1, 1, 1, 0, 1]);
    assert_eq!(words(vm, 256, 4), [1, 2, 0, 3]);
    assert_eq!(words(vm, 384, 5), [0x5a5a5a5a, 0x5a5a5a5a, 0, 0, 3]);
    assert_eq!(&*vm.read(272, 6)?, b"b0b1\0Z");
    assert_eq!(&*vm.read(0x8000_0000, 4)?, b"Bob\0");
    assert_eq!(&*vm.read(288, 8)?, b"Bobb0b1\0");
    assert_eq!(&*vm.read(304, 4)?, b"c1\0Z");
    assert_eq!(&*vm.read(320, 2)?, b"\0Z");
    assert_eq!(&*vm.read(336, 4)?, b"ZZZZ");

    runtime.update(2, 1)?;
    let vm = runtime.object(task).unwrap().interpreter().unwrap();
    assert_eq!(words(vm, 184, 2), [0, 1]);
    assert_eq!(words(vm, 404, 2), [0, 0]);
    assert_eq!(&*vm.read(352, 4)?, b"ZZZZ");
    assert_eq!(&*vm.read(368, 4)?, b"ZZZZ");
    Ok(())
}

#[test]
fn reader_metadata_keeps_reserved_fields_after_load_and_skips_null_outputs() {
    let mut code = vec![];
    call(&mut code, 323, &[], Some(32));
    call(&mut code, 338, &[1, 200], Some(0));
    call(&mut code, 325, &[1, 64], Some(4));
    call(&mut code, 338, &[1, 0x8000_0020], Some(8));
    call(&mut code, 338, &[0, u32::MAX], Some(12));
    call(&mut code, 338, &[0], Some(16));
    code.push(0x3b);
    for budget in [1, 4096] {
        let (mut runtime, task, _, _) = runtime(&code, budget);
        runtime
            .object_mut(task)
            .unwrap()
            .interpreter_mut()
            .unwrap()
            .write(0x8000_0020, &[0x66; 4])
            .unwrap();
        runtime.update(0, 1).unwrap();
        let vm = runtime.object(task).unwrap().interpreter().unwrap();
        let values: Vec<_> = vm
            .read(0, 20)
            .unwrap()
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        assert_eq!(values, [0, 1, 0, 0, 0]);
        assert_eq!(vm.read(200, 4).unwrap(), [0, 0x5a, 0x5a, 0x5a]);
        assert_eq!(vm.read(0x8000_0020, 4).unwrap(), [0, 0x66, 0x66, 0x66]);
        assert!(runtime.cst_context(1).unwrap().script().is_some());
    }
}

#[test]
fn legacy_scene_queries_keep_previous_output_on_miss_and_use_captured_languages() {
    let mut code = vec![];
    call(&mut code, 220, &[], Some(0));
    call(&mut code, 219, &[], None);
    call(&mut code, 221, &[0, 0], Some(4));
    call(&mut code, 65, &[], None);
    call(&mut code, 220, &[64], Some(8));
    call(&mut code, 221, &[u32::MAX, u32::MAX], Some(12));
    call(&mut code, 221, &[0, u32::MAX], Some(16));
    call(&mut code, 222, &[200, u32::MAX, u32::MAX], Some(20));
    call(&mut code, 222, &[200, 2, 0], Some(24));
    call(&mut code, 1035, &[0x8000_0020, 0, 2, 80], Some(28));
    call(&mut code, 1035, &[u32::MAX, 0, 0, u32::MAX], Some(32));
    code.push(0x3b);
    let (mut game, task, _, requests) = runtime(&code, 1);
    game.update(0, 1).unwrap();
    assert!(requests.borrow().is_empty());
    game.host_mut().languages.prefixes.clear();
    game.host_mut().unicode_text = 1;
    game.update(1, 1).unwrap();
    let vm = game.object(task).unwrap().interpreter().unwrap();
    let values: Vec<_> = vm
        .read(0, 36)
        .unwrap()
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .collect();
    assert_eq!(values, [0, u32::MAX, 1, 2, 4, 4, 4, 5, 0]);
    assert_eq!(vm.read(200, 6).unwrap(), b"\x96\xbc\x91\x4f\0Z".as_slice());
    assert_eq!(vm.read(0x8000_0020, 6).unwrap(), b"J-two\0".as_slice());
    assert_eq!(
        requests.borrow().as_slice(),
        [b"mapped/scene.cst".as_slice(), b"mapped/scene.cstl"]
    );
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
            rt.set_resources(katze_system2::host::resources::CachedResources::new(
                TranslatedSources(scene.clone()),
            ));
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
    for (native, handle, output) in [(608, 1, 8), (980, 1, 12), (608, 0, 16), (980, 0, 20)] {
        call(&mut code, native, &[handle], Some(output));
    }
    for _ in 0..6 {
        call(&mut code, 325, &[1, 64], Some(0));
        call(&mut code, 608, &[1], Some(24));
        call(&mut code, 980, &[1], Some(28));
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
            let counts: Vec<_> = rt
                .object(task)
                .unwrap()
                .interpreter()
                .unwrap()
                .read(8, 24)
                .unwrap()
                .chunks_exact(4)
                .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
                .collect();
            assert_eq!(
                counts,
                [
                    u32::MAX,
                    0,
                    0,
                    0,
                    if stage == 3 {
                        u32::MAX
                    } else if stage == 5 {
                        0
                    } else {
                        2
                    },
                    if matches!(stage, 3 | 5) { 0 } else { 3 }
                ]
            );
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
