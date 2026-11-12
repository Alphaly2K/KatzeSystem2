use super::*;
use crate::message::{FontRequest, FontServices, GlyphMask, MessageDefaults, MessageFactory};
use crate::runtime::{BaseObject, Runtime, TaskObject, kind};

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂没有该类型"))
    }
}
struct Fonts;
impl FontServices for Fonts {
    fn validate(&mut self, _: &FontRequest) -> Result<()> {
        Ok(())
    }
    fn measure(&mut self, _: &FontRequest, _: char, _: u32) -> Result<i32> {
        Ok(4)
    }
    fn rasterize(&mut self, _: &FontRequest, _: char, _: u32) -> Result<GlyphMask> {
        GlyphMask::new([2, 2], [1, 1], 4, vec![64; 4])
    }
}
struct Images(std::rc::Rc<std::cell::Cell<usize>>);
impl crate::kcs::ResourceServices for Images {
    fn resolve(&mut self, _: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        Ok(name.to_vec())
    }
    fn read(&mut self, _: &[u8], _: crate::resource::ResourceMode) -> Result<Option<Vec<u8>>> {
        self.0.set(self.0.get() + 1);
        let mut bytes = b"HG-3".to_vec();
        bytes.extend(12u32.to_le_bytes());
        bytes.extend(0x300u32.to_le_bytes());
        for (id, next, x, y) in [(10u32, 64u32, 2u32, 3u32), (11, 0, 18, 15)] {
            bytes.extend(next.to_le_bytes());
            bytes.extend(id.to_le_bytes());
            bytes.extend(b"stdinfo\0");
            for value in [0u32, 40, 12, 6, 32, x, y, 32, 32, 0, 0, 0] {
                bytes.extend(value.to_le_bytes());
            }
        }
        Ok(Some(bytes))
    }
}
type Engine = Runtime<MessageFactory<crate::graphics::GraphicsFactory<Reject>>>;
fn advance(runtime: &mut Engine, scene: &mut Scene, now: u32) {
    for _ in 0..10000 {
        if runtime.update_fes_scene(0, scene, now).unwrap() {
            return;
        }
    }
    panic!("合成日志场景未完成当前更新");
}
fn state(scene: &Scene) -> &LogStringState {
    let handle = scene.objects().lookup("log", 0).unwrap();
    let ObjectState::LogString(state) = &scene.objects().get(handle).unwrap().state else {
        panic!()
    };
    state
}
fn texts(runtime: &Engine, scene: &Scene) -> Vec<String> {
    state(scene)
        .task_ids()
        .map(|task| {
            runtime
                .object(task)
                .unwrap()
                .message_page()
                .unwrap()
                .source()
                .to_owned()
        })
        .collect()
}

#[test]
fn logs_render_retained_rows_scroll_query_sources_and_release_all_tasks() {
    for budget in [1, 4096] {
        for (kind, unicode) in [
            ("LOGSTRING", false),
            ("LOGSTRING", true),
            ("LOGSTRING8", false),
        ] {
            let mut runtime = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                crate::expression::State::new(12),
                MessageFactory {
                    fallback: crate::graphics::GraphicsFactory { fallback: Reject },
                },
            );
            runtime.set_budget(budget, 10000);
            runtime.host_mut().unicode_text = u32::from(unicode);
            runtime.host_mut().message_defaults = Some(MessageDefaults::default());
            runtime.host_mut().message_time_scale = Some(10);
            runtime.set_fonts(Fonts);
            let reads = std::rc::Rc::new(std::cell::Cell::new(0));
            runtime.set_resources(Images(reads.clone()));
            runtime.drawing_mut().target =
                Some(crate::graphics::RenderTarget::new([32, 32]).unwrap());
            runtime.drawing_mut().enabled = true;
            runtime.drawing_mut().active = true;
            let handle = runtime.host().handle_allocator().allocate().unwrap();
            let mut log = Backlog::new(crate::backlog::Settings {
                columns: 100,
                ..Default::default()
            });
            for (i, text) in [b"A", b"B", b"C", b"D", b"E", b"F"].into_iter().enumerate() {
                log.push(crate::backlog::Record {
                    file: b"scene",
                    input: i as u32 + 10,
                    name: b"",
                    text,
                    flag: u32::from(i % 2 == 0),
                    space: 0,
                    extra: i as u32 + 100,
                })
                .unwrap();
            }
            log.set_limit(4);
            runtime.resources.backlogs.insert(handle, log);
            let script = format!(
                "#DEFINE\n{kind} log\n#OBJECT COUNT SIZE2 VRAM POS2 PRI DISP\nlog 2 atlas:10 1,2 atlas:10 1 1\n#START\nlog setlog {handle}\nlog layout size 4\nlog layout margin 0 0 0 0\nlog layout frame 0\nlog layout color 255 255 255\nlog layout vcolor 255 255 255\nlog max 0\nlog getpos 1\nlog blockinfo 0 2 3 4 5\nlog lineinfo 0 7 6\nlog pathinfo 0 7 8\nlog draw\nwait 1\nlog page -1\nwait 1\nlog next 1\nwait 1\nlog setlog 0\nwait 1\nlog setlog {handle}\nlog disp 0\nwait 1\nlog disp 1\nwait 1000\n"
            );
            let mut scene = Scene::new(
                fes::Program::from_text(script.as_bytes()).unwrap(),
                runtime.host().handle_allocator(),
            )
            .unwrap();
            advance(&mut runtime, &mut scene, 0);
            assert_eq!(
                &runtime.numbers().shared[..9],
                &[4, 2, 0, 0, 0, 4, 12, 102, 12]
            );
            assert_eq!(runtime.numbers().strings.get(&7).unwrap(), b"scene");
            assert_eq!(texts(&runtime, &scene), ["E", "F"]);
            assert_eq!(
                state(&scene).rows[0].text.layout.base.vram()[..2],
                [1.0, 2.0]
            );
            assert_eq!(
                state(&scene).rows[1].text.layout.base.vram()[..2],
                [1.0, 10.0]
            );
            assert_eq!(state(&scene).positions, [[2.0, 3.0], [18.0, 15.0]]);
            let initial_reads = reads.get();
            let tasks = state(&scene).task_ids().collect::<Vec<_>>();
            advance(&mut runtime, &mut scene, 1);
            assert_eq!(state(&scene).position(), 0);
            assert_eq!(texts(&runtime, &scene), ["C", "D"]);
            advance(&mut runtime, &mut scene, 2);
            assert_eq!(state(&scene).position(), 1);
            assert_eq!(texts(&runtime, &scene), ["D", "E"]);
            advance(&mut runtime, &mut scene, 3);
            assert_eq!(state(&scene).position(), 1);
            advance(&mut runtime, &mut scene, 4);
            assert_eq!(state(&scene).position(), 2);
            assert!(!state(&scene).drawing);
            advance(&mut runtime, &mut scene, 5);
            assert_eq!(texts(&runtime, &scene), ["E", "F"]);
            assert_eq!(state(&scene).task_ids().collect::<Vec<_>>(), tasks);
            runtime
                .store()
                .set_fes_media_sleep(0, &mut scene, true)
                .unwrap();
            runtime
                .store()
                .set_fes_media_sleep(0, &mut scene, false)
                .unwrap();
            assert_eq!(reads.get(), initial_reads);
            let handle = scene.objects().lookup("log", 0).unwrap();
            runtime
                .remove_fes_object(0, scene.objects_mut().unwrap(), handle)
                .unwrap();
            runtime.update(6, 1).unwrap();
            assert!(tasks.into_iter().all(|task| runtime.object(task).is_none()));
        }
    }
}
struct Scenes;
impl crate::kcs::ResourceServices for Scenes {
    fn resolve(&mut self, _: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        Ok(name.to_vec())
    }
    fn read(&mut self, name: &[u8], _: crate::resource::ResourceMode) -> Result<Option<Vec<u8>>> {
        if name.ends_with(b".cstl") {
            let mut bytes = b"CSTL\0\0\0\0\x02".to_vec();
            let string = |bytes: &mut Vec<u8>, value: &str| {
                bytes.push(value.len() as u8);
                bytes.extend(value.as_bytes());
            };
            string(&mut bytes, "main");
            string(&mut bytes, "sub");
            bytes.push(2);
            for (name, body) in [
                ("主名", "主一\x1f主二"),
                ("副名", "副一\x1f副二"),
                ("", "next"),
                ("", "next"),
            ] {
                string(&mut bytes, name);
                string(&mut bytes, body);
            }
            return Ok(Some(bytes));
        }
        let mut text = Vec::new();
        let mut offsets = Vec::new();
        for (kind, value) in [
            (0x21, "base"),
            (0x20, "one"),
            (0x20, "two"),
            (2, ""),
            (0x20, "next"),
            (2, ""),
        ] {
            offsets.push(text.len() as u32);
            text.extend([1, kind]);
            text.extend(value.as_bytes());
            text.push(0);
        }
        let strings = 16 + offsets.len() as u32 * 4;
        let mut body = [strings + text.len() as u32, 2, 16, strings, 4, 0, 2, 4]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        body.extend(offsets.into_iter().flat_map(u32::to_le_bytes));
        body.extend(text);
        let mut bytes = b"CatScene".to_vec();
        bytes.extend(0u32.to_le_bytes());
        bytes.extend((body.len() as u32).to_le_bytes());
        bytes.extend(body);
        Ok(Some(bytes))
    }
}

#[test]
fn remake_selects_languages_preserves_geometry_and_hides_disabled_subtitles() {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        crate::expression::State::new(0),
        MessageFactory {
            fallback: crate::graphics::GraphicsFactory { fallback: Reject },
        },
    );
    runtime.set_fonts(Fonts);
    runtime.set_resources(Scenes);
    runtime.host_mut().message_defaults = Some(MessageDefaults::default());
    runtime.host_mut().message_time_scale = Some(10);
    runtime.host_mut().languages.prefixes = vec![b"main".to_vec(), b"sub".to_vec()];
    runtime.drawing_mut().target = Some(crate::graphics::RenderTarget::new([32, 32]).unwrap());
    let log_handle = runtime.host().handle_allocator().allocate().unwrap();
    let mut log = Backlog::new(crate::backlog::Settings {
        name_line: 0,
        display_name: 0,
        ..Default::default()
    });
    log.push(crate::backlog::Record {
        file: b"scene",
        input: 0,
        name: b"old",
        text: b"old",
        flag: 1,
        space: 0,
        extra: 9,
    })
    .unwrap();
    runtime.resources.backlogs.insert(log_handle, log);
    let script = format!(
        "#DEFINE\nLOGSTRING8 log\n#OBJECT COUNT SIZE VRAM DISP\nlog 1 12,6 0,0 1\n#START\nlog setlog {log_handle}\nlog layout size 4\nlog layout margin 0 0 0 0\nlog remake 0 1\nlog draw\nwait 1\nlog remake 0 -1\nwait 1000\n"
    );
    let mut scene = Scene::new(
        fes::Program::from_text(script.as_bytes()).unwrap(),
        runtime.host().handle_allocator(),
    )
    .unwrap();
    advance(&mut runtime, &mut scene, 0);
    assert_eq!(texts(&runtime, &scene), ["主一主二", "副一副二"]);
    let log = runtime.backlog(log_handle).unwrap();
    assert_eq!(log.name_for_row(0), Some("主名".as_bytes()));
    assert_eq!(log.secondary_row(0), Some("副一副二".as_bytes()));
    assert_eq!(state(&scene).template.layout.base.base.size, [12, 6]);
    let (task, text) = state(&scene).rows[0].secondary.as_ref().unwrap();
    assert_eq!(
        runtime
            .object(*task)
            .unwrap()
            .message_page()
            .unwrap()
            .source(),
        "副一副二"
    );
    assert_eq!(text.layout.base.vram()[..2], [12.0, 0.0]);
    assert!(state(&scene).task_display(*task));
    let secondary = *task;
    advance(&mut runtime, &mut scene, 1);
    assert!(!state(&scene).task_display(secondary));
    runtime
        .store()
        .set_fes_media_sleep(0, &mut scene, true)
        .unwrap();
    runtime
        .store()
        .set_fes_media_sleep(0, &mut scene, false)
        .unwrap();
    assert!(!state(&scene).task_display(secondary));
}
