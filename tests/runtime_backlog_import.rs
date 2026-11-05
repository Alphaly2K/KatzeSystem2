use katze_system2::{
    Result,
    backlog::{Backlog, Record, Settings},
    expression::State,
    kcs::{Program, ResourceServices},
    resource::ResourceMode,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
    settings::SystemSettings,
};
use std::{cell::RefCell, rc::Rc};

fn script() -> Vec<u8> {
    let groups = [
        vec![(0x21, "pre＠$str2"), (0x20, "一$str1"), (2, "")],
        vec![(0x30, "cont 1"), (0x20, "two"), (2, "")],
        vec![
            (0x30, "voice absent hit later"),
            (0x30, "cont 0"),
            (0x20, "three"),
            (3, ""),
        ],
        vec![(0x30, "CONT #0++"), (0x20, "four"), (2, "")],
    ];
    let mut text = Vec::new();
    let mut offsets = Vec::new();
    let mut ranges = Vec::new();
    for group in groups {
        ranges.extend([group.len() as u32, offsets.len() as u32]);
        for (kind, body) in group {
            offsets.push(text.len() as u32);
            text.extend([1, kind]);
            text.extend(encoding_rs::SHIFT_JIS.encode(body).0.as_ref());
            text.push(0);
        }
    }
    let start = ranges.len() as u32 * 4;
    let strings = start + offsets.len() as u32 * 4;
    let mut body: Vec<u8> = [strings + text.len() as u32, 4, start, strings]
        .into_iter()
        .chain(ranges)
        .chain(offsets)
        .flat_map(u32::to_le_bytes)
        .collect();
    body.extend(text);
    let mut out = b"CatScene".to_vec();
    out.extend(
        [0, body.len() as u32]
            .into_iter()
            .flat_map(u32::to_le_bytes),
    );
    out.extend(body);
    out
}

struct Sources(Rc<RefCell<Vec<Vec<u8>>>>);
impl ResourceServices for Sources {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        let mut out = name.to_vec();
        out.extend(if category == b"scene" {
            b".cst"
        } else {
            b".wav"
        });
        Ok(out)
    }
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        self.0.borrow_mut().push(name.to_vec());
        Ok(match name {
            b"A.cst" => Some(script()),
            b"A.cstl" => Some(b"CSTL\0\0\0\0\x01\x02jp\x01\x05Other\x05Local".to_vec()),
            b"hit.wav" => Some(vec![]),
            _ => None,
        })
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
fn call(code: &mut Vec<u8>, index: u32, args: &[u32], value: bool) {
    for arg in args {
        ins(code, 0x25, &[*arg]);
    }
    ins(code, 0x2b, &[index, args.len() as u32 * 4]);
    if value {
        ins(code, 0x26, &[400]);
    }
}

#[test]
fn import_selects_history_window_and_preserves_flags_and_void_return_slot() {
    for index in [699, 1034] {
        for budget in [1, 4096] {
            let mut data = vec![0x5a; 512];
            for (at, text) in [
                (64, b"A\0".as_slice()),
                (80, b"B\0"),
                (96, b"voice\0"),
                (112, b"cont-extra\0"),
                (128, b"jp\0"),
            ] {
                data[at..at + text.len()].copy_from_slice(text);
            }
            let mut code = Vec::new();
            call(&mut code, 323, &[], true);
            call(&mut code, 696, &[], true);
            for (name, count) in [(64, 4), (80, 3), (64, 4)] {
                call(&mut code, 329, &[1, name, count], true);
            }
            call(&mut code, 785, &[2, 2], false);
            ins(&mut code, 0x25, &[0x12345678]);
            let args = [2, 1, 96, 112, 128];
            call(
                &mut code,
                index,
                &args[..if index == 699 { 4 } else { 5 }],
                false,
            );
            call(&mut code, index, &[0], false);
            let args = [2, 0, u32::MAX, u32::MAX, u32::MAX];
            call(
                &mut code,
                index,
                &args[..if index == 699 { 4 } else { 5 }],
                false,
            );
            ins(&mut code, 0x26, &[404]);
            call(&mut code, 65, &[], false);
            let mut numbers = State::new(1);
            numbers.strings.insert(1, b"body".to_vec());
            numbers.strings.insert(2, b"Alice".to_vec());
            let mut rt = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                numbers,
                KcsFactory {
                    programs: Loader(Program::from_code(&code, data, 64).unwrap()),
                    external: katze_system2::memory::SharedMemory::new(vec![0; 0]).unwrap(),
                },
            );
            rt.set_budget(budget, 1 << 20);
            rt.host_mut().languages.prefixes = vec![b"jp".to_vec()];
            let settings = Settings {
                space: 1,
                ..Default::default()
            };
            rt.host_mut().system_settings = Some(SystemSettings {
                backlog: settings.clone(),
                ..Default::default()
            });
            let requests = Rc::new(RefCell::new(Vec::new()));
            rt.set_resources(Sources(requests.clone()));
            let task = rt.create(0, kind::ORDINARY, b"import", b"").unwrap();
            rt.update(0, 1).unwrap();
            let mut expected = Backlog::new(settings);
            expected.set_limit(2);
            let first = if index == 699 {
                b"\x88\xeabody".as_slice()
            } else {
                "一body".as_bytes()
            };
            for (input, text, name, flag, space) in [
                (0, first, b"Alice".as_slice(), 0, 1),
                (1, b"two", b"", 0, 0),
                (2, b"three", b"", 1, 1),
                (3, b"four", b"", 0, 0),
            ] {
                expected
                    .push(Record {
                        file: b"A",
                        input,
                        name,
                        text,
                        flag,
                        space,
                        extra: 2,
                    })
                    .unwrap();
            }
            assert_eq!(
                rt.backlog(2).unwrap().serialize().unwrap(),
                expected.serialize().unwrap()
            );
            assert_eq!(rt.numbers().shared, [1]);
            assert_eq!(
                *requests.borrow(),
                [b"A.cst".as_slice(), b"A.cstl", b"absent.wav", b"hit.wav"]
            );
            assert_eq!(rt.cst_context(1).unwrap().history().len(), 3);
            assert!(rt.cst_context(1).unwrap().script().is_none());
            assert_eq!(
                rt.object(task)
                    .unwrap()
                    .interpreter()
                    .unwrap()
                    .read(404, 4)
                    .unwrap()
                    .as_ref(),
                &0x12345678u32.to_le_bytes()
            );
        }
    }
}
