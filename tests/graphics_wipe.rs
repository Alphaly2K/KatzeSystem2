use katze_system2::{
    Error, Result,
    expression::State,
    graphics::{GraphicsFactory, RenderTarget, WipeObject},
    kcs::{Host, Interpreter, NativeReturn},
    runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, kind},
    settings::SystemSettings,
    task::{Command, CommandBuffers},
};

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂没有该类型"))
    }
}

fn command(id: u32, code: u32) -> Command {
    Command {
        target: id,
        sender: 0,
        code,
        data: [0; 3],
    }
}

fn image(rt: &mut Runtime<GraphicsFactory<Reject>>, size: [u32; 2], words: &[u32]) -> u32 {
    let handle = rt.images_mut().create(0, size).unwrap();
    for (out, word) in rt
        .images_mut()
        .get_mut(handle)
        .unwrap()
        .pixels_mut()
        .chunks_exact_mut(4)
        .zip(words)
    {
        out.copy_from_slice(&word.to_le_bytes());
    }
    handle
}

#[test]
fn mask_transition_submits_local_pixels_then_restores_the_full_target() {
    let mut rt = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory { fallback: Reject },
    );
    rt.host_mut().system_settings = Some(SystemSettings::default());
    rt.drawing_mut().enabled = true;
    rt.drawing_mut().active = true;
    rt.drawing_mut().target = Some(RenderTarget::new([6, 4]).unwrap());
    let source = image(
        &mut rt,
        [3, 2],
        &[
            0xff000000, 0xff000000, 0xff000000, 0xff000000, 0xff000000, 0x00112233,
        ],
    );
    let target = image(&mut rt, [3, 2], &[0xff806040; 6]);
    let mask = image(
        &mut rt,
        [2, 2],
        &[0xabcdef00, 0x000000ff, 0xff000040, 0x12345680],
    );
    let id = rt.create(0, kind::WIPE, b"wipe", b"").unwrap();
    let mut parameters: Vec<_> = [4, source, 0, 0, target, 0, 0, mask, 1, 1, 99, 99, 1, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    rt.dispatch_command_with_buffers(
        0,
        command(id, 280),
        &mut CommandBuffers {
            data1: Some(&mut parameters),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        rt.images().get(source).unwrap().surface().pixel(2, 1),
        Some(0x00806040)
    );
    assert!(rt.update(10, 1).unwrap());
    rt.dispatch_command(0, command(id, 263)).unwrap();
    assert_eq!(rt.dispatch_command(0, command(id, 265)).unwrap(), 1);
    assert!(rt.update(12, 1).unwrap());
    let canvas = rt.drawing().target.as_ref().unwrap();
    assert_eq!(canvas.pixel(1, 1), Some(0));
    assert_eq!(canvas.pixel(2, 1), Some(0xff201810));
    assert_eq!(canvas.pixel(3, 1), Some(0x7f806040));
    assert_eq!(canvas.pixel(2, 2), Some(0xff000000));
    assert_eq!(canvas.pixel(3, 2), Some(0xff7f5f3f));
    assert!(rt.update(14, 1).unwrap());
    assert_eq!(rt.dispatch_command(0, command(id, 265)).unwrap(), 0);
    for y in 1..3 {
        for x in 1..4 {
            assert_eq!(
                rt.drawing().target.as_ref().unwrap().pixel(x, y),
                Some(0xff806040)
            );
        }
    }

    let replacement = image(&mut rt, [3, 2], &[0x80706050; 6]);
    let mut replacement_parameters: Vec<_> = [0, source, 0, 0, replacement]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    rt.dispatch_command_with_buffers(
        0,
        command(id, 266),
        &mut CommandBuffers {
            data1: Some(&mut replacement_parameters),
            ..Default::default()
        },
    )
    .unwrap();
    rt.dispatch_command(0, command(id, 267)).unwrap();
    assert_eq!(
        rt.drawing().target.as_ref().unwrap().pixel(1, 1),
        Some(0xff806040)
    );
    rt.dispatch_command(0, command(id, 263)).unwrap();
    rt.dispatch_command(0, command(id, 267)).unwrap();
    assert_eq!(rt.dispatch_command(0, command(id, 265)).unwrap(), 0);
    assert_eq!(
        rt.drawing().target.as_ref().unwrap().pixel(1, 1),
        Some(0x80706050)
    );
    rt.host_mut().system_settings.as_mut().unwrap().effects[..4]
        .copy_from_slice(&4u32.to_le_bytes());
    rt.dispatch_command(0, command(id, 263)).unwrap();
    assert!(rt.update(15, 1).unwrap());
    assert_eq!(rt.dispatch_command(0, command(id, 265)).unwrap(), 0);
}

struct Empty;
impl Host for Empty {
    fn call(&mut self, _: u32, _: &[u8], _: &mut Interpreter) -> Result<NativeReturn> {
        Err(Error::new(0, "合成宿主不执行 native"))
    }
}

#[test]
fn missing_images_leave_the_transition_unprepared_and_preserve_command_results() {
    let mut wipe = WipeObject::new();
    let mut host = Empty;
    let mut output = 0x12345678;
    let mut parameters = [0; 56];
    wipe.dispatch_command_with_buffers(
        kind::WIPE,
        &command(1, 280),
        &mut CommandBuffers {
            data1: Some(&mut parameters),
            ..Default::default()
        },
        &mut output,
        &mut host,
        1,
    )
    .unwrap();
    assert!(!wipe.is_ready());
    assert_eq!(wipe.duration(), 0);
    for code in [263, 264, 267] {
        wipe.dispatch_command(kind::WIPE, &command(1, code), &mut output, &mut host, 1)
            .unwrap();
        assert_eq!(output, 0x12345678);
        assert!(!wipe.is_active());
    }
    wipe.dispatch_command(kind::WIPE, &command(1, 265), &mut output, &mut host, 1)
        .unwrap();
    assert_eq!(output, 0);
}
