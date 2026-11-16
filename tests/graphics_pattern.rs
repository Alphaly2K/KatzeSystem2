use katze_system2::{
    Result,
    expression::State,
    graphics::{Drawables, GraphicsFactory, PlaneObject, RenderTarget},
    kcs::{Host, Interpreter, NativeReturn, Program},
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, TaskObject, kind},
    task::{Command, CommandBuffers, CommandOutcome},
};

#[derive(Default)]
struct PatternHost {
    drawables: Drawables,
    packets: Vec<Vec<u32>>,
    next_child: u32,
}

impl Host for PatternHost {
    fn call(&mut self, _: u32, _: &[u8], _: &mut Interpreter) -> Result<NativeReturn> {
        unreachable!()
    }
    fn drawables(&mut self) -> Option<&mut Drawables> {
        Some(&mut self.drawables)
    }
    fn create_task_child(&mut self, kind: u32, name: &[u8], context: &[u8]) -> Result<u32> {
        assert!(context.is_empty());
        let mut packet = vec![7, 7, 1, kind, 0, 0, name.len() as u32];
        packet.extend(name.iter().map(|&b| b as u32));
        self.packets.push(packet);
        let child = self.next_child;
        self.next_child += 1;
        Ok(child)
    }
    fn dispatch_task_command(
        &mut self,
        command: Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        let bytes = buffers.data1.as_deref().unwrap();
        let mut packet = vec![
            command.sender,
            command.target,
            command.code,
            command.data[0],
            0,
            command.data[2],
            bytes.len() as u32 / 4,
        ];
        packet.extend(
            bytes
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap())),
        );
        self.packets.push(packet);
        *output = 0xdeadbeef;
        Ok(false)
    }
}

fn dispatch(
    plane: &mut PlaneObject,
    host: &mut PatternHost,
    code: u32,
    value: u32,
    bytes: Option<&mut [u8]>,
) -> Result<()> {
    let mut output = 0xabcdef01;
    assert_eq!(
        plane.dispatch_command_with_buffers(
            kind::PLANE,
            &Command {
                target: 7,
                sender: 99,
                code,
                data: [0, value, 0]
            },
            &mut CommandBuffers {
                data1: bytes,
                ..Default::default()
            },
            &mut output,
            host,
            1
        )?,
        CommandOutcome::finished(false)
    );
    assert_eq!(output, 0xabcdef01);
    Ok(())
}

#[test]
fn pattern_keeps_its_record_and_sends_rectangles_then_relative_coordinates_in_row_order()
-> Result<()> {
    let mut host = PatternHost {
        next_child: 100,
        ..Default::default()
    };
    let mut plane = PlaneObject::new(7);
    let record = [
        1.25f32.to_bits(),
        (-3.5f32).to_bits(),
        7f32.to_bits(),
        4f32.to_bits(),
        5f32.to_bits(),
        32,
        1,
        u32::MAX,
        2,
        2,
    ];
    let mut bytes: Vec<_> = record.into_iter().flat_map(u32::to_le_bytes).collect();
    dispatch(&mut plane, &mut host, 46, 0, Some(&mut bytes))?;
    assert_eq!(plane.pattern(), Some(&record));
    assert_eq!(
        (plane.shape_kind(), plane.size(), plane.offset()),
        (9, [0.; 2], [0.; 2])
    );
    assert_eq!(host.packets.len(), 12);
    for (index, packets) in host.packets.chunks_exact(3).enumerate() {
        assert_eq!(&packets[1][..7], &[7, 100 + index as u32, 41, 0, 0, 0, 8]);
        assert_eq!(
            &packets[1][7..],
            &[0, 0, 0, record[3], record[4], 32, 1, u32::MAX]
        );
        assert_eq!(&packets[2][..7], &[7, 100 + index as u32, 53, 0, 0, 0, 3]);
        assert_eq!(
            &packets[2][7..],
            &[
                (1.25f32 + 4. * (index % 2) as f32).to_bits(),
                (-3.5f32 + 5. * (index / 2) as f32).to_bits(),
                7f32.to_bits()
            ]
        );
    }
    host.packets.clear();
    dispatch(&mut plane, &mut host, 84, 2, None)?;
    dispatch(&mut plane, &mut host, 46, 0, None)?;
    assert!(host.drawables.get(7).is_none());
    assert_eq!(plane.pattern(), Some(&record));
    assert!(host.packets.is_empty());
    dispatch(&mut plane, &mut host, 84, 1, None)?;
    bytes[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
    dispatch(&mut plane, &mut host, 46, 0, Some(&mut bytes))?;
    assert_eq!(plane.pattern().unwrap()[8], u32::MAX);
    assert!(host.packets.is_empty());
    bytes[32..36].copy_from_slice(&1_000_001u32.to_le_bytes());
    assert!(dispatch(&mut plane, &mut host, 46, 0, Some(&mut bytes)).is_err());
    assert!(host.packets.is_empty());
    Ok(())
}

struct Loader(Program);
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}
fn instruction(code: &mut Vec<u8>, opcode: u8, values: &[u32]) {
    code.push(opcode);
    for value in values {
        code.push(3);
        code.extend(value.to_le_bytes());
    }
}
fn call(code: &mut Vec<u8>, native: u32, values: &[u32]) {
    for value in values {
        instruction(code, 0x25, &[*value]);
    }
    instruction(code, 0x2b, &[native, values.len() as u32 * 4]);
}

#[test]
fn native_pattern_children_inherit_parent_motion_and_survive_replacement_until_deletion()
-> Result<()> {
    let mut code = vec![];
    instruction(&mut code, 0x25, &[0xabcdef01]);
    call(&mut code, 433, &[0]);
    call(&mut code, 433, &[99, 2f32.to_bits(), 2f32.to_bits(), 2, 2]);
    call(&mut code, 433, &[1, 2f32.to_bits(), 2f32.to_bits(), 2, 2]);
    call(&mut code, 65, &[]);
    call(&mut code, 433, &[1, 1f32.to_bits(), 1f32.to_bits(), 1, 1]);
    code.push(0x3b);
    let program = Program::from_code(&code, vec![], 64)?;
    for budget in [1, 4096] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            GraphicsFactory {
                fallback: KcsFactory::new(Loader(program.clone())),
            },
        );
        runtime.set_budget(budget, 1 << 20);
        assert_eq!(runtime.create(0, kind::PLANE, b"pattern", b"")?, 1);
        let caller = runtime.create(0, kind::ORDINARY, b"create", b"")?;
        runtime.drawing_mut().target = Some(RenderTarget::new([4, 4])?);
        runtime.update(0, 1)?;
        let children: Vec<_> = runtime.tree().children(1).unwrap().collect();
        assert_eq!(children, [3, 4, 5, 6]);
        assert!(runtime.drawables().get(1).unwrap().triangles().is_empty());
        for (index, &child) in children.iter().enumerate() {
            let drawing = runtime.drawables().get(child).unwrap();
            assert!(drawing.canvas_triangles().is_some());
            assert_eq!(
                drawing.transform().base_point(),
                [-2. * (index % 2) as f32, -2. * (index / 2) as f32, 5000.]
            );
        }
        let mut position: Vec<_> = [3f32, 4., 5.]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        runtime.dispatch_command_with_buffers(
            0,
            Command {
                target: 1,
                sender: 0,
                code: 51,
                data: [0; 3],
            },
            &mut CommandBuffers {
                data1: Some(&mut position),
                ..Default::default()
            },
        )?;
        for &child in &children {
            assert_eq!(
                runtime
                    .drawables()
                    .get(child)
                    .unwrap()
                    .transform()
                    .position(),
                [3., 4., 5.]
            );
        }
        runtime.update(1, 1)?;
        assert_eq!(runtime.tree().children(1).unwrap().count(), 5);
        assert_eq!(
            runtime
                .object(caller)
                .unwrap()
                .interpreter()
                .unwrap()
                .stack_bytes(),
            0xabcdef01u32.to_le_bytes()
        );
        runtime.destroy(1)?;
        for child in children {
            assert!(runtime.drawables().get(child).is_none());
        }
    }
    Ok(())
}
