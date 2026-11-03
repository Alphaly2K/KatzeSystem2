use katze_system2::{
    Error, Result,
    expression::State,
    fes,
    graphics::{GraphicsFactory, Rect, RenderTarget},
    kcs::ResourceServices,
    resource::ResourceMode,
    runtime::{
        BaseObject, ObjectFactory, Runtime, TaskObject,
        fes::{ObjectState, Scene},
        kind,
    },
    task::{Command, CommandBuffers},
};
use std::{cell::Cell, io::Write, rc::Rc};

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂没有该类型"))
    }
}

struct Factory {
    fail: Rc<Cell<bool>>,
    attempts: Rc<Cell<u32>>,
}
impl ObjectFactory for Factory {
    fn create(&mut self, id: u32, object_kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        if object_kind == kind::FADE {
            assert_eq!(name, b"FE Draw");
            self.attempts.set(self.attempts.get() + 1);
            if self.fail.get() {
                return Err(Error::new(0, "合成过渡创建失败"));
            }
        }
        GraphicsFactory { fallback: Reject }.create(id, object_kind, name)
    }
}

struct Files {
    motion: Option<Vec<u8>>,
    fail: Rc<Cell<bool>>,
    reads: Rc<Cell<u32>>,
    resolves: Rc<Cell<u32>>,
}
impl ResourceServices for Files {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!((category, name), (b"mot".as_slice(), b"d".as_slice()));
        self.resolves.set(self.resolves.get() + 1);
        Ok(b"draw.kcs".to_vec())
    }
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(name, b"draw.kcs");
        assert_eq!(mode, ResourceMode::LooseFirst);
        self.reads.set(self.reads.get() + 1);
        if self.fail.get() {
            return Err(Error::new(0, "合成运动读取失败"));
        }
        Ok(self.motion.clone())
    }
}

fn motion() -> Vec<u8> {
    let code = [0x3b];
    let mut bytes = b"KCS\0".to_vec();
    for word in [2u32, 0, 52, 1, 53, 0, 0, 0, 64, 53, 0, 1] {
        bytes.extend(word.to_le_bytes());
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&code).unwrap();
    bytes.extend(encoder.finish().unwrap());
    bytes
}

struct Harness {
    runtime: Runtime<Factory>,
    scene: Scene,
    source: u32,
    fail_create: Rc<Cell<bool>>,
    fail_read: Rc<Cell<bool>>,
    attempts: Rc<Cell<u32>>,
    reads: Rc<Cell<u32>>,
    resolves: Rc<Cell<u32>>,
}

impl Harness {
    fn new(script: &[u8], budget: usize, moving: bool) -> Self {
        let fail_create = Rc::new(Cell::new(false));
        let fail_read = Rc::new(Cell::new(false));
        let attempts = Rc::new(Cell::new(0));
        let reads = Rc::new(Cell::new(0));
        let resolves = Rc::new(Cell::new(0));
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(4),
            Factory {
                fail: fail_create.clone(),
                attempts: attempts.clone(),
            },
        );
        runtime.set_budget(budget, 10000);
        runtime.set_resources(Files {
            motion: moving.then(motion),
            fail: fail_read.clone(),
            reads: reads.clone(),
            resolves: resolves.clone(),
        });
        runtime.drawing_mut().enabled = true;
        runtime.drawing_mut().active = true;
        runtime.drawing_mut().target = Some(RenderTarget::new([10, 10]).unwrap());
        let list = runtime.create(0, kind::IMAGE_LIST, b"list", b"").unwrap();
        for (code, data) in [(285, [4, 4, 0]), (289, [2, 3, 0])] {
            runtime
                .dispatch_command(
                    0,
                    Command {
                        target: list,
                        sender: 0,
                        code,
                        data,
                    },
                )
                .unwrap();
        }
        let mut source = [0; 4];
        runtime
            .dispatch_command_with_buffers(
                0,
                Command {
                    target: list,
                    sender: 0,
                    code: 291,
                    data: [0; 3],
                },
                &mut CommandBuffers {
                    data0: None,
                    data1: Some(&mut source),
                },
            )
            .unwrap();
        runtime.update(0, 0).unwrap();
        let source = u32::from_le_bytes(source);
        let image = runtime.images_mut().get_mut(source).unwrap();
        for (i, pixel) in image.pixels_mut().chunks_exact_mut(4).enumerate() {
            pixel.copy_from_slice(
                &(0xff000000 | ((i as u32 / 4) << 8) | (i as u32 % 4)).to_le_bytes(),
            );
        }
        let mut scene = Scene::new(
            fes::Program::from_text(script).unwrap(),
            runtime.host().handle_allocator(),
        )
        .unwrap();
        scene
            .objects_mut()
            .unwrap()
            .set_image_list(list, Rect::from_xywh(0, 0, 4, 4));
        while !runtime.prepare_fes_scene(0, &mut scene).unwrap() {}
        Self {
            runtime,
            scene,
            source,
            fail_create,
            fail_read,
            attempts,
            reads,
            resolves,
        }
    }

    fn advance(&mut self, now: u32) -> Result<()> {
        for _ in 0..512 {
            if self.runtime.update_fes_scene(0, &mut self.scene, now)? {
                return Ok(());
            }
        }
        panic!("合成过渡场景未完成预算续跑");
    }

    fn round(&mut self, now: u32) {
        self.runtime.update(now, 1).unwrap();
        self.advance(now).unwrap();
    }

    fn draw(&self) -> u32 {
        self.scene.objects().lookup("__drawobj__", 0).unwrap()
    }

    fn phase(&self) -> Option<i32> {
        let ObjectState::Draw(state) = &self.scene.objects().get(self.draw()).unwrap().state else {
            panic!()
        };
        state.phase()
    }

    fn canvas(&self, x: u32, y: u32) -> u32 {
        self.runtime
            .drawing()
            .target
            .as_ref()
            .unwrap()
            .pixel(x, y)
            .unwrap()
    }
}

#[test]
fn draw_preserves_snapshot_stages_zero_clock_and_restores_submission_after_both_completion_paths() {
    let script = b"#DEFINE\nPLANE panel[2]\nBASE other\n#OBJECT SIZE VRAM DISP\npanel 2,2 1,1 0\n#START\npanel draw ($0+=100)\nother draw ($0+=100)\npanel[1] draw ($0+=4)\npanel[0] draw ($0+=100)\nwait 10\npanel[0] draw 2\nwait 1000\n";
    for budget in [1, 4096] {
        for moving in [false, true] {
            let mut h = Harness::new(script, budget, moving);
            h.round(0);
            assert_eq!(h.runtime.numbers().shared[0], 4);
            assert_eq!(h.phase(), Some(-1));
            let draw = h.draw();
            let object = h.scene.objects().get(draw).unwrap();
            assert_eq!(object.parent(), h.scene.objects().lookup("panel", 1));
            let snapshot = object.image_handle().unwrap();
            let task = object.task_id().unwrap();
            let image = h.runtime.images().get(snapshot).unwrap();
            assert_eq!(image.info().size, [2, 2]);
            assert_eq!(image.surface().pixel(0, 0), Some(0xff000101));
            assert_eq!(image.surface().pixel(1, 1), Some(0xff000202));
            for pixel in h
                .runtime
                .images_mut()
                .get_mut(h.source)
                .unwrap()
                .pixels_mut()
                .chunks_exact_mut(4)
            {
                pixel.copy_from_slice(&0xff8899aau32.to_le_bytes());
            }
            h.round(0);
            assert_eq!(h.phase(), Some(0));
            h.round(0);
            h.round(0);
            assert_eq!(h.phase(), Some(0));
            assert_eq!(
                h.runtime
                    .dispatch_command(
                        0,
                        Command {
                            target: task,
                            sender: 0,
                            code: 265,
                            data: [0; 3]
                        }
                    )
                    .unwrap(),
                1
            );
            h.round(1);
            assert_eq!(h.phase(), Some(1));
            h.round(2);
            assert_eq!(h.canvas(3, 4) == 0xff8899aa, !moving);
            for now in 3..=8 {
                h.round(now);
            }
            assert!(h.scene.objects().lookup("__drawobj__", 0).is_none());
            assert!(h.scene.interpreter().named["__drawobj__"].is_empty());
            assert!(h.runtime.images().get(snapshot).is_err());
            assert!(h.runtime.tree().node(task).is_none());
            assert_eq!(h.canvas(2, 3), 0xff8899aa);
            assert_eq!(h.canvas(3, 4), 0xff8899aa);
            h.round(10);
            assert_ne!(h.draw(), draw);
            assert_eq!(
                h.scene
                    .objects()
                    .media_records()
                    .iter()
                    .filter(|record| record.kind() == "DRAW")
                    .count(),
                1
            );
            assert_eq!(h.attempts.get(), 2);
            assert_eq!(h.reads.get(), if moving { 1 } else { 2 });
        }
    }
}

#[test]
fn draw_retries_resource_and_task_failures_without_repeating_duration_or_creating_another_object() {
    let script =
        b"#DEFINE\nPLANE panel\n#OBJECT SIZE\npanel 2,2\n#START\npanel draw ($0+=3)\nwait 1000\n";
    for budget in [1, 4096] {
        let mut h = Harness::new(script, budget, false);
        h.fail_read.set(true);
        assert!(
            h.advance(0)
                .unwrap_err()
                .message
                .contains("合成运动读取失败")
        );
        let draw = h.draw();
        assert_eq!(h.runtime.numbers().shared[0], 3);
        h.fail_read.set(false);
        h.fail_create.set(true);
        assert!(
            h.advance(0)
                .unwrap_err()
                .message
                .contains("合成过渡创建失败")
        );
        assert_eq!(h.draw(), draw);
        h.fail_create.set(false);
        h.advance(0).unwrap();
        assert_eq!(h.draw(), draw);
        assert_eq!(h.runtime.numbers().shared[0], 3);
        assert_eq!(
            h.scene
                .objects()
                .media_records()
                .iter()
                .filter(|record| record.kind() == "DRAW")
                .count(),
            1
        );
        assert_eq!(h.attempts.get(), 2);
        assert_eq!(h.resolves.get(), 1);
        assert_eq!(h.reads.get(), 2);
    }
}
