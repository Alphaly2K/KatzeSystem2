use katze_system2::{
    Error, Result,
    expression::State,
    fes::{ObjectReference, Program},
    message::{FontRequest, FontServices, GlyphMask, MessageDefaults},
    runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, fes::Scene, kind},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂没有该类型"))
    }
}

#[derive(Clone, Default)]
struct Fonts {
    names: Rc<RefCell<Vec<Vec<u8>>>>,
    fail: Rc<Cell<bool>>,
}
impl FontServices for Fonts {
    fn add_resource(&mut self, name: &[u8]) -> Result<bool> {
        self.names.borrow_mut().push(name.to_vec());
        if name == b"two.otf" && self.fail.get() {
            return Err(Error::new(0, "合成字体服务暂时不可用"));
        }
        Ok(false)
    }
    fn validate(&mut self, _: &FontRequest) -> Result<()> {
        Err(Error::new(0, "合成服务只提供字体注册"))
    }
    fn measure(&mut self, _: &FontRequest, _: char, _: u32) -> Result<i32> {
        Err(Error::new(0, "合成服务只提供字体注册"))
    }
    fn rasterize(&mut self, _: &FontRequest, _: char, _: u32) -> Result<GlyphMask> {
        Err(Error::new(0, "合成服务只提供字体注册"))
    }
}

fn runtime(budget: usize) -> Runtime<Reject> {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(8),
        Reject,
    );
    runtime.set_budget(budget, 10000);
    runtime
}

fn update(runtime: &mut Runtime<Reject>, scene: &mut Scene) -> Result<()> {
    for _ in 0..256 {
        if runtime.update_fes_scene(0, scene, 100)? {
            return Ok(());
        }
    }
    panic!("合成场景没有完成本轮更新");
}

#[test]
fn font_add_routes_expanded_names_in_main_and_handlers_and_retries_only_the_failed_call() {
    let source = b"#DEFINE\nBASE obj\n#START\n$0+=1\nfontcom add \"$str0;\" trailing\nfontcom add two.otf\n$1+=1\nwait 100\n#_FES_SLEEP_\nfontcom add sleep.ttf\n#obj.PUSH_L\nfontcom add event.otf\n";
    for budget in [1, 4096] {
        let mut runtime = runtime(budget);
        let fonts = Fonts::default();
        fonts.fail.set(true);
        runtime.set_fonts(fonts.clone());
        runtime
            .numbers_mut()
            .strings
            .insert(0, b"font one.ttf".to_vec());
        let mut scene = Scene::new(
            Program::from_text(source).unwrap(),
            runtime.host().handle_allocator(),
        )
        .unwrap();
        assert!(update(&mut runtime, &mut scene).is_err());
        assert_eq!(runtime.numbers().shared[..2], [1, 0]);
        assert_eq!(
            *fonts.names.borrow(),
            [b"font one.ttf".to_vec(), b"two.otf".to_vec()]
        );
        fonts.fail.set(false);
        update(&mut runtime, &mut scene).unwrap();
        assert_eq!(runtime.numbers().shared[..2], [1, 1]);
        let deadline = scene.deadline();
        let mut done = false;
        for _ in 0..256 {
            if runtime
                .dispatch_fes_system_block(0, &mut scene, "_FES_SLEEP_")
                .unwrap()
            {
                done = true;
                break;
            }
        }
        assert!(done);
        let target = ObjectReference {
            name: "obj".to_owned(),
            bank: 0,
        };
        done = false;
        for _ in 0..256 {
            if let Some(handled) = runtime
                .dispatch_fes_event(0, &mut scene, Some(&target), 2)
                .unwrap()
            {
                assert!(handled);
                done = true;
                break;
            }
        }
        assert!(done);
        assert_eq!(scene.deadline(), deadline);
        assert_eq!(
            *fonts.names.borrow(),
            [
                b"font one.ttf".to_vec(),
                b"two.otf".to_vec(),
                b"two.otf".to_vec(),
                b"sleep.ttf".to_vec(),
                b"event.otf".to_vec()
            ]
        );
    }
}

#[test]
fn font_add_skips_empty_names_and_unknown_operations_but_requires_a_registration_service() {
    let source = b"#START\nfontcom\nfontcom add\nfontcom add \"\"\nfontcom ADD ignored.ttf\nfontcom unknown ignored.ttf\n$0+=1\nfontcom add real.ttf\n$1+=1\nwait 10\n";
    let mut runtime = runtime(4096);
    let mut scene = Scene::new(
        Program::from_text(source).unwrap(),
        runtime.host().handle_allocator(),
    )
    .unwrap();
    assert!(update(&mut runtime, &mut scene).is_err());
    assert_eq!(runtime.numbers().shared[..2], [1, 0]);
    let fonts = Fonts::default();
    runtime.set_fonts(fonts.clone());
    update(&mut runtime, &mut scene).unwrap();
    assert_eq!(*fonts.names.borrow(), [b"real.ttf".to_vec()]);
    assert_eq!(runtime.numbers().shared[..2], [1, 1]);
    let mut scene = Scene::new(
        Program::from_text(b"#START\nfontcom init 0\n").unwrap(),
        runtime.host().handle_allocator(),
    )
    .unwrap();
    assert!(
        update(&mut runtime, &mut scene)
            .unwrap_err()
            .message
            .contains("字体枚举")
    );
}

struct EnumerationFonts(Rc<RefCell<Vec<(u32, u32)>>>);
impl FontServices for EnumerationFonts {
    fn enumerate(&mut self, charset: u32, variable: u32) -> Result<Vec<Vec<u16>>> {
        self.0.borrow_mut().push((charset, variable));
        if charset == 555 {
            return Err(Error::new(0, "合成枚举失败"));
        }
        if charset == 777 {
            return Ok(Vec::new());
        }
        Ok(["Alpha Face", "alpha face", "あい", "Alpha Face"]
            .into_iter()
            .map(|name| name.encode_utf16().collect())
            .collect())
    }
    fn validate(&mut self, _: &FontRequest) -> Result<()> {
        unreachable!()
    }
    fn measure(&mut self, _: &FontRequest, _: char, _: u32) -> Result<i32> {
        unreachable!()
    }
    fn rasterize(&mut self, _: &FontRequest, _: char, _: u32) -> Result<GlyphMask> {
        unreachable!()
    }
}
fn advance(rt: &mut Runtime<Reject>, scene: &mut Scene, now: u32) -> Result<()> {
    for _ in 0..1000 {
        if rt.update_fes_scene(0, scene, now)? {
            return Ok(());
        }
    }
    panic!("合成场景未在预算内完成");
}

#[test]
fn font_commands_keep_scene_lists_raw_names_evaluation_order_and_failed_initialization() {
    for budget in [1, 4096] {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut rt = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(16),
            Reject,
        );
        rt.set_budget(budget, 10000);
        rt.set_fonts(EnumerationFonts(calls.clone()));
        rt.host_mut().message_measurement_parameter = 7;
        rt.host_mut().message_defaults = Some(MessageDefaults {
            font_name: "あい Face".encode_utf16().collect(),
            ..MessageDefaults::default()
        });
        let script = b"#START\nfontcom count ($0+=1)\nfontcom get ($0+=1) ($0+=1)\nfontcom default ($0+=1)\nfontcom search ($0+=1) Alpha Face\nfontcom init\nfontcom count 1\nfontcom get 0 10\nfontcom get 2 11\nfontcom search 2 Alpha Face\nfontcom search 3 ALPHA FACE\nfontcom default 12\nfontcom get ($0+=1) ($0+=1)\nfontcom get ($0+=1)\nfontcom search ($0+=1)\nfontcom unknown ($0+=1)\nwait 1\nfontcom init 777\nfontcom count 1\nfontcom get 0 10\nfontcom default 13\nwait 1\nfontcom init 555\nwait 999\n#_FES_WAKE_\nfontcom count 5\nfontcom get 2 15\n";
        let mut scene = Scene::new(
            Program::from_text(script).unwrap(),
            rt.host().handle_allocator(),
        )
        .unwrap();
        advance(&mut rt, &mut scene, 0).unwrap();
        assert_eq!(rt.numbers().shared[..4], [4, 4, 0, -1]);
        let strings = &rt.numbers().strings;
        assert_eq!(strings[&10], b"Alpha Face");
        assert_eq!(strings[&11], "あい".as_bytes());
        assert_eq!(strings[&12], "あい Face".as_bytes());
        assert_eq!(strings[&2], b"alpha face");
        assert_eq!(scene.font_names().unwrap().len(), 4);
        assert_eq!(
            scene.font_names().unwrap()[0],
            scene.font_names().unwrap()[3]
        );
        while !rt
            .dispatch_fes_system_block(0, &mut scene, "_FES_WAKE_")
            .unwrap()
        {}
        assert_eq!(rt.numbers().shared[5], 4);
        assert_eq!(rt.numbers().strings[&15], "あい".as_bytes());
        let mut other = Scene::new(
            Program::from_text(b"#START\nfontcom count ($0+=1)\nwait 999\n").unwrap(),
            rt.host().handle_allocator(),
        )
        .unwrap();
        advance(&mut rt, &mut other, 0).unwrap();
        assert_eq!(other.font_names(), None);
        assert_eq!(rt.numbers().shared[0], 4);
        advance(&mut rt, &mut scene, 1).unwrap();
        assert_eq!(rt.numbers().shared[1], 0);
        assert!(rt.numbers().strings[&10].is_empty());
        assert_eq!(rt.numbers().strings[&13], "あい Face".as_bytes());
        assert_eq!(scene.font_names(), Some([].as_slice()));
        assert_eq!(
            advance(&mut rt, &mut scene, 2).unwrap_err().message,
            "合成枚举失败"
        );
        assert_eq!(scene.font_names(), None);
        assert_eq!(*calls.borrow(), [(128, 7), (777, 7), (555, 7)]);
    }
}

#[test]
fn configured_font_names_use_string_slots_and_keep_native_code_units() {
    let cases = [
        (
            b"Alpha Face".to_vec(),
            "Alpha Face".encode_utf16().collect::<Vec<_>>(),
            b"Alpha Face".to_vec(),
        ),
        (
            "あい Face".as_bytes().to_vec(),
            "あい Face".encode_utf16().collect(),
            "あい Face".as_bytes().to_vec(),
        ),
        (
            b"A\xff\xffZ".to_vec(),
            vec![65, 65535, 90],
            b"A\xff\xffZ".to_vec(),
        ),
        (
            b"A\xf1\x80\x80\x80Z".to_vec(),
            vec![65, 0xd8c0, 90],
            "A�Z".as_bytes().to_vec(),
        ),
        (b"A\x80Z".to_vec(), vec![65], b"A".to_vec()),
        (b"A\xe3\x41\x80Z".to_vec(), vec![65], b"A".to_vec()),
        (
            b"A\xc2\xa2".to_vec(),
            vec![65, 162],
            "A¢".as_bytes().to_vec(),
        ),
        (b"A\0ignored".to_vec(), vec![65], b"A".to_vec()),
        (vec![b'X'; 63], vec![88; 63], vec![b'X'; 63]),
    ];
    for budget in [1, 4096] {
        for (input, units, returned) in &cases {
            let mut rt = runtime(budget);
            rt.set_fonts(EnumerationFonts(Rc::new(RefCell::new(Vec::new()))));
            rt.host_mut().system_settings = Some(katze_system2::settings::SystemSettings {
                font_names: [Some(vec![80]), Some(vec![82])],
                ..Default::default()
            });
            rt.host_mut().message_defaults = Some(MessageDefaults {
                font_name: vec![80],
                ..Default::default()
            });
            rt.numbers_mut().shared.fill(73);
            rt.numbers_mut().strings.insert(1, input.clone());
            let mut scene = Scene::new(
                Program::from_text(b"#START\nfontcom init\nsetconf 1 font\ngetconf 20 font\ngetconf 21 ruby\nfontcom default 22\nwait 999\n#_FES_WAKE_\nsetconf 99 ruby\ngetconf -1 ruby\n").unwrap(),
                rt.host().handle_allocator(),
            ).unwrap();
            update(&mut rt, &mut scene).unwrap();
            assert_eq!(rt.numbers().strings[&20], *returned);
            assert_eq!(rt.numbers().strings[&21], b"R");
            assert_eq!(rt.numbers().strings[&22], *returned);
            assert_eq!(rt.numbers().shared, [73; 8]);
            assert_eq!(
                rt.host().message_defaults.as_ref().unwrap().font_name,
                *units
            );
            assert_eq!(
                rt.host().system_settings.as_ref().unwrap().font_names,
                [Some(units.clone()), Some(vec![82])]
            );
            assert_eq!(rt.host().system_settings_dirty, 1);
            rt.host_mut().system_settings_dirty = 0;
            while !rt
                .dispatch_fes_system_block(0, &mut scene, "_FES_WAKE_")
                .unwrap()
            {}
            assert_eq!(rt.numbers().strings[&u32::MAX], b"");
            assert_eq!(
                rt.host().system_settings.as_ref().unwrap().font_names,
                [Some(units.clone()), Some(Vec::new())]
            );
            assert_eq!(
                rt.host().message_defaults.as_ref().unwrap().font_name,
                *units
            );
            assert_eq!(rt.host().system_settings_dirty, 1);
        }
    }
}

#[test]
fn invalid_font_name_boundaries_preserve_the_previous_name_and_argument_effects() {
    for (input, error) in [
        (vec![b'X'; 64], "63"),
        (b"A\xc2".to_vec(), "截断"),
        (b"A\xe3\x80".to_vec(), "截断"),
    ] {
        let mut rt = runtime(1);
        rt.host_mut().system_settings = Some(Default::default());
        rt.host_mut().message_defaults = Some(MessageDefaults {
            font_name: vec![80],
            ..Default::default()
        });
        rt.numbers_mut().strings.insert(1, input.clone());
        let mut scene = Scene::new(
            Program::from_text(b"#START\ngetconf 20 font\nsetconf ($0+=1) font ($0+=1) ($0+=1)\n")
                .unwrap(),
            rt.host().handle_allocator(),
        )
        .unwrap();
        assert!(
            update(&mut rt, &mut scene)
                .unwrap_err()
                .message
                .contains(error)
        );
        assert_eq!(rt.numbers().strings[&20], b"P");
        assert_eq!(rt.numbers().strings[&1], input);
        assert_eq!(rt.numbers().shared[0], 3);
        assert_eq!(rt.host().system_settings_dirty, 0);
        assert_eq!(
            rt.host().system_settings.as_ref().unwrap().font_names,
            [None, None]
        );
        assert_eq!(rt.host().message_defaults.as_ref().unwrap().font_name, [80]);
    }
}
