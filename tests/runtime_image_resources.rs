use katze_system2::{
    Result,
    expression::State,
    kcs::{Interpreter, Program},
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
    task::{Command, CommandBuffers},
};

struct Loader(Program);

impl ProgramLoader for Loader {
    fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
        Ok(Some(if name == b"empty" {
            Program::from_code(&[0x3b], vec![0; 128], 128)?
        } else {
            self.0.clone()
        }))
    }
}

fn ins(code: &mut Vec<u8>, op: u8, args: &[u32]) {
    code.push(op);
    for &value in args {
        code.push(3);
        code.extend(value.to_le_bytes());
    }
}

fn call(code: &mut Vec<u8>, native: u32, args: &[u32]) {
    for &value in args {
        ins(code, 0x25, &[value]);
    }
    ins(code, 0x2b, &[native, args.len() as u32 * 4]);
}

fn handle_call(code: &mut Vec<u8>, native: u32, args: &[u32]) {
    code.extend([0x25, 0x13]);
    code.extend(0u32.to_le_bytes());
    for &value in args {
        ins(code, 0x25, &[value]);
    }
    ins(code, 0x2b, &[native, (args.len() as u32 + 1) * 4]);
}

fn runtime(code: &[u8], budget: usize) -> Runtime<KcsFactory<Loader>> {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        KcsFactory::new(Loader(Program::from_code(code, vec![0; 128], 128).unwrap())),
    );
    runtime.set_budget(budget, 1 << 20);
    runtime
}

fn vm(runtime: &Runtime<KcsFactory<Loader>>, task: u32) -> &Interpreter {
    runtime.object(task).unwrap().interpreter().unwrap()
}

fn word(vm: &Interpreter, at: u32) -> u32 {
    u32::from_le_bytes(vm.read(at, 4).unwrap().try_into().unwrap())
}

#[test]
fn resource_queries_preserve_native_stack_protocol_and_overlapping_outputs() {
    for budget in [1, 4096] {
        for width in [0, 2] {
            let mut code = Vec::new();
            call(&mut code, 733, &[width, 3]);
            ins(&mut code, 0x26, &[0]);
            ins(&mut code, 0x25, &[0xdead_beef]);
            handle_call(&mut code, 712, &[17, 18]);
            handle_call(&mut code, 714, &[0x8000_0001, 0x8000_0002]);
            handle_call(&mut code, 715, &[24, 28]);
            handle_call(&mut code, 717, &[32, 36]);
            ins(&mut code, 0x26, &[4]);
            handle_call(
                &mut code,
                724,
                &[17, u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX],
            );
            ins(&mut code, 0x26, &[8]);
            for native in [711, 712, 714, 715] {
                call(&mut code, native, &[0]);
            }
            for (native, at) in [(717, 12), (724, 40)] {
                call(&mut code, native, &[0]);
                ins(&mut code, 0x26, &[at]);
            }
            ins(&mut code, 0x26, &[44]);
            call(&mut code, 65, &[]);
            let mut runtime = runtime(&code, budget);
            let task = runtime.create(0, kind::ORDINARY, b"queries", b"").unwrap();
            runtime
                .object_mut(task)
                .unwrap()
                .interpreter_mut()
                .unwrap()
                .replace_external(vec![0xa5; 16])
                .unwrap();
            runtime.update(0, 1).unwrap();
            let vm = vm(&runtime, task);
            assert_eq!(vm.read(17, 5).unwrap(), [width as u8, 3, 0, 0, 0]);
            assert_eq!(
                vm.read(0x8000_0000, 7).unwrap(),
                [0xa5, width as u8, 3, 0, 0, 0, 0xa5]
            );
            assert_eq!(vm.read(24, 16).unwrap(), [0; 16]);
            for at in [4, 12, 40] {
                assert_eq!(word(vm, at), 1);
            }
            assert_eq!(word(vm, 8), 0);
            assert_eq!(word(vm, 44), 0xdead_beef);
            assert!(vm.stack_bytes().is_empty());
            let handle = word(vm, 0);
            assert_ne!(handle, 0);
            assert!(runtime.images().get(handle).is_err());
            let image = runtime.image_resource(handle).unwrap();
            assert_eq!(image.info().size, [width, 3]);
            assert_eq!(image.surface().pixels(), vec![0; width as usize * 3 * 4]);
        }
    }
}

#[test]
fn resources_release_owner_registration_and_keep_independent_references() {
    for budget in [1, 4096] {
        let mut code = Vec::new();
        call(&mut code, 733, &[2, 3]);
        ins(&mut code, 0x26, &[0]);
        call(&mut code, 65, &[]);
        handle_call(&mut code, 711, &[]);
        call(&mut code, 65, &[]);
        let mut runtime = runtime(&code, budget);
        for release_native in [false, true] {
            let task = runtime.create(0, kind::ORDINARY, b"create", b"").unwrap();
            runtime.update(0, 1).unwrap();
            let handle = word(vm(&runtime, task), 0);
            runtime.retain_image_resource(handle).unwrap();
            if release_native {
                runtime.update(1, 1).unwrap();
            }
            runtime.destroy(task).unwrap();
            assert!(runtime.image_resource(handle).is_ok());
            runtime.release_image_resource_reference(handle).unwrap();
            assert!(runtime.image_resource(handle).is_err());
            runtime.update(2, 1).unwrap();
        }
        let task = runtime.create(0, kind::ORDINARY, b"reload", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let handle = word(vm(&runtime, task), 0);
        runtime
            .dispatch_command_with_buffers(
                0,
                Command {
                    target: task,
                    sender: 0,
                    code: 161,
                    data: [0, 1, 0],
                },
                &mut CommandBuffers {
                    data1: Some(&mut b"empty".to_vec()),
                    ..CommandBuffers::default()
                },
            )
            .unwrap();
        assert!(runtime.image_resource(handle).is_err());
        runtime.destroy(task).unwrap();
        runtime.update(1, 1).unwrap();
        let task = runtime.create(0, kind::ORDINARY, b"release", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let handle = word(vm(&runtime, task), 0);
        runtime.destroy(task).unwrap();
        assert!(runtime.image_resource(handle).is_err());
        runtime.release_image_resource_reference(0).unwrap();
    }
}
