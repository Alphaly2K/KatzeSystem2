use katze_system2::{
    Error, Result,
    expression::State,
    input::{InputFactory, KeyEvent, KeyEventKind},
    kcs::{CoreHost, OrdinaryTask, Program},
    runtime::{ObjectFactory, OrdinaryObject, RootObject, Runtime, TaskObject, kind},
    task::{Command, CommandBuffers},
};

struct ReceiverFactory;

impl ObjectFactory for ReceiverFactory {
    fn create(&mut self, id: u32, kind: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        if kind != kind::ORDINARY {
            return Err(Error::new(0, "合成工厂未提供该类型"));
        }
        let mut code = Vec::new();
        for (opcode, operands) in [
            (0x25, &[32][..]),
            (0x2b, &[71, 4][..]),
            (0x26, &[0][..]),
            (0x2f, &[32][..]),
            (0x25, &[0][..]),
            (0x2b, &[108, 8][..]),
            (0x29, &[0][..]),
        ] {
            code.push(opcode);
            for operand in operands {
                code.push(3);
                code.extend(u32::to_le_bytes(*operand));
            }
        }
        let mut task = OrdinaryTask::new(Program::from_code(&code, vec![0; 64], 64)?, vec![])?;
        task.task_id = id;
        task.interpreter
            .resume(&mut CoreHost::new(&mut State::new(0)), 100)?;
        Ok(Box::new(OrdinaryObject::new(task)))
    }
}

#[test]
fn button_selection_press_and_release_reach_a_waiting_ordinary_vm() -> Result<()> {
    for budget in [1, 4096] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(RootObject::new(1, 0)),
            State::new(8),
            InputFactory {
                fallback: ReceiverFactory,
            },
        );
        runtime.set_budget(budget, 1 << 20);
        let input = runtime.create(0, kind::INPUT, b"input", b"")?;
        let receiver = runtime.create(0, kind::ORDINARY, b"receiver", b"")?;
        let button = runtime.create(0, kind::BUTTON, b"button", b"")?;
        runtime.input_coordinates_mut().logical_size = [100, 100];
        let mut region = [0u32, 0, 10, 10, 5, 77]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        runtime.dispatch_command_with_buffers(
            0,
            Command {
                target: button,
                sender: receiver,
                code: 90,
                data: [0; 3],
            },
            &mut CommandBuffers {
                data0: None,
                data1: Some(&mut region),
                external: None,
            },
        )?;
        runtime.update_input_selection(input, Some([5, 5]))?;
        let message = |runtime: &Runtime<InputFactory<ReceiverFactory>>| {
            runtime
                .object(receiver)
                .unwrap()
                .interpreter()
                .unwrap()
                .read(32, 16)
                .unwrap()
                .chunks_exact(4)
                .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
                .collect::<Vec<_>>()
        };
        assert_eq!(message(&runtime), [button, 0xffff_0005, button, 1]);
        for (pressed, kind) in [(1, 0xffff_0003), (0, 0xffff_0004)] {
            assert!(runtime.dispatch_input_key(
                input,
                KeyEvent {
                    kind: KeyEventKind::Change,
                    code: 4,
                    key: 0,
                    pressed,
                    mask: 2,
                    point: [5, 5],
                }
            )?);
            assert_eq!(message(&runtime), [button, kind, button, 0]);
            assert_eq!(
                runtime.input_state(input).unwrap().events().captures()[0],
                if pressed != 0 { button } else { 0 }
            );
            assert!(
                runtime
                    .object(receiver)
                    .unwrap()
                    .interpreter()
                    .unwrap()
                    .is_waiting_for_message()
            );
        }
        assert_eq!(runtime.object(receiver).unwrap().clock().time, 0);
    }
    Ok(())
}
