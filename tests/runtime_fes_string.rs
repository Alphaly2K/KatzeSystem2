use katze_system2::{
    Error, Result,
    expression::State,
    fes,
    graphics::{GraphicsFactory, RenderTarget},
    message::{
        FontRequest, FontServices, GlyphMask, MessageDefaults, MessageFactory, TextEncoding,
    },
    runtime::{
        BaseObject, ObjectFactory, Runtime, TaskObject,
        fes::{ObjectState, Scene},
        kind,
    },
    task::Command,
};
use std::{cell::Cell, rc::Rc};

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂没有该类型"))
    }
}
struct Fonts(Rc<Cell<bool>>);
impl FontServices for Fonts {
    fn validate(&mut self, _: &FontRequest) -> Result<()> {
        if self.0.get() {
            Err(Error::new(0, "合成字体暂时不可用"))
        } else {
            Ok(())
        }
    }
    fn measure(&mut self, _: &FontRequest, _: char, _: u32) -> Result<i32> {
        Ok(4)
    }
    fn rasterize(&mut self, _: &FontRequest, _: char, _: u32) -> Result<GlyphMask> {
        GlyphMask::new([2; 2], [1; 2], 4, vec![64; 4])
    }
}
type Engine = Runtime<MessageFactory<GraphicsFactory<Reject>>>;
fn engine(budget: usize, unicode: bool, fail: Rc<Cell<bool>>) -> Engine {
    let mut rt = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(8),
        MessageFactory {
            fallback: GraphicsFactory { fallback: Reject },
        },
    );
    rt.set_budget(budget, 10000);
    rt.host_mut().unicode_text = u32::from(unicode);
    rt.host_mut().message_defaults = Some(MessageDefaults::default());
    rt.host_mut().message_time_scale = Some(10);
    rt.set_fonts(Fonts(fail));
    rt.drawing_mut().enabled = true;
    rt.drawing_mut().active = true;
    rt.drawing_mut().target = Some(RenderTarget::new([32, 24]).unwrap());
    rt
}
fn advance(rt: &mut Engine, scene: &mut Scene, now: u32) -> Result<()> {
    for _ in 0..1000 {
        if rt.update_fes_scene(0, scene, now)? {
            return Ok(());
        }
    }
    panic!("合成 FES 未在预算内结束本轮")
}
fn string(scene: &Scene) -> (&katze_system2::runtime::fes::StringState, u32) {
    let handle = scene.objects().lookup("text", 0).unwrap();
    let object = scene.objects().get(handle).unwrap();
    let ObjectState::String(state) = &object.state else {
        panic!()
    };
    (state, object.task_id().unwrap())
}
fn remaining(rt: &mut Engine, task: u32, encoding: TextEncoding) -> u32 {
    rt.dispatch_command(
        0,
        Command {
            target: task,
            sender: 0,
            code: if encoding == TextEncoding::Unicode {
                481
            } else {
                460
            },
            data: [0; 3],
        },
    )
    .unwrap()
}

#[test]
fn fes_strings_preserve_accumulation_segment_limits_cached_queries_and_canvas_submission() {
    for budget in [1, 4096] {
        for (name, unicode, encoding) in [
            ("STRING", false, TextEncoding::Cp932),
            ("STRING", true, TextEncoding::Unicode),
            ("STRING8", false, TextEncoding::Unicode),
        ] {
            let mut rt = engine(budget, unicode, Rc::new(Cell::new(false)));
            rt.host_mut().message_defaults.as_mut().unwrap().font_name =
                "Global Face".encode_utf16().collect();
            let script = format!(
                "#DEFINE\n{name} text\n#OBJECT VRAM DISP\ntext 2,3 1\n#START\ntext layout view 24 12\ntext layout size 4\ntext layout vdraw ($0+=1)\ntext layout fontind 5\ntext layout fontfix 9\ntext layout color 255 255 255\ntext str A\ntext apendmark\ntext apend B\nwait 1\ntext linecnt 1\ntext endpos 3 4\ntext lineinfo ($7+=1) ($7+=1) ($7+=1)\ntext isdraw\n$2 = \\0\ntext draw 0\nwait 1\ntext skip\nwait 1\ntext draw -1\ntext skip\nwait 1\ntext clear\nwait 1\ntext str \"C D\" tail\nwait 1\ntext str   E\nwait 1000\n"
            );
            let mut scene = Scene::new(
                fes::Program::from_text(script.as_bytes()).unwrap(),
                rt.host().handle_allocator(),
            )
            .unwrap();
            while !rt.prepare_fes_scene(0, &mut scene).unwrap() {}
            advance(&mut rt, &mut scene, 0).unwrap();
            let (state, task) = string(&scene);
            assert_eq!(state.layout.encoding(), encoding);
            let bytes = state.layout.configuration_bytes();
            let backend = if encoding == TextEncoding::Unicode {
                0x118
            } else {
                0xd8
            };
            assert_eq!(
                u32::from_le_bytes(bytes[backend..backend + 4].try_into().unwrap()),
                5
            );
            assert_eq!(
                u16::from_le_bytes(bytes[0x84..0x86].try_into().unwrap()),
                b'G' as u16
            );
            assert_eq!(rt.numbers().shared[0], 0);
            assert_eq!(state.remaining(), 2);
            assert_eq!(state.line_count(), 1);
            assert_eq!(remaining(&mut rt, task, encoding), 2);
            advance(&mut rt, &mut scene, 1).unwrap();
            assert_eq!(rt.numbers().shared[1], 1);
            assert_eq!(rt.numbers().shared[2], 2);
            assert_eq!(rt.numbers().shared[3..5], string(&scene).0.cursor());
            assert_eq!(rt.numbers().shared[7], 3);
            advance(&mut rt, &mut scene, 2).unwrap();
            assert_eq!(remaining(&mut rt, task, encoding), 1);
            advance(&mut rt, &mut scene, 3).unwrap();
            assert_eq!(remaining(&mut rt, task, encoding), 0);
            assert!(rt.drawing().target.as_ref().unwrap().pixel(3, 4).unwrap() != 0);
            advance(&mut rt, &mut scene, 4).unwrap();
            assert_eq!(remaining(&mut rt, task, encoding), 0);
            assert_eq!(string(&scene).0.text(), Some(b"".as_slice()));
            advance(&mut rt, &mut scene, 5).unwrap();
            assert_eq!(string(&scene).0.text(), Some(b"\"C D\" tail".as_slice()));
            assert_eq!(remaining(&mut rt, task, encoding), 10);
            advance(&mut rt, &mut scene, 6).unwrap();
            assert_eq!(string(&scene).0.text(), Some(b"  E".as_slice()));
        }
    }
}

#[test]
fn fes_string_creation_retry_and_reconfiguration_keep_text_and_task_identity() {
    for budget in [1, 4096] {
        let fail = Rc::new(Cell::new(true));
        let mut rt = engine(budget, false, fail.clone());
        let script = b"#DEFINE\nSTRING8 text\nSTRING8 text\n#OBJECT DISP\ntext 1\n#START\ntext layout view 24 12\ntext str AB\nwait 1\ntext layout margin 1 1 1 1\ntext apend C\ntext reset\nwait 1000\n";
        let mut scene = Scene::new(
            fes::Program::from_text(script).unwrap(),
            rt.host().handle_allocator(),
        )
        .unwrap();
        while !rt.prepare_fes_scene(0, &mut scene).unwrap() {}
        let tasks: Vec<_> = scene
            .objects()
            .media_records()
            .iter()
            .map(|r| r.task_id().unwrap())
            .collect();
        assert_ne!(tasks[0], tasks[1]);
        assert_eq!(
            advance(&mut rt, &mut scene, 0).unwrap_err().message,
            "合成字体暂时不可用"
        );
        fail.set(false);
        advance(&mut rt, &mut scene, 0).unwrap();
        assert_eq!(remaining(&mut rt, tasks[1], TextEncoding::Unicode), 2);
        advance(&mut rt, &mut scene, 1).unwrap();
        assert_eq!(string(&scene).1, tasks[1]);
        assert_eq!(remaining(&mut rt, tasks[1], TextEncoding::Unicode), 1);
        assert_eq!(string(&scene).0.text(), Some(b"ABC".as_slice()));
        let handle = scene.objects().lookup("text", 0).unwrap();
        rt.remove_fes_object(0, scene.objects_mut().unwrap(), handle)
            .unwrap();
        assert!(
            tasks
                .iter()
                .all(|&task| rt.object(task).unwrap().control().deletion_requested != 0)
        );
    }
}

#[test]
fn layout_shift_evaluates_once_updates_banks_and_survives_reset() {
    for budget in [1, 4096] {
        for (name, unicode) in [("STRING", false), ("STRING", true), ("STRING8", false)] {
            let mut rt = engine(budget, unicode, Rc::new(Cell::new(false)));
            let script = format!(
                "#DEFINE\n{name} text[2]\n#OBJECT DISP\ntext[0] 1\ntext[1] 1\n#START\ntext layout view 24 12\ntext layout size 4\ntext layout color 255 255 255\ntext layout shift ($0+=3) ($1+=99)\ntext str AB\ntext draw -1\nwait 1\ntext layout shift 7\ntext endpos 2 3\nwait 1\ntext reset\ntext str C\ntext draw -1\nwait 1000\n"
            );
            let mut scene = Scene::new(
                fes::Program::from_text(script.as_bytes()).unwrap(),
                rt.host().handle_allocator(),
            )
            .unwrap();
            while !rt.prepare_fes_scene(0, &mut scene).unwrap() {}
            for (now, y) in [(100, 3), (101, 7), (102, 7)] {
                advance(&mut rt, &mut scene, now).unwrap();
                assert_eq!(rt.numbers().shared[..2], [3, 0]);
                for bank in 0..2 {
                    let handle = scene.objects().lookup("text", bank).unwrap();
                    let ObjectState::String(state) = &scene.objects().get(handle).unwrap().state
                    else {
                        unreachable!()
                    };
                    assert_eq!(state.layout.shift(), y);
                    assert!(!state.layout.shift_dirty());
                    assert_eq!(state.cursor()[1], if now == 101 { 3 } else { y });
                }
                let surface = rt.drawing().target.as_ref().unwrap();
                assert_ne!(surface.pixel(1, (y + 1) as u32), Some(0));
                if now == 101 {
                    assert_eq!(surface.pixel(1, 4), Some(0));
                    assert_eq!(rt.numbers().shared[3], 3);
                }
            }
        }
    }
}

#[test]
fn string_waits_finish_the_first_busy_bank_and_resume_after_cached_counts_refresh() {
    for budget in [1, 4096] {
        for (name, unicode, encoding) in [
            ("STRING", false, TextEncoding::Cp932),
            ("STRING", true, TextEncoding::Unicode),
            ("STRING8", false, TextEncoding::Unicode),
        ] {
            let mut rt = engine(budget, unicode, Rc::new(Cell::new(false)));
            let script = format!(
                "#DEFINE\n{name} text[2]\n#OBJECT DISP\ntext[0] 1\ntext[1] 1\n#START\ntext layout view 24 12\ntext layout size 4\ntext layout frame 1000\ntext layout color 255 255 255\ntext str AB\ntext draw -1\nwait string text[($0+=1)-1]\n$1+=1\nwait string text\n$2+=1\nwait 1000\n"
            );
            let mut scene = Scene::new(
                fes::Program::from_text(script.as_bytes()).unwrap(),
                rt.host().handle_allocator(),
            )
            .unwrap();
            while !rt.prepare_fes_scene(0, &mut scene).unwrap() {}
            advance(&mut rt, &mut scene, 100).unwrap();
            let first = scene.objects().lookup("text", 0).unwrap();
            let second = scene.objects().lookup("text", 1).unwrap();
            let tasks = [first, second]
                .map(|handle| scene.objects().get(handle).unwrap().task_id().unwrap());
            assert_eq!(string(&scene).0.remaining(), 1);
            for (now, click) in [(101, true), (102, false), (103, false)] {
                if click {
                    rt.dispatch_fes_event(0, &mut scene, None, 2).unwrap();
                }
                advance(&mut rt, &mut scene, now).unwrap();
                assert_eq!(rt.numbers().shared[0], 1);
                assert_eq!(rt.numbers().shared[1], i32::from(now == 103));
                assert_eq!(rt.numbers().shared[2], 0);
                assert_eq!(remaining(&mut rt, tasks[0], encoding), 0);
                assert_eq!(remaining(&mut rt, tasks[1], encoding), 1);
                assert_eq!(string(&scene).0.remaining(), i32::from(now == 101));
                assert_eq!(scene.deadline(), Some(u32::MAX - 5));
            }
            assert!((0..24).any(|y| {
                (0..32).any(|x| rt.drawing().target.as_ref().unwrap().pixel(x, y).unwrap() != 0)
            }));
            rt.remove_fes_object(0, scene.objects_mut().unwrap(), first)
                .unwrap();
            rt.dispatch_fes_event(0, &mut scene, None, 2).unwrap();
            for now in 104..=106 {
                advance(&mut rt, &mut scene, now).unwrap();
                assert_eq!(remaining(&mut rt, tasks[1], encoding), 0);
                assert_eq!(rt.numbers().shared[..3], [1, 1, i32::from(now == 106)]);
            }
            assert_eq!(scene.deadline(), Some(1106));
        }
    }
}
