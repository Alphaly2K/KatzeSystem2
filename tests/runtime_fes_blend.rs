use katze_system2::{
    Error, Result,
    expression::State,
    fes::{ObjectReference, Program},
    kcs::ResourceServices,
    resource::ResourceMode,
    runtime::{
        BaseObject, ObjectFactory, Runtime, TaskObject,
        fes::{ObjectState, Scene},
        kind,
    },
};
use std::{cell::Cell, io::Write, rc::Rc};

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成场景不创建下层任务"))
    }
}

#[derive(Clone, Default)]
struct Files {
    motion: Option<Vec<u8>>,
    fail: Rc<Cell<bool>>,
    resolves: Rc<Cell<usize>>,
    reads: Rc<Cell<usize>>,
}
impl ResourceServices for Files {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!((category, name), (b"mot".as_slice(), b"p".as_slice()));
        self.resolves.set(self.resolves.get() + 1);
        Ok(b"motions/p.kcs".to_vec())
    }
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(name, b"motions/p.kcs");
        assert_eq!(mode, ResourceMode::LooseFirst);
        self.reads.set(self.reads.get() + 1);
        if self.fail.get() {
            return Err(Error::new(0, "合成运动读取暂时失败"));
        }
        Ok(self.motion.clone())
    }
}

fn motion() -> Vec<u8> {
    let mut code = vec![0x15, 3];
    code.extend(0x80000070u32.to_le_bytes());
    code.push(3);
    code.extend(0x23000000u32.to_le_bytes());
    code.push(0x3b);
    let length = code.len() as u32;
    let mut bytes = b"KCS\0".to_vec();
    for word in [
        2,
        0,
        52,
        length,
        52 + length,
        0,
        0,
        0,
        64,
        52 + length,
        0,
        length,
    ] {
        bytes.extend(word.to_le_bytes());
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&code).unwrap();
    bytes.extend(encoder.finish().unwrap());
    bytes
}

fn setup(source: &[u8], budget: usize, files: Files) -> (Runtime<Reject>, Scene) {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(8),
        Reject,
    );
    runtime.set_budget(budget, 10000);
    runtime.set_resources(files);
    let scene = Scene::new(
        Program::from_text(source).unwrap(),
        runtime.host().handle_allocator(),
    )
    .unwrap();
    (runtime, scene)
}

fn update(runtime: &mut Runtime<Reject>, scene: &mut Scene, now: u32) -> Result<()> {
    for _ in 0..256 {
        if runtime.update_fes_scene(0, scene, now)? {
            return Ok(());
        }
    }
    panic!("合成场景没有完成本轮更新");
}

fn system(runtime: &mut Runtime<Reject>, scene: &mut Scene) {
    for _ in 0..256 {
        if runtime
            .dispatch_fes_system_block(0, scene, "_FES_SLEEP_")
            .unwrap()
        {
            return;
        }
    }
    panic!("合成系统块没有完成");
}

fn value(state: &ObjectState) -> u32 {
    match state {
        ObjectState::Sound(sound) => sound.volume as u32,
        ObjectState::CImage(image) => image.blend as u32,
        ObjectState::Image(image) => image.base.blend as u32,
        ObjectState::Button(button) => button.image.base.blend as u32,
        ObjectState::Slider(control) => control.alpha,
        ObjectState::Scroll2(control) => control.control.alpha,
        _ => panic!("合成目标类型无音量或透明度"),
    }
}

#[test]
fn plane_blend_keeps_rgb_and_raw_alpha_parameters_across_resource_retry() {
    for kind in ["CPLANE", "PLANE"] {
        for budget in [1, 4096] {
            let source = format!(
                "#DEFINE\n{kind} panel\nCPLANE child\n#OBJECT COLOR\npanel 0xab123456\n#START\nwait 99\n#_FES_SLEEP_\npanel blend ($0-=1) ($1+=100)\n"
            );
            let files = Files {
                motion: Some(motion()),
                ..Default::default()
            };
            files.fail.set(true);
            let (mut runtime, mut scene) = setup(source.as_bytes(), budget, files.clone());
            while !scene.prepare(runtime.numbers_mut(), budget).unwrap() {}
            let handle = scene.objects().lookup("panel", 0).unwrap();
            let child = scene.objects().lookup("child", 0).unwrap();
            scene
                .objects_mut()
                .unwrap()
                .set_parent(child, Some(handle))
                .unwrap();
            let mut failed = false;
            for _ in 0..256 {
                match runtime.dispatch_fes_system_block(0, &mut scene, "_FES_SLEEP_") {
                    Err(_) => {
                        failed = true;
                        break;
                    }
                    Ok(false) => (),
                    Ok(true) => panic!("合成读取没有失败"),
                }
            }
            assert!(failed);
            assert_eq!(runtime.numbers().shared[..2], [-1, 0]);
            files.fail.set(false);
            system(&mut runtime, &mut scene);
            assert_eq!(runtime.numbers().shared[..2], [-1, 0]);
            assert_eq!((files.resolves.get(), files.reads.get()), (1, 2));
            let object = scene.objects().get(handle).unwrap();
            let color = match &object.state {
                ObjectState::CPlane(plane) => plane.color(),
                ObjectState::Plane(plane) => plane.base.color(),
                _ => unreachable!(),
            };
            assert_eq!(color, 0xff123456);
            let queue = object.motions();
            assert_eq!(queue.base_properties[28], color);
            assert_eq!(
                queue.task(1).unwrap().state().unwrap().parameters[..4],
                [-1f32, 18., 52., 86.].map(f32::to_bits)
            );
            let ObjectState::CPlane(child) = &scene.objects().get(child).unwrap().state else {
                unreachable!()
            };
            assert!(child.is_dirty());
        }
    }
}

#[test]
fn sound_blend_retries_once_then_runs_default_motion_at_the_normal_update_boundary() {
    let source = b"#DEFINE\nSOUND sound[3]\nBASE ignored\n#START\nsound[1] blend ($0+=257)\nignored blend ($1+=7)\nwait 99\n#_FES_SLEEP_\nsound blend ($0+=3)\n#sound.PUSH_L\n@ blend ($0+=1)\n";
    for budget in [1, 4096] {
        for available in [false, true] {
            let files = Files {
                motion: available.then(motion),
                ..Default::default()
            };
            files.fail.set(true);
            let (mut runtime, mut scene) = setup(source, budget, files.clone());
            assert!(update(&mut runtime, &mut scene, 100).is_err());
            assert_eq!(runtime.numbers().shared[..2], [257, 0]);
            let target = scene.objects().lookup("sound", 1).unwrap();
            assert_eq!(value(&scene.objects().get(target).unwrap().state), 257);
            assert!(scene.objects().get(target).unwrap().motions().is_empty());
            assert!(scene.objects_mut().is_err());
            files.fail.set(false);
            update(&mut runtime, &mut scene, 100).unwrap();
            assert_eq!(runtime.numbers().shared[..2], [257, 7]);
            assert_eq!(files.resolves.get(), 1);
            assert_eq!(files.reads.get(), 2);
            assert_eq!(
                value(&scene.objects().get(target).unwrap().state),
                if available { 35 } else { 257 }
            );
            let deadline = scene.deadline();
            system(&mut runtime, &mut scene);
            for bank in 0..3 {
                let id = scene.objects().lookup("sound", bank).unwrap();
                assert_eq!(value(&scene.objects().get(id).unwrap().state), 260);
            }
            let target_ref = ObjectReference {
                name: "sound".into(),
                bank: 1,
            };
            let mut done = false;
            for _ in 0..256 {
                if let Some(handled) = runtime
                    .dispatch_fes_event(0, &mut scene, Some(&target_ref), 2)
                    .unwrap()
                {
                    assert!(handled);
                    done = true;
                    break;
                }
            }
            assert!(done);
            assert_eq!(value(&scene.objects().get(target).unwrap().state), 261);
            assert_eq!(scene.deadline(), deadline);
            assert_eq!(files.resolves.get(), 5);
            assert_eq!(files.reads.get(), if available { 2 } else { 6 });
            update(&mut runtime, &mut scene, 101).unwrap();
            assert_eq!(
                value(&scene.objects().get(target).unwrap().state),
                if available { 35 } else { 261 }
            );
        }
    }
}

#[test]
fn blend_preserves_explicit_banks_skips_holes_and_evaluates_array_values_once() {
    for kind in ["SOUND", "CIMAGE", "IMAGE", "BUTTON", "SLIDER", "SCROLL2"] {
        for count in [1, 3] {
            for budget in [1, 4096] {
                let source = format!(
                    "#DEFINE\n{kind} item[{count}]\n{kind} child\n#START\nwait 99\n#_FES_SLEEP_\nitem[99] blend ($0+=2)\nitem blend ($0+=3)\nitem[2] blend ($1-=1)\n"
                );
                let files = Files::default();
                let (mut runtime, mut scene) = setup(source.as_bytes(), budget, files.clone());
                while !scene.prepare(runtime.numbers_mut(), budget).unwrap() {}
                let objects = scene.objects_mut().unwrap();
                let first = objects.lookup("item", 0).unwrap();
                let child = objects.lookup("child", 0).unwrap();
                objects.set_parent(child, Some(first)).unwrap();
                if count == 3 {
                    objects.remove(objects.lookup("item", 1).unwrap()).unwrap();
                }
                system(&mut runtime, &mut scene);
                assert_eq!(runtime.numbers().shared[..2], [5, -1]);
                assert_eq!(value(&scene.objects().get(first).unwrap().state), 5);
                if count == 3 {
                    let last = scene.objects().lookup("item", 2).unwrap();
                    assert_eq!(value(&scene.objects().get(last).unwrap().state), u32::MAX);
                }
                let resolves = if count == 1 { 1 } else { 3 };
                assert_eq!(files.resolves.get(), resolves);
                assert_eq!(files.reads.get(), resolves);
                let state = &scene.objects().get(child).unwrap().state;
                match state {
                    ObjectState::Sound(sound) => assert!(sound.is_dirty()),
                    ObjectState::CImage(image) => assert!(image.updates.is_dirty()),
                    ObjectState::Image(image) => assert!(image.base.updates.is_dirty()),
                    ObjectState::Button(button) => assert!(button.image.base.updates.is_dirty()),
                    ObjectState::Slider(control) => assert!(!control.updates.alpha),
                    ObjectState::Scroll2(control) => assert!(!control.control.updates.alpha),
                    _ => unreachable!(),
                }
            }
        }
    }
}
