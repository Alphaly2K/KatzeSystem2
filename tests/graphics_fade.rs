use katze_system2::{
    Error, Result,
    expression::State,
    graphics::{FadeObject, GraphicsFactory, RenderTarget},
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

fn context(
    source: u32,
    target: u32,
    source_at: [u32; 2],
    target_at: [u32; 2],
    size: [u32; 2],
) -> Vec<u8> {
    [
        2,
        source,
        source_at[0],
        source_at[1],
        target,
        target_at[0],
        target_at[1],
        1,
        1,
        size[0],
        size[1],
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect()
}

fn runtime(budget: usize) -> Runtime<GraphicsFactory<Reject>> {
    let mut rt = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory { fallback: Reject },
    );
    rt.set_budget(budget, 10000);
    rt.host_mut().system_settings = Some(SystemSettings::default());
    rt.drawing_mut().enabled = true;
    rt.drawing_mut().active = true;
    rt.drawing_mut().target = Some(RenderTarget::new([6, 4]).unwrap());
    rt
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

fn prepare(rt: &mut Runtime<GraphicsFactory<Reject>>, id: u32, bytes: &mut [u8]) {
    rt.dispatch_command_with_buffers(
        0,
        command(id, 281),
        &mut CommandBuffers {
            data0: None,
            data1: Some(bytes),
        },
    )
    .unwrap();
}

#[test]
fn fade_task_routes_real_images_through_canvas_and_copies_final_alpha_on_completion_or_stop() {
    for budget in [1, 4096] {
        let mut rt = runtime(budget);
        let source = image(
            &mut rt,
            [3, 2],
            &[
                0x00112233, 0xff405060, 0x80000000, 0xff112233, 0x00123456, 0xff010203,
            ],
        );
        let target = image(
            &mut rt,
            [3, 2],
            &[
                0xffabcdef, 0x00778899, 0xffffffff, 0xffaabbcc, 0x00000099, 0x80445566,
            ],
        );
        let id = rt.create(0, kind::FADE, b"fade", b"").unwrap();
        prepare(
            &mut rt,
            id,
            &mut context(source, target, [0, 0], [0, 0], [99, 99]),
        );
        assert_eq!(
            rt.images().get(source).unwrap().surface().pixel(0, 0),
            Some(0x00abcdef)
        );
        assert_eq!(
            rt.images().get(target).unwrap().surface().pixel(1, 0),
            Some(0x00405060)
        );
        assert!(rt.update(17, 1).unwrap());
        rt.dispatch_command(0, command(id, 263)).unwrap();
        assert_eq!(rt.dispatch_command(0, command(id, 265)).unwrap(), 1);
        assert!(rt.update(18, 1).unwrap());
        let canvas = rt.drawing().target.as_ref().unwrap();
        assert_eq!(canvas.pixel(1, 1), Some(0xff5d6e7f));
        assert_eq!(canvas.pixel(2, 1), Some(0x00000099));
        assert_eq!(canvas.pixel(1, 2), Some(0x7fabcdef));
        assert_eq!(canvas.pixel(2, 2), Some(0x7f405060));
        assert_eq!(canvas.pixel(0, 0), Some(0));
        assert!(rt.update(19, 1).unwrap());
        assert_eq!(rt.dispatch_command(0, command(id, 265)).unwrap(), 0);
        let canvas = rt.drawing().target.as_ref().unwrap();
        assert_eq!(canvas.pixel(1, 1), Some(0xffaabbcc));
        assert_eq!(canvas.pixel(2, 1), Some(0x00000099));
        assert_eq!(canvas.pixel(1, 2), Some(0xffabcdef));
        assert_eq!(canvas.pixel(2, 2), Some(0x00405060));

        rt.images_mut().get_mut(target).unwrap().pixels_mut()[..4]
            .copy_from_slice(&0xff102030u32.to_le_bytes());
        rt.dispatch_command(0, command(id, 267)).unwrap();
        assert_eq!(
            rt.drawing().target.as_ref().unwrap().pixel(1, 2),
            Some(0xffabcdef)
        );
        rt.dispatch_command(0, command(id, 264)).unwrap();
        assert_eq!(
            rt.drawing().target.as_ref().unwrap().pixel(1, 2),
            Some(0xff102030)
        );
        rt.host_mut().system_settings.as_mut().unwrap().effects[..4]
            .copy_from_slice(&3u32.to_le_bytes());
        rt.dispatch_command(0, command(id, 263)).unwrap();
        assert!(rt.update(20, 1).unwrap());
        assert_eq!(rt.dispatch_command(0, command(id, 265)).unwrap(), 0);
        rt.images_mut().get_mut(target).unwrap().pixels_mut()[..4]
            .copy_from_slice(&0xff304050u32.to_le_bytes());
        rt.dispatch_command(0, command(id, 263)).unwrap();
        rt.dispatch_command(0, command(id, 267)).unwrap();
        assert_eq!(rt.dispatch_command(0, command(id, 265)).unwrap(), 0);
        assert_eq!(
            rt.drawing().target.as_ref().unwrap().pixel(1, 2),
            Some(0xff304050)
        );
    }
}

#[test]
fn aliased_image_wrappers_keep_preparation_feedback_and_bottom_origin_regions() {
    let mut rt = runtime(4096);
    let source = image(
        &mut rt,
        [3, 2],
        &[
            0xffaaaaaa, 0xffbbbbbb, 0xffcccccc, 0x00111111, 0x00222222, 0xff333333,
        ],
    );
    let target = rt.images_mut().share(source).unwrap();
    let id = rt.create(0, kind::FADE, b"fade", b"").unwrap();
    prepare(
        &mut rt,
        id,
        &mut context(source, target, [0, 0], [1, 0], [2, 1]),
    );
    let source = rt.images().get(source).unwrap().surface();
    assert_eq!(source.pixel(0, 1), Some(0x00222222));
    assert_eq!(source.pixel(1, 1), Some(0x00333333));
    assert!(rt.update(10, 1).unwrap());
    rt.dispatch_command(0, command(id, 263)).unwrap();
    assert!(rt.update(11, 1).unwrap());
    let canvas = rt.drawing().target.as_ref().unwrap();
    assert_eq!(canvas.pixel(1, 1), Some(0x002a2a2a));
    assert_eq!(canvas.pixel(2, 1), Some(0x7f333333));
}

struct Empty;
impl Host for Empty {
    fn call(&mut self, _: u32, _: &[u8], _: &mut Interpreter) -> Result<NativeReturn> {
        Err(Error::new(0, "合成宿主不执行 native"))
    }
}

#[test]
fn absent_sources_do_not_require_media_services_and_commands_keep_the_return_slot() {
    let mut fade = FadeObject::new();
    let mut host = Empty;
    let mut output = 0x12345678;
    let mut bytes = [0; 44];
    fade.dispatch_command_with_buffers(
        kind::FADE,
        &command(1, 281),
        &mut CommandBuffers {
            data0: None,
            data1: Some(&mut bytes),
        },
        &mut output,
        &mut host,
        1,
    )
    .unwrap();
    assert!(!fade.is_ready());
    assert_eq!(fade.duration(), 0);
    for code in [263, 264, 267] {
        fade.dispatch_command(kind::FADE, &command(1, code), &mut output, &mut host, 1)
            .unwrap();
        assert_eq!(output, 0x12345678);
        assert!(!fade.is_active());
    }
    fade.dispatch_command(kind::FADE, &command(1, 265), &mut output, &mut host, 1)
        .unwrap();
    assert_eq!(output, 0);
}
