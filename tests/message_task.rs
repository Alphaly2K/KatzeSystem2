use katze_system2::{
    Error, Result,
    expression::State,
    graphics::{
        Drawables, GraphicsFactory, Projection, Rect, RenderTarget, Surface, TextureFilter,
        Transform,
    },
    kcs::{Host, Interpreter, NativeReturn, Program},
    message::{
        FontRequest, FontServices, GlyphMask, MessageDefaults, MessageFactory, MessageObject,
        TextEncoding,
    },
    runtime::{BaseObject, KcsFactory, ObjectFactory, ProgramLoader, Runtime, TaskObject, kind},
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
fn defaults_write_only_tagged_configuration_and_require_explicit_global_settings() {
    let mut rt = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        MessageFactory { fallback: Reject },
    );
    for (encoding, kind, code, length, capacity) in [
        (TextEncoding::Cp932, kind::STRING_PLANE, 455, 0xe8, 32),
        (
            TextEncoding::Unicode,
            kind::STRING_PLANE_UTF8,
            476,
            0x12c,
            64,
        ),
    ] {
        let id = rt.create(0, kind, b"defaults", b"").unwrap();
        rt.host_mut().message_defaults = None;
        rt.dispatch_command(0, command(id, code, 0)).unwrap();
        let mut config = vec![0xa5; length + 7];
        let original = config.clone();
        let query = |rt: &mut Runtime<_>, config: &mut [u8]| {
            rt.dispatch_command_with_buffers(
                0,
                command(id, code, 0),
                &mut CommandBuffers {
                    data1: Some(config),
                    ..Default::default()
                },
            )
        };
        query(&mut rt, &mut config).unwrap();
        assert_eq!(config, original);
        set(&mut config, 0, length as u32);
        let tagged = config.clone();
        assert!(
            query(&mut rt, &mut config)
                .unwrap_err()
                .message
                .contains("消息默认设置")
        );
        assert_eq!(config, tagged);
        let mut name = vec![b'T' as u16; capacity - 1];
        name.extend([0, b'X' as u16]);
        rt.host_mut().message_defaults = Some(MessageDefaults {
            font_name: name,
            bold: true,
            reveal: true,
            shadow: u32::MAX,
            outline: 0x12345678,
            glyph_parameter: Some(0),
        });
        assert!(query(&mut rt, &mut config[..4]).is_err());
        assert_eq!(config, tagged);
        query(&mut rt, &mut config).unwrap();
        let words = |offset: usize, count: usize| {
            config[offset..offset + count * 4]
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .collect::<Vec<_>>()
        };
        assert_eq!(words(0, 4), [length as u32, 1, 800, 600]);
        assert_eq!(words(0x1c, 5), [24; 5]);
        assert_eq!(words(0x30, 5), [10; 5]);
        assert_eq!(
            words(0x84 + capacity * 2, 5),
            [700, u32::MAX, 0x12345678, 0, 60]
        );
        let expected = rt
            .host()
            .message_defaults
            .as_ref()
            .unwrap()
            .configuration(encoding)
            .unwrap();
        assert_eq!(&config[..length], expected);
        assert_eq!(&config[length..], &[0xa5; 7]);
        assert!(rt.drawables().get(id).is_none());
        rt.host_mut().message_defaults.as_mut().unwrap().font_name = vec![b'T' as u16; capacity];
        let original = config.clone();
        assert!(
            query(&mut rt, &mut config)
                .unwrap_err()
                .message
                .contains("容量")
        );
        assert_eq!(config, original);
    }
}

#[test]
fn ordinary_message_queries_reject_missing_deleted_and_unconfigured_targets() {
    struct Query {
        native: u32,
        target: u32,
        extra: bool,
    }
    impl ProgramLoader for Query {
        fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
            let mut code = vec![];
            for value in std::iter::once(self.target).chain(self.extra.then_some(0)) {
                code.extend([0x25, 3]);
                code.extend(value.to_le_bytes());
            }
            code.extend([0x2b, 3]);
            code.extend(self.native.to_le_bytes());
            code.push(3);
            code.extend((if self.extra { 8u32 } else { 4 }).to_le_bytes());
            code.push(0x3b);
            Ok(Some(Program::from_code(&code, vec![], 64)?))
        }
    }
    for native in [616, 884] {
        for (target, object_kind, delete, extra, diagnostic) in [
            (0, kind::STRING_PLANE, false, false, "目标"),
            (99, kind::STRING_PLANE, false, false, "目标"),
            (1, kind::STRING_PLANE_UTF8, false, false, "目标"),
            (1, kind::STRING_PLANE, true, false, "目标"),
            (1, kind::STRING_PLANE, false, false, "显示对象"),
            (1, kind::STRING_PLANE, false, true, "参数字节数"),
        ] {
            let mut rt = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                State::new(0),
                MessageFactory {
                    fallback: KcsFactory::new(Query {
                        native,
                        target,
                        extra,
                    }),
                },
            );
            let text = rt.create(0, object_kind, b"text", b"").unwrap();
            if delete {
                rt.dispatch_command(0, command(text, 3, 0)).unwrap();
            }
            let caller = rt.create(0, kind::ORDINARY, b"query", b"").unwrap();
            let error = rt.update(0, 0).unwrap_err();
            assert!(error.message.contains(diagnostic), "{}", error.message);
            assert!(
                rt.object(caller)
                    .unwrap()
                    .interpreter()
                    .unwrap()
                    .stack_bytes()
                    .is_empty()
            );
        }
    }
}

struct MessageProgram {
    address: u32,
}
impl ProgramLoader for MessageProgram {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        fn ins(code: &mut Vec<u8>, opcode: u8, words: &[u32]) {
            code.push(opcode);
            for word in words {
                code.push(3);
                code.extend(word.to_le_bytes());
            }
        }
        fn call(code: &mut Vec<u8>, native: u32, words: &[u32]) {
            for word in words {
                ins(code, 0x25, &[*word]);
            }
            ins(code, 0x2b, &[native, words.len() as u32 * 4]);
        }
        fn wait(code: &mut Vec<u8>) {
            call(code, 71, &[48]);
            ins(code, 0x26, &[8]);
        }
        let mut code = vec![];
        ins(&mut code, 0x25, &[0xdeadbeef]);
        call(&mut code, 611, &[1, self.address]);
        wait(&mut code);
        call(&mut code, 612, &[1, self.address]);
        wait(&mut code);
        call(&mut code, 613, &[1, self.address]);
        call(&mut code, 614, &[1, 320]);
        call(&mut code, 616, &[1]);
        ins(&mut code, 0x26, &[0]);
        call(&mut code, 619, &[1, 0]);
        call(&mut code, 884, &[1]);
        ins(&mut code, 0x26, &[4]);
        call(&mut code, 616, &[1]);
        ins(&mut code, 0x26, &[12]);
        wait(&mut code);
        call(&mut code, 618, &[1]);
        call(&mut code, 614, &[1, 336]);
        call(&mut code, 884, &[1]);
        ins(&mut code, 0x26, &[20]);
        call(&mut code, 616, &[1]);
        ins(&mut code, 0x26, &[24]);
        call(&mut code, 695, &[1, 0]);
        call(&mut code, 619, &[1, 1]);
        call(&mut code, 617, &[1]);
        call(&mut code, 884, &[1]);
        ins(&mut code, 0x26, &[28]);
        wait(&mut code);
        call(&mut code, 615, &[1]);
        call(&mut code, 616, &[1]);
        ins(&mut code, 0x26, &[36]);
        ins(&mut code, 0x26, &[40]);
        ins(&mut code, 0x3b, &[]);
        let mut data = vec![0xa5; 384];
        set(&mut data, 16, b'B' as u32);
        data[320..331].copy_from_slice(b"A%c[G16]\0\0\0");
        data[336..338].copy_from_slice(b"C\0");
        if self.address & 0x80000000 == 0 {
            set(&mut data, self.address as usize, 0xe8);
        }
        Ok(Some(Program::from_code(&code, data, 256)?))
    }
}

#[test]
fn ordinary_text_calls_write_vm_configuration_and_render_segmented_messages() {
    for budget in [1, 4096] {
        for address in [64, 0x80000040] {
            let mut factory = KcsFactory::new(MessageProgram { address });
            factory.external = katze_system2::memory::SharedMemory::new(vec![0; 384]).unwrap();
            let mut rt = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                State::new(0),
                MessageFactory {
                    fallback: GraphicsFactory { fallback: factory },
                },
            );
            rt.set_budget(budget, 1 << 20);
            rt.set_fonts(Fonts);
            rt.host_mut().message_time_scale = Some(10);
            rt.host_mut().message_defaults = Some(MessageDefaults {
                font_name: vec![b'T' as u16],
                glyph_parameter: Some(0),
                ..Default::default()
            });
            rt.drawing_mut().enabled = true;
            rt.drawing_mut().active = true;
            rt.drawing_mut().target = Some(RenderTarget::new([32, 24]).unwrap());
            let text = rt.create(0, kind::STRING_PLANE, b"text", b"").unwrap();
            assert_eq!(text, 1);
            let caller = rt.create(0, kind::ORDINARY, b"caller", b"").unwrap();
            if address & 0x80000000 != 0 {
                rt.object_mut(caller)
                    .unwrap()
                    .interpreter_mut()
                    .unwrap()
                    .write(address - 4, &[0xa5; 240])
                    .unwrap();
                rt.object_mut(caller)
                    .unwrap()
                    .interpreter_mut()
                    .unwrap()
                    .write(address, &0xe8u32.to_le_bytes())
                    .unwrap();
            }
            rt.update(0, 0).unwrap();
            let vm = rt.object(caller).unwrap().interpreter().unwrap();
            assert_eq!(
                vm.read(address, 0xe8).unwrap(),
                rt.host()
                    .message_defaults
                    .as_ref()
                    .unwrap()
                    .configuration(TextEncoding::Cp932)
                    .unwrap()
            );
            assert_eq!(vm.read(address - 4, 4).unwrap(), [0xa5; 4]);
            assert_eq!(vm.read(address + 0xe8, 4).unwrap(), [0xa5; 4]);
            let config = configuration(TextEncoding::Cp932, true);
            rt.object_mut(caller)
                .unwrap()
                .interpreter_mut()
                .unwrap()
                .write(address, &config)
                .unwrap();
            rt.send_message(0, caller, [0; 4]).unwrap();
            rt.dispatch_command(0, command(text, 47, 1)).unwrap();
            rt.send_message(0, caller, [0; 4]).unwrap();
            let vm = rt.object(caller).unwrap().interpreter().unwrap();
            for (at, value) in [(0, 2u32), (4, 1), (12, 1)] {
                assert_eq!(vm.read(at, 4).unwrap(), value.to_le_bytes());
            }
            assert_eq!(
                rt.drawing().target.as_ref().unwrap().pixel(3, 4),
                Some(0xff112233)
            );
            assert_eq!(rt.drawing().target.as_ref().unwrap().pixel(7, 4), Some(0));
            rt.dispatch_command(0, command(caller, 163, 0)).unwrap();
            rt.update(10, 0).unwrap();
            assert_eq!(rt.dispatch_command(0, command(text, 460, 0)).unwrap(), 1);
            rt.update(11, 0).unwrap();
            assert_eq!(rt.dispatch_command(0, command(text, 460, 0)).unwrap(), 0);
            assert_eq!(
                rt.drawing().target.as_ref().unwrap().pixel(7, 4),
                Some(0xff112233)
            );
            rt.send_message(0, caller, [0; 4]).unwrap();
            let vm = rt.object(caller).unwrap().interpreter().unwrap();
            for (at, value) in [(20, 0u32), (24, 1), (28, 0)] {
                assert_eq!(vm.read(at, 4).unwrap(), value.to_le_bytes());
            }
            assert_eq!(
                rt.drawing().target.as_ref().unwrap().pixel(11, 4),
                Some(0xff112233)
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
            rt.send_message(0, caller, [0; 4]).unwrap();
            let vm = rt.object(caller).unwrap().interpreter().unwrap();
            assert_eq!(vm.read(36, 4).unwrap(), [0; 4]);
            assert_eq!(vm.read(40, 4).unwrap(), 0xdeadbeefu32.to_le_bytes());
            assert!(vm.stack_bytes().is_empty());
            assert_eq!(rt.drawing().target.as_ref().unwrap().pixel(3, 4), Some(0));
        }
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
