use katze_system2::{
    Error, Result,
    graphics::{Drawable, Drawables, PlaneObject},
    kcs::{Host, Interpreter, NativeReturn},
    runtime::TaskObject,
    task::{Command, CommandBuffers},
};

#[derive(Default)]
struct Services {
    drawings: Drawables,
    hides: usize,
    fail_at: usize,
}

impl Host for Services {
    fn call(&mut self, _: u32, _: &[u8], _: &mut Interpreter) -> Result<NativeReturn> {
        unreachable!()
    }

    fn drawables(&mut self) -> Option<&mut Drawables> {
        Some(&mut self.drawings)
    }

    fn dispatch_task_command(
        &mut self,
        command: Command,
        _: &mut CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        match command.code {
            34 => *output = 8,
            127 => {}
            47 => {
                assert_eq!((command.target, command.data), (19, [0; 3]));
                self.hides += 1;
                if self.hides == self.fail_at {
                    return Err(Error::new(0, "合成提示隐藏失败"));
                }
            }
            _ => unreachable!(),
        }
        Ok(true)
    }
}

#[test]
fn disabled_state_keeps_the_two_hide_calls_and_the_state_written_between_them() {
    for fail_at in [0, 1, 2] {
        let mut services = Services::default();
        services.drawings.insert(7, Drawable::default()).unwrap();
        let mut plane = PlaneObject::new(7);
        let mut output = 0;
        let command = |code, value| Command {
            target: 7,
            sender: 99,
            code,
            data: [0, value, 0],
        };
        let mut slots: Vec<_> = [0u32, 0, 0, 0, 0, 0, 0, 0, 0, 1, 71, 81]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        plane
            .dispatch_command_with_buffers(
                2,
                &command(72, 0),
                &mut CommandBuffers {
                    data0: Some(&mut slots),
                    data1: None,
                },
                &mut output,
                &mut services,
                1,
            )
            .unwrap();
        plane
            .dispatch_command(2, &command(77, 19), &mut output, &mut services, 1)
            .unwrap();
        services.hides = 0;
        services.fail_at = fail_at;
        let result = plane.dispatch_command(2, &command(73, 3), &mut output, &mut services, 1);
        assert_eq!(result.is_err(), fail_at != 0);
        assert_eq!(services.hides, if fail_at == 1 { 1 } else { 2 });
        plane
            .dispatch_command(2, &command(74, 0), &mut output, &mut services, 1)
            .unwrap();
        assert_eq!(output, if fail_at == 1 { 0 } else { 3 });
        assert_eq!(
            services.drawings.get(7).unwrap().transform().position(),
            [71., 81., 0.]
        );
    }
}
