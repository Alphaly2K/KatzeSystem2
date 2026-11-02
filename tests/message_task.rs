use katze_system2::{
    Error, Result,
    expression::State,
    graphics::{
        Drawables, GraphicsFactory, Projection, Rect, RenderTarget, Surface, TextureFilter,
        Transform,
    },
    kcs::{Host, Interpreter, NativeReturn},
    message::{FontRequest, FontServices, GlyphMask, MessageFactory, MessageObject, TextEncoding},
    runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, kind},
    task::{Command, CommandBuffers},
};

struct Fonts;
impl FontServices for Fonts {
    fn validate(&mut self, request: &FontRequest) -> Result<()> {
        if request.name != [b'T' as u16]
            || request.backend != 0
            || request.glyph_parameter != 0
            || request.measure_parameter != 0
        {
            return Err(Error::new(0, "合成字体不支持此请求"));
        }
        Ok(())
    }
    fn measure(&mut self, request: &FontRequest, _: char, _: u32) -> Result<i32> {
        self.validate(request)?;
        Ok(4)
    }
    fn rasterize(&mut self, request: &FontRequest, _: char, _: u32) -> Result<GlyphMask> {
        self.validate(request)?;
        GlyphMask::new([2; 2], [1; 2], 4, vec![64; 4])
    }
}

fn set(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn configuration(encoding: TextEncoding, submit: bool) -> Vec<u8> {
    let unicode = encoding == TextEncoding::Unicode;
    let mut bytes = vec![0; if unicode { 0x12c } else { 0xe8 }];
    let length = bytes.len() as u32;
    set(&mut bytes, 0, length);
    for (at, value) in [
        (8, 24),
        (12, 12),
        (16, 0x0033_2211),
        (0x64, 1),
        (0x74, 10),
        (0x84, b'T' as u32),
    ] {
        set(&mut bytes, at, value);
    }
    for i in 0..5 {
        set(&mut bytes, 0x1c + i * 4, 4);
        set(&mut bytes, 0x30 + i * 4, 2);
    }
    let end = if unicode { 0x120 } else { 0xdc };
    for (at, value) in [(end, 2), (end + 4, 3), (end + 8, u32::from(submit))] {
        set(&mut bytes, at, value);
    }
    bytes
}
fn command(id: u32, code: u32, b: u32) -> Command {
    Command {
        target: id,
        sender: 0,
        code,
        data: [0, b, 0],
    }
}

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂无此类型"))
    }
}

#[test]
fn runtime_routes_both_encodings_through_segment_playback_canvas_and_frame() {
    for (encoding, kind, shift, text) in [
        (
            TextEncoding::Cp932,
            kind::STRING_PLANE,
            0,
            vec![b'A', b'B', 0],
        ),
        (
            TextEncoding::Unicode,
            kind::STRING_PLANE_UTF8,
            21,
            "あB\0".as_bytes().to_vec(),
        ),
    ] {
        let mut rt = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            MessageFactory {
                fallback: GraphicsFactory { fallback: Reject },
            },
        );
        rt.set_fonts(Fonts);
        rt.host_mut().message_time_scale = Some(10);
        rt.drawing_mut().enabled = true;
        rt.drawing_mut().active = true;
        rt.drawing_mut().target = Some(
            RenderTarget::from_tiles(
                [8; 2],
                [4, 3],
                vec![Some(Surface::new([8; 2]).unwrap()); 12],
            )
            .unwrap(),
        );
        let id = rt.create(0, kind, b"text", b"").unwrap();
        let mut config = configuration(encoding, true);
        rt.dispatch_command_with_buffers(
            0,
            command(id, 456 + shift, 0),
            &mut CommandBuffers {
                data1: Some(&mut config),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(rt.drawables().get(id).is_some());
        rt.dispatch_command(0, command(id, 47, 1)).unwrap();
        let mut text = text;
        rt.dispatch_command_with_buffers(
            0,
            command(id, 458 + shift, 0),
            &mut CommandBuffers {
                data1: Some(&mut text),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            rt.dispatch_command(0, command(id, 460 + shift, 0)).unwrap(),
            2
        );
        rt.drawing_mut().mode = 1;
        rt.dispatch_command(0, command(id, 463 + shift, 0)).unwrap();
        assert_eq!(
            rt.dispatch_command(0, command(id, 460 + shift, 0)).unwrap(),
            2
        );
        rt.drawing_mut().mode = 0;
        rt.dispatch_command(0, command(id, 463 + shift, 0)).unwrap();
        rt.update(10, 0).unwrap();
        assert_eq!(
            rt.dispatch_command(0, command(id, 460 + shift, 0)).unwrap(),
            1
        );
        rt.update(11, 0).unwrap();
        assert_eq!(
            rt.drawing().target.as_ref().unwrap().pixel(3, 4),
            Some(0xff11_2233)
        );
        assert_eq!(
            rt.dispatch_command(0, command(id, 460 + shift, 0)).unwrap(),
            0
        );
        rt.update(12, 0).unwrap();
        assert_eq!(
            rt.dispatch_command(0, command(id, 460 + shift, 0)).unwrap(),
            0
        );
        assert_eq!(
            rt.drawing().target.as_ref().unwrap().pixel(7, 4),
            Some(0xff11_2233)
        );
        let mut frame = RenderTarget::new([32, 24]).unwrap();
        rt.render_frame(
            &mut frame,
            &Projection::default(),
            *Transform::default().matrix(),
            TextureFilter::Point,
        )
        .unwrap();
        assert!(
            frame
                .tiles()
                .iter()
                .flatten()
                .any(|t| t.pixels().chunks_exact(4).any(|p| p[3] != 0))
        );
        rt.dispatch_command(0, command(id, 462 + shift, 0)).unwrap();
        let mut tail = b"C\0".to_vec();
        rt.dispatch_command_with_buffers(
            0,
            command(id, 458 + shift, 0),
            &mut CommandBuffers {
                data1: Some(&mut tail),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            rt.dispatch_command(0, command(id, 460 + shift, 0)).unwrap(),
            1
        );
        assert_eq!(
            rt.dispatch_command(0, command(id, 472 + shift, 0)).unwrap(),
            0
        );
        rt.dispatch_command(0, command(id, 463 + shift, 1)).unwrap();
        rt.dispatch_command(0, command(id, 461 + shift, 0)).unwrap();
        assert_eq!(
            rt.dispatch_command(0, command(id, 460 + shift, 0)).unwrap(),
            0
        );
        rt.dispatch_command(0, command(id, 459 + shift, 0)).unwrap();
        assert_eq!(rt.drawing().target.as_ref().unwrap().pixel(3, 4), Some(0));
        rt.destroy(id).unwrap();
        assert!(rt.drawables().get(id).is_none());
        rt.update(13, 0).unwrap();
        assert_eq!(rt.create(0, kind, b"replacement", b"").unwrap(), id);
    }
}

struct TestHost {
    drawings: Drawables,
    fonts: Fonts,
    submissions: Vec<Rect>,
}
impl Host for TestHost {
    fn call(&mut self, _: u32, _: &[u8], _: &mut Interpreter) -> Result<NativeReturn> {
        unreachable!()
    }
    fn fonts(&mut self) -> Option<&mut dyn FontServices> {
        Some(&mut self.fonts)
    }
    fn message_time_scale(&self) -> Option<u32> {
        Some(10)
    }
    fn next_task_child(&self, _: Option<u32>) -> Result<Option<u32>> {
        Ok(None)
    }
    fn drawables(&mut self) -> Option<&mut Drawables> {
        Some(&mut self.drawings)
    }
    fn canvas_layout(&self) -> Result<Option<katze_system2::graphics::CanvasLayout>> {
        Ok(Some(katze_system2::graphics::CanvasLayout::new(
            [32, 24],
            [1; 2],
            [32, 24],
        )?))
    }
    fn submit_message(&mut self, _: &Surface, rect: Rect, _: [i32; 2]) -> Result<bool> {
        self.submissions.push(rect);
        Ok(false)
    }
    fn dispatch_task_command(
        &mut self,
        _: Command,
        _: &mut CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        *output = 0;
        Ok(true)
    }
}
fn send(
    object: &mut MessageObject,
    host: &mut TestHost,
    code: u32,
    b: u32,
    buffer: Option<&mut [u8]>,
) -> u32 {
    let mut output = 0xdead_beef;
    let result = object
        .dispatch_command_with_buffers(
            kind::STRING_PLANE,
            &command(1, code, b),
            &mut CommandBuffers {
                data1: buffer,
                ..Default::default()
            },
            &mut output,
            host,
            4096,
        )
        .unwrap();
    assert!(result.done);
    assert!(!result.propagate);
    output
}

#[test]
fn task_keeps_snapshot_separate_from_completion_and_reconfigures_without_resetting_clock() {
    let mut host = TestHost {
        drawings: Drawables::default(),
        fonts: Fonts,
        submissions: Vec::new(),
    };
    let mut object = MessageObject::new(1, TextEncoding::Cp932);
    let mut config = configuration(TextEncoding::Cp932, true);
    send(&mut object, &mut host, 456, 0, Some(&mut config));
    send(&mut object, &mut host, 47, 1, None);
    let mut text = b"AB\0".to_vec();
    send(&mut object, &mut host, 458, 0, Some(&mut text));
    assert!(!object.complete_snapshot());
    send(&mut object, &mut host, 463, 0, None);
    assert!(!object.complete_snapshot());
    send(&mut object, &mut host, 95, 123, None);
    object.state().1.update(10);
    object.update_body(&mut host, 0, 4096).unwrap();
    assert_eq!(object.playback_time(), 10);
    assert_eq!(send(&mut object, &mut host, 460, 0, None), 1);
    object.state().1.update(11);
    object.update_body(&mut host, 0, 4096).unwrap();
    assert_eq!(send(&mut object, &mut host, 5, 0, None), 123);
    assert!(object.complete_snapshot());
    assert!(!object.active());
    assert_eq!(object.playback_time(), 11);
    send(&mut object, &mut host, 467, 20, None);
    let mut pending = b"C\0".to_vec();
    send(&mut object, &mut host, 458, 0, Some(&mut pending));
    set(&mut config, 0x74, 100);
    set(&mut config, 0xe4, 0);
    let before_reconfigure = host.submissions.len();
    send(&mut object, &mut host, 457, 0, Some(&mut config));
    assert_eq!(host.submissions.len(), before_reconfigure + 1);
    assert_eq!(object.playback_time(), 11);
    assert_eq!(object.display().unwrap().page().source(), "ABC");
    assert_eq!(object.display().unwrap().page().speed(), 20);
    assert_eq!(send(&mut object, &mut host, 460, 0, None), 2);
    let submissions = host.submissions.len();
    send(&mut object, &mut host, 461, 0, None);
    assert_eq!(send(&mut object, &mut host, 460, 0, None), 0);
    assert!(!object.complete_snapshot());
    assert_eq!(host.submissions.len(), submissions);
    assert!(object.display().unwrap().dirty_rectangle().is_some());
    set(&mut config, 0, 0);
    send(&mut object, &mut host, 456, 0, Some(&mut config));
    assert_eq!(object.display().unwrap().page().source(), "");
    assert!(host.drawings.get(1).is_none());
    send(&mut object, &mut host, 459, 0, None);
    assert_eq!(object.playback_time(), 0);
    assert_eq!(object.segment_limit(), 0);
    assert!(!object.complete_snapshot());
    assert_eq!(send(&mut object, &mut host, 460, 0, None), 0);
}
