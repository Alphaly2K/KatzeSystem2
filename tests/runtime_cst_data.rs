use katze_system2::{
    Result,
    expression::State,
    kcs::Program,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};

struct Loader(Program);
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}
fn ins(code: &mut Vec<u8>, opcode: u8, args: &[(u8, u32)]) {
    code.push(opcode);
    for &(flags, value) in args {
        code.push(flags);
        code.extend(value.to_le_bytes());
    }
}
fn call(code: &mut Vec<u8>, native: u32, args: &[(u8, u32)]) {
    for arg in args {
        ins(code, 0x25, &[*arg]);
    }
    ins(code, 0x2b, &[(3, native), (3, args.len() as u32 * 4)]);
}
fn value(code: &mut Vec<u8>, native: u32, args: &[(u8, u32)], output: u32) {
    call(code, native, args);
    ins(code, 0x26, &[(3, output)]);
}
fn record(code: &mut Vec<u8>, container: u32, id: u32, address: u32, length: u32) {
    call(
        code,
        244,
        &[(0x13, container), (3, id), (3, length), (3, address)],
    );
    ins(code, 0x2d, &[]);
}
fn history(code: &mut Vec<u8>, context: u32, name: u32) {
    call(code, 329, &[(0x13, context), (3, name), (3, name)]);
    ins(code, 0x2d, &[]);
}

#[test]
fn nested_history_containers_copy_independently_and_self_copy_clears_only_data() {
    let mut code = Vec::new();
    value(&mut code, 323, &[], 0);
    value(&mut code, 323, &[], 4);
    value(&mut code, 238, &[(3, 0)], 8);
    value(&mut code, 238, &[(3, 0x12345678)], 12);
    call(&mut code, 978, &[(0x13, 0), (0x13, 12)]);
    value(&mut code, 979, &[(0x13, 0), (3, 0)], 40);
    history(&mut code, 0, 128);
    record(&mut code, 12, 7, 192, 7);
    call(&mut code, 978, &[(0x13, 0), (0x13, 12)]);
    record(&mut code, 12, 9, 200, 5);
    call(&mut code, 246, &[(0x13, 12)]);
    value(&mut code, 979, &[(0x13, 0), (3, 0)], 16);
    value(&mut code, 979, &[(0x13, 0), (3, 0)], 20);
    record(&mut code, 16, 11, 208, 5);
    history(&mut code, 4, 160);
    call(&mut code, 978, &[(0x13, 4), (0x13, 20)]);
    history(&mut code, 0, 144);
    call(&mut code, 978, &[(0x13, 0), (0x13, 16)]);
    call(&mut code, 981, &[(0x13, 0), (0x13, 4)]);
    call(&mut code, 324, &[(0x13, 4)]);
    value(&mut code, 979, &[(0x13, 0), (3, 0)], 24);
    value(&mut code, 979, &[(0x13, 0), (3, 1)], 28);
    call(&mut code, 981, &[(0x13, 0), (0x13, 0)]);
    value(&mut code, 979, &[(0x13, 0), (3, 0)], 32);
    value(&mut code, 979, &[(0x13, 0), (3, 1)], 36);
    call(
        &mut code,
        385,
        &[(0x13, 8), (3, 10), (0x13, 0), (3, 1), (3, 1)],
    );
    ins(&mut code, 0x2d, &[]);
    call(&mut code, 978, &[(3, 0), (3, u32::MAX)]);
    value(&mut code, 979, &[(3, 0), (3, u32::MAX)], 44);
    call(&mut code, 981, &[(3, 0), (3, u32::MAX)]);
    call(&mut code, 981, &[(0x13, 0), (3, 0)]);
    code.push(0x3b);
    let mut data = vec![0; 224];
    for (at, text) in [
        (128, b"first\0".as_slice()),
        (144, b"second\0"),
        (160, b"different\0"),
        (192, b"payload"),
        (200, b"later"),
        (208, b"extra"),
    ] {
        data[at..at + text.len()].copy_from_slice(text);
    }
    let program = Program::from_code(&code, data, 128).unwrap();
    for budget in [1, 4096] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Loader(program.clone())),
        );
        runtime.set_budget(budget, 1 << 20);
        let creator = runtime.create(0, kind::ORDINARY, b"main", b"").unwrap();
        runtime.update(0, 0).unwrap();
        let vm = runtime.object(creator).unwrap().interpreter().unwrap();
        let word = |at| u32::from_le_bytes(vm.read(at, 4).unwrap().as_ref().try_into().unwrap());
        let handles = [word(16), word(20), word(24), word(28)];
        assert!(handles.windows(2).all(|pair| pair[0] < pair[1]));
        for handle in handles {
            let container = runtime.host().data_container(handle).unwrap();
            assert_eq!(container.tag(), 0x12345678);
            assert_eq!(container.record(7).unwrap().unwrap(), b"payload");
            assert_eq!(container.record(9).unwrap(), None);
            assert_eq!(
                container.record(11).unwrap(),
                if handle == word(16) || handle == word(28) {
                    Some(b"extra".as_slice())
                } else {
                    None
                }
            );
        }
        for at in [32, 36, 40, 44] {
            assert_eq!(word(at), 0);
        }
        let root = word(8);
        let records = runtime
            .host()
            .data_container(root)
            .unwrap()
            .record(10)
            .unwrap()
            .unwrap();
        assert_eq!(records.len(), 100);
        assert_eq!(&records[4..10], b"first\0");
        assert_eq!(&records[52..59], b"second\0");
        assert_eq!(u32::from_le_bytes(records[36..40].try_into().unwrap()), 128);
        assert_eq!(u32::from_le_bytes(records[84..88].try_into().unwrap()), 144);
        assert_eq!(&records[44..52], &[0; 8]);
        assert_eq!(&records[92..100], &[0; 8]);
        assert!(vm.stack_bytes().is_empty());
        runtime.destroy(creator).unwrap();
        assert_eq!(
            runtime
                .host()
                .data_container(handles[0])
                .unwrap()
                .record(7)
                .unwrap()
                .unwrap(),
            b"payload"
        );
    }
}
