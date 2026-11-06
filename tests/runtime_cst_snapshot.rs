use katze_system2::{
    Result,
    expression::State,
    kcs::{Program, ResourceServices},
    resource::ResourceMode,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};
use std::{cell::Cell, rc::Rc};

struct Loader(Program);
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

struct Resources(Rc<Cell<bool>>);
impl ResourceServices for Resources {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert!(!self.0.get());
        assert_eq!(
            (category, name),
            (b"config".as_slice(), b"options".as_slice())
        );
        Ok(name.to_vec())
    }
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert!(!self.0.get());
        assert_eq!(name, b"options");
        assert_eq!(mode, ResourceMode::LooseFirst);
        Ok(Some(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><document><localstr>1,2</localstr></document>".to_vec()))
    }
}

fn ins(code: &mut Vec<u8>, opcode: u8, operands: &[(u8, u32)]) {
    code.push(opcode);
    for &(flags, value) in operands {
        code.push(flags);
        code.extend(value.to_le_bytes());
    }
}
fn call(code: &mut Vec<u8>, index: u32, arguments: &[(u8, u32)]) {
    for &argument in arguments {
        ins(code, 0x25, &[argument]);
    }
    ins(code, 0x2b, &[(3, index), (3, arguments.len() as u32 * 4)]);
}
fn register(code: &mut Vec<u8>, name: u32) {
    call(code, 329, &[(0x13, 0), (3, name), (3, 0)]);
    ins(code, 0x2d, &[]);
}
fn save(code: &mut Vec<u8>, position: u32) {
    call(code, 991, &[(0x13, 0), (3, position), (3, 80)]);
}
fn export(code: &mut Vec<u8>, record: u32) {
    call(
        code,
        385,
        &[(0x13, 4), (3, record), (0x13, 0), (3, 1), (3, 1)],
    );
    ins(code, 0x2d, &[]);
}
fn pause(code: &mut Vec<u8>) -> usize {
    call(code, 65, &[]);
    code.len()
}
fn prefix() -> Vec<u8> {
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[(3, 0x1234_5678)]);
    call(&mut code, 323, &[]);
    ins(&mut code, 0x26, &[(3, 0)]);
    call(&mut code, 238, &[(3, 0)]);
    ins(&mut code, 0x26, &[(3, 4)]);
    code
}
type Engine = Runtime<KcsFactory<Loader>>;
fn engine(code: &[u8], budget: usize, unloaded: bool) -> (Engine, u32, Rc<Cell<bool>>) {
    let mut data = vec![0; 256];
    data[80..88].copy_from_slice(b"options\0");
    data[128..134].copy_from_slice(b"first\0");
    data[144..151].copy_from_slice(b"second\0");
    data[160..166].copy_from_slice(b"third\0");
    let program = Program::from_code(code, data, 128).unwrap();
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(3),
        KcsFactory::new(Loader(program)),
    );
    runtime.set_budget(budget, 1 << 20);
    let mut settings = katze_system2::settings::SystemSettings {
        local_flag_ranges: [[u32::MAX; 2]; 8],
        ..Default::default()
    };
    settings.local_flag_ranges[0] = [0, 1];
    settings.local_flag_ranges[1] = [2, 1];
    runtime.host_mut().system_settings = Some(settings);
    let forbid = Rc::new(Cell::new(false));
    runtime.set_resources(Resources(forbid.clone()));
    let task = if unloaded {
        let task = runtime.create_unloaded_ordinary(0, b"").unwrap();
        let mut name = b"main.kcs\0".to_vec();
        runtime
            .dispatch_command_with_buffers(
                0,
                katze_system2::task::Command {
                    target: task,
                    sender: 0,
                    code: 161,
                    data: [0, 1, 0],
                },
                &mut katze_system2::task::CommandBuffers {
                    data1: Some(&mut name),
                    ..Default::default()
                },
            )
            .unwrap();
        task
    } else {
        runtime.create(0, kind::ORDINARY, b"main", b"").unwrap()
    };
    (runtime, task, forbid)
}
fn advance(runtime: &mut Engine, task: u32, target: usize) {
    for tick in 0..500 {
        runtime.update(tick, 1).unwrap();
        if runtime.object(task).unwrap().interpreter().unwrap().pc == target {
            return;
        }
    }
    panic!("script did not reach its checkpoint");
}
fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn snapshot(bytes: &[u8]) -> &[u8] {
    let start = 4 + word(bytes, 48) as usize;
    &bytes[start..start + word(bytes, 44) as usize]
}

#[test]
fn snapshots_restore_selected_values_after_css_roundtrip_and_keep_pruning_rules() {
    let mut code = prefix();
    register(&mut code, 128);
    save(&mut code, 10);
    let first = pause(&mut code);
    save(&mut code, 30);
    export(&mut code, 17);
    call(&mut code, 331, &[(0x13, 0)]);
    call(&mut code, 386, &[(0x13, 4), (3, 17), (0x13, 0), (3, 1)]);
    ins(&mut code, 0x2d, &[]);
    let second = pause(&mut code);
    call(&mut code, 992, &[(0x13, 0), (3, 32), (3, 36), (3, 80)]);
    ins(&mut code, 0x26, &[(3, 24)]);
    let restored = pause(&mut code);
    call(&mut code, 994, &[(0x13, 0), (3, 0), (3, 20)]);
    export(&mut code, 18);
    let pruned = pause(&mut code);
    call(&mut code, 994, &[(0x13, 0), (3, 0), (3, 0)]);
    export(&mut code, 19);
    call(&mut code, 994, &[(0x13, 0), (3, u32::MAX), (3, 0)]);
    call(&mut code, 991, &[(3, 0), (3, u32::MAX), (3, u32::MAX)]);
    call(
        &mut code,
        992,
        &[(3, 0), (3, u32::MAX), (3, u32::MAX), (3, u32::MAX)],
    );
    ins(&mut code, 0x26, &[(3, 28)]);
    let end = pause(&mut code);
    for budget in [1, 4096] {
        let (mut runtime, task, forbid) = engine(&code, budget, false);
        runtime.numbers_mut().shared.copy_from_slice(&[10, 11, 12]);
        runtime
            .numbers_mut()
            .strings
            .insert(1, b"old\0ignored".to_vec());
        advance(&mut runtime, task, first);
        runtime.numbers_mut().shared.copy_from_slice(&[30, 31, 32]);
        runtime.numbers_mut().strings.insert(1, b"new".to_vec());
        advance(&mut runtime, task, second);
        runtime.numbers_mut().shared.copy_from_slice(&[99, 98, 97]);
        runtime.numbers_mut().strings.insert(1, b"changed".to_vec());
        runtime.numbers_mut().strings.insert(2, b"missing".to_vec());
        runtime
            .numbers_mut()
            .strings
            .insert(3, b"untouched".to_vec());
        runtime
            .object_mut(task)
            .unwrap()
            .interpreter_mut()
            .unwrap()
            .write(32, &[0u32.to_le_bytes(), 20u32.to_le_bytes()].concat())
            .unwrap();
        advance(&mut runtime, task, restored);
        assert_eq!(runtime.numbers().shared, [10, 98, 12]);
        assert_eq!(runtime.numbers().strings[&1], b"old");
        assert_eq!(runtime.numbers().strings[&2], b"");
        assert_eq!(runtime.numbers().strings[&3], b"untouched");
        let vm = runtime.object(task).unwrap().interpreter().unwrap();
        assert_eq!(vm.read(24, 4).unwrap(), 1u32.to_le_bytes());
        assert_eq!(
            vm.read(32, 8).unwrap(),
            [0u32.to_le_bytes(), 10u32.to_le_bytes()].concat()
        );
        forbid.set(true);
        advance(&mut runtime, task, pruned);
        let data = runtime
            .host()
            .data_container(2)
            .unwrap()
            .record(18)
            .unwrap()
            .unwrap();
        assert_eq!(word(data, 36), 20);
        assert_eq!((word(snapshot(data), 0), word(snapshot(data), 16)), (0, 10));
        advance(&mut runtime, task, end);
        let data = runtime
            .host()
            .data_container(2)
            .unwrap()
            .record(19)
            .unwrap()
            .unwrap();
        assert_eq!(word(data, 36), 0);
        assert_eq!(word(snapshot(data), 16), 10);
        assert_eq!(runtime.cst_context(1).unwrap().history().len(), 0);
        let vm = runtime.object(task).unwrap().interpreter().unwrap();
        assert_eq!(vm.read(28, 4).unwrap(), [0; 4]);
        assert_eq!(vm.stack_bytes(), 0x1234_5678u32.to_le_bytes());
    }
}

#[test]
fn pruning_older_history_keeps_the_original_write_to_last_entry_before_truncation() {
    let mut code = prefix();
    register(&mut code, 128);
    save(&mut code, 10);
    save(&mut code, 30);
    register(&mut code, 144);
    register(&mut code, 160);
    save(&mut code, 20);
    save(&mut code, 40);
    call(&mut code, 994, &[(0x13, 0), (3, 0), (3, 20)]);
    export(&mut code, 17);
    let end = pause(&mut code);
    let (mut runtime, task, _) = engine(&code, 4096, false);
    advance(&mut runtime, task, end);
    assert_eq!(
        runtime
            .cst_context(1)
            .unwrap()
            .history()
            .collect::<Vec<_>>(),
        [(b"first".as_slice(), 20)]
    );
    let data = runtime
        .host()
        .data_container(2)
        .unwrap()
        .record(17)
        .unwrap()
        .unwrap();
    let saved = snapshot(data);
    assert_eq!(word(saved, 16), 30);
    assert_eq!(word(saved, word(saved, 0) as usize + 16), 10);
}

#[test]
fn an_unbound_shared_number_store_saves_zeroes_and_skips_number_restoration() {
    use std::io::Read;
    let mut code = prefix();
    register(&mut code, 128);
    save(&mut code, 10);
    export(&mut code, 17);
    let saved_at = pause(&mut code);
    call(&mut code, 992, &[(0x13, 0), (3, 32), (3, 36), (3, 80)]);
    ins(&mut code, 0x26, &[(3, 24)]);
    let end = pause(&mut code);
    let (mut runtime, task, _) = engine(&code, 4096, true);
    runtime.numbers_mut().shared.copy_from_slice(&[10, 11, 12]);
    advance(&mut runtime, task, saved_at);
    assert_eq!(
        runtime.object(task).unwrap().shared_numbers_available(),
        Some(false)
    );
    let data = runtime
        .host()
        .data_container(2)
        .unwrap()
        .record(17)
        .unwrap()
        .unwrap();
    let data = snapshot(data);
    let mut body = Vec::new();
    flate2::read::ZlibDecoder::new(&data[20..word(data, 4) as usize])
        .read_to_end(&mut body)
        .unwrap();
    assert_eq!(
        &body[..16],
        [0u32, 0, 2, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>()
    );
    runtime.numbers_mut().shared.copy_from_slice(&[99, 98, 97]);
    runtime
        .object_mut(task)
        .unwrap()
        .interpreter_mut()
        .unwrap()
        .write(32, &[0u32.to_le_bytes(), 10u32.to_le_bytes()].concat())
        .unwrap();
    advance(&mut runtime, task, end);
    assert_eq!(runtime.numbers().shared, [99, 98, 97]);
    assert_eq!(
        runtime
            .object(task)
            .unwrap()
            .interpreter()
            .unwrap()
            .read(24, 4)
            .unwrap(),
        1u32.to_le_bytes()
    );
}
