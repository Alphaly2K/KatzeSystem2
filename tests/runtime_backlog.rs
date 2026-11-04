use katze_system2::{
    Result,
    expression::State,
    kcs::Program,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
    settings::SystemSettings,
};

fn ins(code: &mut Vec<u8>, op: u8, args: &[(u8, u32)]) {
    code.push(op);
    for &(flags, value) in args {
        code.push(flags);
        code.extend(value.to_le_bytes());
    }
}
fn call(code: &mut Vec<u8>, index: u32, args: &[u32]) {
    for &arg in args {
        ins(code, 0x25, &[(3, arg)]);
    }
    ins(code, 0x2b, &[(3, index), (3, args.len() as u32 * 4)]);
}
fn store(code: &mut Vec<u8>, address: u32) {
    ins(code, 0x26, &[(3, address)]);
}
struct Loader;
impl ProgramLoader for Loader {
    fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
        let mut code = Vec::new();
        let mut data = vec![0x5a; 512];
        ins(&mut code, 0x25, &[(3, 0x12345678)]);
        if name == b"create" {
            call(&mut code, 696, &[u32::MAX]);
            store(&mut code, 0);
            call(&mut code, 238, &[7]);
            store(&mut code, 4);
            data[64..74].copy_from_slice(b"scene.cst\0");
            let name = "tag※Alice\0".as_bytes();
            data[96..96 + name.len()].copy_from_slice(name);
            data[128..132].copy_from_slice(b"abc\0");
            call(&mut code, 977, &[1, 64, 5, 96, 128, 1, 0, 0x99887766]);
            call(&mut code, 850, &[2, 9, 1]);
            store(&mut code, 8);
        } else {
            call(&mut code, 876, &[1]);
            call(&mut code, 851, &[2, 99, 1]);
            store(&mut code, 0);
            call(&mut code, 851, &[2, 9, 1]);
            store(&mut code, 4);
            call(&mut code, 700, &[1, 0x80000020, 0]);
            call(&mut code, 700, &[1, 64, u32::MAX]);
            for index in [697, 698, 700, 785, 876, 977] {
                call(&mut code, index, &[0]);
            }
            call(&mut code, 850, &[0]);
            store(&mut code, 8);
            call(&mut code, 851, &[2, 9, 0]);
            store(&mut code, 12);
        }
        call(&mut code, 65, &[]);
        call(&mut code, 697, &[1]);
        call(&mut code, 65, &[]);
        code.push(0x3b);
        Ok(Some(Program::from_code(&code, data, 64)?))
    }
}

#[test]
fn scripts_share_logs_and_save_records_across_tasks_and_keep_void_return_slots() {
    for budget in [1, 4096] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory {
                programs: Loader,
                external_bytes: 64,
            },
        );
        runtime.set_budget(budget, 1 << 20);
        let mut settings = SystemSettings::default();
        settings.backlog.name_prefix = b"<".to_vec();
        settings.backlog.name_suffix = b">".to_vec();
        runtime.host_mut().system_settings = Some(settings);
        let first = runtime.create(0, kind::ORDINARY, b"create", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let original = runtime.backlog(1).unwrap().serialize().unwrap();
        assert_eq!(
            runtime.backlog(1).unwrap().name_for_row(0),
            Some(b"<Alice>".as_slice())
        );
        assert_eq!(
            runtime
                .object(first)
                .unwrap()
                .interpreter()
                .unwrap()
                .stack_bytes(),
            0x12345678u32.to_le_bytes()
        );
        runtime.destroy(first).unwrap();
        assert!(runtime.backlog(1).is_some());
        runtime
            .host_mut()
            .system_settings
            .as_mut()
            .unwrap()
            .backlog
            .name_prefix
            .clear();
        let second = runtime.create(0, kind::ORDINARY, b"restore", b"").unwrap();
        runtime.update(1, 1).unwrap();
        assert_eq!(runtime.backlog(1).unwrap().serialize().unwrap(), original);
        let vm = runtime.object(second).unwrap().interpreter().unwrap();
        assert_eq!(
            vm.read(0, 16).unwrap().as_ref(),
            [0u32, 1, 0, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert_eq!(vm.read(0x80000020, 8).unwrap().as_ref(), b"<Alice>\0");
        assert_eq!(vm.read(64, 1).unwrap().as_ref(), &[0]);
        assert_eq!(vm.stack_bytes(), 0x12345678u32.to_le_bytes());
        runtime.update(2, 1).unwrap();
        assert!(runtime.backlog(1).is_none());
    }
}
