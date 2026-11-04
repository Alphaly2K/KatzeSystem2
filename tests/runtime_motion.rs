use katze_system2::{
    Result,
    expression::State,
    kcs::Program,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};

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

fn store(code: &mut Vec<u8>, address: u32) {
    ins(code, 0x26, &[(3, address)]);
}

struct Loader(Program);

impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

fn word(vm: &katze_system2::kcs::Interpreter, address: u32) -> u32 {
    u32::from_le_bytes(vm.read(address, 4).unwrap().try_into().unwrap())
}

#[test]
fn ordinary_motion_properties_preserve_groups_save_direction_and_handle_lifetime() {
    let mut data = vec![0; 2048];
    let properties: [u32; 36] = std::array::from_fn(|i| 0x8000_0100 + i as u32);
    for (i, value) in properties.iter().enumerate() {
        data[128 + i * 4..132 + i * 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[(3, 0xdead_beef)]);
    call(&mut code, 294, &[(3, u32::MAX)]);
    store(&mut code, 1900);
    call(&mut code, 586, &[(0x13, 1900), (3, 0x8000_1234)]);
    call(&mut code, 586, &[(3, 0)]);
    call(&mut code, 112, &[(3, 0), (3, 0)]);
    store(&mut code, 1904);

    let offsets = [0, 12, 24, 36, 100, 112, 116, 128, 140];
    let mut inputs = vec![(0x13, 1900)];
    inputs.extend(offsets.map(|offset| (3, 128 + offset)));
    inputs[6] = (3, properties[28]);
    inputs[9] = (3, properties[35]);
    call(&mut code, 301, &inputs);
    call(&mut code, 303, &[(0x13, 1900)]);

    let mut outputs = vec![(0x13, 1900)];
    outputs.extend(offsets.map(|offset| (3, 512 + offset)));
    call(&mut code, 300, &outputs);
    call(&mut code, 307, &[(0x13, 1900), (3, 99), (3, 1)]);
    store(&mut code, 1908);
    for (index, address, arguments) in [
        (309, 1912, vec![(0x13, 1900), (3, 999)]),
        (310, 1916, vec![(0x13, 1900), (3, u32::MAX)]),
        (311, 1920, vec![(0x13, 1900)]),
    ] {
        call(&mut code, index, &arguments);
        store(&mut code, address);
    }
    let mut overlap = vec![(0x13, 1900)];
    overlap.extend([(3, 768); 9]);
    call(&mut code, 300, &overlap);
    let mut optional = vec![(0x13, 1900)];
    optional.extend(offsets.map(|offset| (3, 1024 + offset)));
    optional[9] = (3, u32::MAX);
    call(&mut code, 302, &optional);
    call(&mut code, 295, &[(0x13, 1900)]);
    call(&mut code, 294, &[]);
    store(&mut code, 1924);
    for index in 295..=314 {
        if index == 304 {
            continue;
        }
        call(&mut code, index, &[(3, 0)]);
        if matches!(index, 307 | 309..=311) {
            ins(&mut code, 0x2d, &[]);
        }
    }
    code.push(0x3b);
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
        let task = runtime.create(0, kind::ORDINARY, b"motion", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let vm = runtime.object(task).unwrap().interpreter().unwrap();
        let read =
            |at: u32, count: u32| (0..count).map(|i| word(vm, at + i * 4)).collect::<Vec<_>>();
        assert_eq!(read(512, 36), properties);
        assert_eq!(read(1024, 35), properties[..35]);
        assert_eq!(word(vm, 1164), 0);
        assert_eq!(word(vm, 768), properties[35]);
        assert_eq!(read(772, 2), properties[33..35]);
        assert_eq!(read(780, 13), properties[12..25]);
        assert_eq!(read(1908, 4), [0; 4]);
        assert_eq!(word(vm, 1900), 1);
        assert_eq!(word(vm, 1904), 2);
        assert!(word(vm, 1924) > word(vm, 1904));
        assert_eq!(vm.stack_bytes(), 0xdead_beefu32.to_le_bytes());
    }
}
