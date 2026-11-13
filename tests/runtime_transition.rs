use katze_system2::{
    Result,
    expression::State,
    graphics::{GraphicsFactory, RenderTarget},
    kcs::Program,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};

struct Loader(Program);
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

fn instruction(code: &mut Vec<u8>, opcode: u8, words: &[u32]) {
    code.push(opcode);
    for &word in words {
        code.push(3);
        code.extend(word.to_le_bytes());
    }
}

fn call(code: &mut Vec<u8>, index: u32, arguments: &[u32]) {
    for &argument in arguments {
        instruction(code, 0x25, &[argument]);
    }
    instruction(code, 0x2b, &[index, arguments.len() as u32 * 4]);
}

#[test]
fn ordinary_transition_calls_prepare_play_query_and_finish_image_effects() {
    for budget in [1, 4096] {
        for effect in [kind::WIPE, kind::FADE, kind::WIPE_ALPHA] {
            let mut code = Vec::new();
            instruction(&mut code, 0x25, &[0xdeadbeef]);
            call(&mut code, 196, &[0, u32::MAX]);
            call(&mut code, 196, &[1, 64]);
            call(&mut code, 197, &[1]);
            call(&mut code, 200, &[1]);
            instruction(&mut code, 0x26, &[0]);
            call(&mut code, 65, &[]);
            call(&mut code, 200, &[1]);
            instruction(&mut code, 0x26, &[4]);
            call(&mut code, 199, &[1, 128]);
            call(&mut code, 203, &[1]);
            call(&mut code, 200, &[1]);
            instruction(&mut code, 0x26, &[16]);
            call(&mut code, 197, &[1]);
            call(&mut code, 198, &[1]);
            call(&mut code, 200, &[1]);
            instruction(&mut code, 0x26, &[8]);
            instruction(&mut code, 0x26, &[12]);
            instruction(&mut code, 0x3b, &[]);

            let mut data = vec![0; 256];
            data[128..192].fill(0x75);
            let mut rt = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                State::new(0),
                GraphicsFactory {
                    fallback: KcsFactory::new(Loader(
                        Program::from_code(&code, data, 256).unwrap(),
                    )),
                },
            );
            rt.set_budget(budget, 1 << 20);
            rt.drawing_mut().enabled = true;
            rt.drawing_mut().active = true;
            rt.drawing_mut().target = Some(RenderTarget::new([3, 3]).unwrap());
            let mut handles = Vec::new();
            for color in [0xff000000u32, 0xff806040, 0xff000080] {
                let handle = rt.images_mut().create(0, [1, 1]).unwrap();
                handles.push(handle);
                rt.images_mut()
                    .get_mut(handle)
                    .unwrap()
                    .pixels_mut()
                    .copy_from_slice(&color.to_le_bytes());
            }
            assert_eq!(rt.create(0, effect, b"effect", b"").unwrap(), 1);
            assert_eq!(rt.create(0, kind::ORDINARY, b"caller", b"").unwrap(), 2);
            let parameters: Vec<_> = match effect {
                kind::WIPE => vec![
                    2, handles[0], 0, 0, handles[1], 0, 0, handles[2], 1, 1, 1, 1, 0, 0,
                ],
                kind::FADE => vec![2, handles[0], 0, 0, handles[1], 0, 0, 1, 1, 1, 1],
                _ => vec![2, handles[1], 0, 0, handles[2], 1, 1, 1, 1, 1, 0, 0],
            }
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
            rt.object_mut(2)
                .unwrap()
                .interpreter_mut()
                .unwrap()
                .write(64, &parameters)
                .unwrap();
            assert!(rt.update(10, 1).unwrap());
            assert!(rt.update(11, 1).unwrap());
            let vm = rt.object(2).unwrap().interpreter().unwrap();
            for (address, expected) in [(0, 1u32), (4, 1), (8, 0), (12, 0xdeadbeef), (16, 0)] {
                assert_eq!(
                    vm.read(address, 4).unwrap().as_ref(),
                    &expected.to_le_bytes()
                );
            }
            assert_eq!(vm.read(64, parameters.len()).unwrap().as_ref(), parameters);
            assert_eq!(vm.read(128, 64).unwrap().as_ref(), &[0x75; 64]);
            assert_eq!(
                rt.drawing().target.as_ref().unwrap().pixel(1, 1),
                Some(0xff806040)
            );
        }
    }
}
