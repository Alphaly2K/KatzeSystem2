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

fn instruction(code: &mut Vec<u8>, opcode: u8, operands: &[(u8, u32)]) {
    code.push(opcode);
    for &(flags, value) in operands {
        code.push(flags);
        code.extend(value.to_le_bytes());
    }
}

fn call(code: &mut Vec<u8>, index: u32, args: &[(u8, u32)]) {
    for &arg in args {
        instruction(code, 0x25, &[arg]);
    }
    instruction(code, 0x2b, &[(3, index), (3, args.len() as u32 * 4)]);
}

#[test]
fn contexts_capture_languages_and_keep_shared_handles_until_explicit_release() {
    let mut code = vec![];
    instruction(&mut code, 0x25, &[(3, 0x1234_5678)]);
    call(&mut code, 323, &[(3, u32::MAX)]);
    instruction(&mut code, 0x26, &[(3, 0)]);
    call(&mut code, 326, &[(3, 0)]);
    instruction(&mut code, 0x26, &[(3, 12)]);
    call(&mut code, 326, &[(0x13, 0), (3, u32::MAX), (3, u32::MAX)]);
    instruction(&mut code, 0x26, &[(3, 16)]);
    call(&mut code, 112, &[(3, 0), (3, 0)]);
    instruction(&mut code, 0x26, &[(3, 4)]);
    call(&mut code, 65, &[]);
    call(&mut code, 323, &[]);
    instruction(&mut code, 0x26, &[(3, 8)]);
    call(&mut code, 65, &[]);
    call(&mut code, 324, &[(0x13, 0)]);
    call(&mut code, 324, &[(3, 0)]);
    code.push(0x3b);
    let program = Program::from_code(&code, vec![0; 20], 64).unwrap();
    for budget in [1, 4096] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Loader(program.clone())),
        );
        runtime.set_budget(budget, 1 << 20);
        runtime.host_mut().languages.prefixes = vec![
            b"jp".to_vec(),
            b"".to_vec(),
            b"en".to_vec(),
            b"jp\0ignored".to_vec(),
        ];
        let task = runtime.create(0, kind::ORDINARY, b"cst", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let first = 1;
        let context = runtime.cst_context(first).unwrap();
        assert_eq!(
            context.languages().collect::<Vec<_>>(),
            [b"".as_slice(), b"en", b"jp"]
        );
        assert!(context.script().is_none());
        runtime.host_mut().languages.prefixes = vec![b"cn".to_vec()];
        runtime.update(1, 1).unwrap();
        assert_eq!(runtime.cst_context(first).unwrap().languages().len(), 3);
        let second = 4;
        assert_eq!(
            runtime
                .cst_context(second)
                .unwrap()
                .languages()
                .collect::<Vec<_>>(),
            [b"cn"]
        );
        runtime.update(2, 1).unwrap();
        assert!(runtime.cst_context(first).is_none());
        let vm = runtime.object(task).unwrap().interpreter().unwrap();
        let handles: Vec<_> = vm
            .read(0, 20)
            .unwrap()
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        assert_eq!(handles, [first, 2, second, 0, u32::MAX]);
        assert_eq!(vm.stack_bytes(), 0x1234_5678u32.to_le_bytes());
        runtime.destroy(task).unwrap();
        assert!(runtime.cst_context(second).is_some());
    }
}

#[test]
fn cst_tables_merge_values_preserve_nonadjacent_records_and_clear_independently() {
    let mut code = vec![];
    instruction(&mut code, 0x25, &[(3, 0x1234_5678)]);
    call(&mut code, 323, &[]);
    instruction(&mut code, 0x26, &[(3, 0)]);
    for (slot, index, name, value) in [
        (4, 328, 64, 7),
        (8, 328, 80, u32::MAX),
        (12, 329, 64, 1),
        (16, 329, 80, 0x8000_0000),
        (20, 329, 96, 4),
        (24, 329, 64, 3),
    ] {
        call(&mut code, index, &[(0x13, 0), (3, name), (3, value)]);
        instruction(&mut code, 0x26, &[(3, slot)]);
    }
    call(&mut code, 332, &[(0x13, 0), (3, 80)]);
    instruction(&mut code, 0x26, &[(3, 28)]);
    call(&mut code, 65, &[]);
    call(&mut code, 330, &[(0x13, 0)]);
    call(&mut code, 332, &[(0x13, 0), (3, 64)]);
    instruction(&mut code, 0x26, &[(3, 32)]);
    call(&mut code, 65, &[]);
    call(&mut code, 331, &[(0x13, 0)]);
    for (slot, index) in [(36, 329), (40, 328)] {
        call(&mut code, index, &[(0x13, 0), (3, 112)]);
        instruction(&mut code, 0x26, &[(3, slot)]);
    }
    call(&mut code, 329, &[(0x13, 0), (3, 64), (3, 9)]);
    instruction(&mut code, 0x26, &[(3, 44)]);
    for (slot, index) in [(48, 332), (52, 328), (56, 329)] {
        call(&mut code, index, &[(3, 0)]);
        instruction(&mut code, 0x26, &[(3, slot)]);
    }
    call(&mut code, 330, &[(3, 0)]);
    call(&mut code, 331, &[(3, 0)]);
    code.push(0x3b);
    let mut data = vec![0; 128];
    data[64..70].copy_from_slice(b"A.cst\0");
    data[80..86].copy_from_slice(b"a.alt\0");
    data[96..102].copy_from_slice(b"B.cst\0");
    data[112..120].copy_from_slice(b".hidden\0");
    let program = Program::from_code(&code, data, 64).unwrap();
    for budget in [1, 4096] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Loader(program.clone())),
        );
        runtime.set_budget(budget, 1 << 20);
        let task = runtime.create(0, kind::ORDINARY, b"tables", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let context = runtime.cst_context(1).unwrap();
        assert_eq!(
            context.registrations().collect::<Vec<_>>(),
            [(b"A".as_slice(), u32::MAX)]
        );
        assert_eq!(
            context.history().collect::<Vec<_>>(),
            [(b"A".as_slice(), 0x8000_0000), (b"B", 4), (b"A", 3)]
        );
        runtime.update(1, 1).unwrap();
        let context = runtime.cst_context(1).unwrap();
        assert_eq!(context.registrations().len(), 0);
        assert_eq!(context.history().len(), 3);
        runtime.update(2, 1).unwrap();
        let context = runtime.cst_context(1).unwrap();
        assert_eq!(context.registrations().len(), 0);
        assert_eq!(
            context.history().collect::<Vec<_>>(),
            [(b"A".as_slice(), 9)]
        );
        let vm = runtime.object(task).unwrap().interpreter().unwrap();
        let values: Vec<_> = vm
            .read(4, 56)
            .unwrap()
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        assert_eq!(
            values,
            [
                1,
                1,
                0,
                0,
                1,
                2,
                u32::MAX,
                0,
                u32::MAX,
                1,
                0,
                0,
                0,
                u32::MAX
            ]
        );
        assert_eq!(vm.stack_bytes(), 0x1234_5678u32.to_le_bytes());
        runtime.destroy(task).unwrap();
        assert_eq!(runtime.cst_context(1).unwrap().history().len(), 1);
    }
}
