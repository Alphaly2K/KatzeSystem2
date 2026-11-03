use katze_system2::{
    Error, HandleAllocator, Result,
    audio::{SoundConfig, SoundFactory},
    expression::State,
    fes::{self, ObjectDefinition},
    graphics::{GraphicsFactory, RenderTarget},
    kcs::{MotionHost, Program, ResourceServices},
    resource::ResourceMode,
    runtime::{
        BaseObject, ObjectFactory, Runtime, TaskObject,
        fes::{ObjectState, Objects, Scene},
        kind,
    },
    task::Command,
};
use std::{
    cell::{Cell, RefCell},
    io::Write,
    rc::Rc,
};

fn ins(code: &mut Vec<u8>, opcode: u8, operands: &[(u8, u32)]) {
    code.push(opcode);
    for &(flags, value) in operands {
        code.push(flags);
        code.extend(value.to_le_bytes());
    }
}

fn movement_code(stop: bool) -> Vec<u8> {
    let mut code = Vec::new();
    ins(&mut code, 0x16, &[(3, 0)]);
    code.push(0x2d);
    ins(&mut code, 1, &[(0x14, 0x8000_0018), (0x14, 0x8000_009c)]);
    ins(&mut code, 0x15, &[(4, 0x8000_0018), (0x44, 0)]);
    code.push(0x2d);
    if stop {
        ins(&mut code, 0x2b, &[(3, 38), (3, 0)]);
    }
    code.push(0x3b);
    code
}

fn movement(stop: bool) -> Program {
    Program::from_code(&movement_code(stop), vec![0; 4], 64).unwrap()
}

fn finishable_movement() -> Program {
    let mut code = movement_code(false);
    code.pop();
    ins(&mut code, 0x25, &[(0x13, 0x8000_0098)]);
    let branch = code.len();
    ins(&mut code, 0x28, &[(3, 0)]);
    code.push(0x3b);
    let finish = code.len() as u32;
    code[branch + 2..branch + 6].copy_from_slice(&finish.to_le_bytes());
    ins(&mut code, 0x2b, &[(3, 38), (3, 0)]);
    code.push(0x3b);
    Program::from_code(&code, vec![0; 4], 64).unwrap()
}

fn setup() -> (Objects, Vec<u32>) {
    let mut objects = Objects::new(HandleAllocator::default()).unwrap();
    let handles = objects
        .define(&ObjectDefinition {
            kind: "CIMAGE",
            name: "item".into(),
            banks: 2,
        })
        .unwrap();
    objects.set_parent(handles[1], Some(handles[0])).unwrap();
    for (&handle, x) in handles.iter().zip([10., 20.]) {
        let ObjectState::CImage(image) = &mut objects.get_mut(handle).unwrap().state else {
            panic!()
        };
        image.position = [x, 0.];
        image.parent_priority = 5;
        image.priority = 25;
    }
    (objects, handles)
}

fn position(objects: &Objects, handle: u32) -> f32 {
    let ObjectState::CImage(image) = &objects.get(handle).unwrap().state else {
        panic!()
    };
    image.position[0]
}

#[test]
fn new_motion_keeps_the_captured_base_while_the_object_or_a_descendant_is_active() {
    for budget in [1, 4096] {
        let (mut objects, handles) = setup();
        let [parent, child] = handles[..] else {
            panic!()
        };
        let mut host = MotionHost::default();
        objects
            .start_motion(parent, movement(true), &[2.], 0)
            .unwrap();
        while !objects.update_motions(100, &mut host, budget).unwrap() {}
        assert_eq!(position(&objects, parent), 12.);
        assert!(objects.get(parent).unwrap().motions().is_empty());
        assert!(!objects.motion_active(parent).unwrap());

        objects
            .start_motion(child, movement(false), &[1.], 0)
            .unwrap();
        let ObjectState::CImage(image) = &mut objects.get_mut(parent).unwrap().state else {
            panic!()
        };
        image.position[0] = 100.;
        assert!(objects.motion_active(parent).unwrap());
        assert_eq!(
            objects
                .start_motion(parent, movement(false), &[3.], 0)
                .unwrap(),
            2
        );
        while !objects.update_motions(101, &mut host, budget).unwrap() {}
        assert_eq!(position(&objects, parent), 13.);
        assert_eq!(position(&objects, child), 21.);
        let ObjectState::CImage(image) = &mut objects.get_mut(child).unwrap().state else {
            panic!()
        };
        image.updates = Default::default();
        objects
            .start_motion(parent, movement(false), &[4.], 0)
            .unwrap();
        while !objects.update_motions(102, &mut host, budget).unwrap() {}
        assert_eq!(position(&objects, parent), 17.);
        assert_eq!(position(&objects, child), 21.);
        let ObjectState::CImage(image) = &objects.get(child).unwrap().state else {
            panic!()
        };
        assert!(image.updates.is_dirty());
        for &handle in &handles {
            let ObjectState::CImage(image) = &objects.get(handle).unwrap().state else {
                panic!()
            };
            assert_eq!(image.parent_priority, 5);
        }
    }
}

fn random_program() -> Program {
    let mut code = Vec::new();
    ins(&mut code, 0x2b, &[(3, 39), (3, 0)]);
    ins(&mut code, 0x15, &[(3, 0), (0x43, 0)]);
    code.extend([0x2d, 0x3b]);
    Program::from_code(&code, vec![0; 4], 64).unwrap()
}

#[test]
fn motion_traversal_consumes_random_child_first_and_freezes_completed_objects_across_yields() {
    let program = random_program();
    for budget in [0, 1, 4096] {
        let (mut objects, handles) = setup();
        let mut host = MotionHost::default();
        let mut expected = MotionHost::default();
        let values = [
            expected.random.next_integer(),
            expected.random.next_integer(),
        ];
        for &handle in &handles {
            objects
                .start_motion(handle, program.clone(), &[], 0)
                .unwrap();
        }
        let mut complete = objects.update_motions(100, &mut host, budget).unwrap();
        if budget <= 1 {
            assert!(!complete);
            assert!(objects.remove(handles[0]).is_err());
            assert!(objects.set_parent(handles[1], None).is_err());
            assert!(
                objects
                    .start_motion(handles[0], program.clone(), &[], 0)
                    .is_err()
            );
        }
        for _ in 0..16 {
            if complete {
                break;
            }
            complete = objects.update_motions(999, &mut host, budget).unwrap();
        }
        assert!(complete);
        for (&handle, expected) in handles.iter().rev().zip(values) {
            let queue = objects.get(handle).unwrap().motions();
            assert_eq!(queue.time_origin(1), Some(100));
            let task = queue.task(1).unwrap();
            assert_eq!(task.state().unwrap().elapsed, 0);
            assert_eq!(task.interpreter.read(0, 4).unwrap(), expected.to_le_bytes());
        }
        assert_eq!(host.random.next_integer(), expected.random.next_integer());
    }
}

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂没有该类型"))
    }
}

struct Files {
    motion: Vec<u8>,
    missing_motion: Rc<Cell<bool>>,
    fail_motion: Rc<Cell<bool>>,
    fail_media: Rc<Cell<bool>>,
    reads: Rc<RefCell<Vec<Vec<u8>>>>,
    resolves: Rc<Cell<usize>>,
}
impl ResourceServices for Files {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(category, b"mot");
        self.resolves.set(self.resolves.get() + 1);
        Ok(match name {
            b"motion" => b"motion.kcs".to_vec(),
            b"MOTION" => b"MOTION.KCS".to_vec(),
            _ => panic!("合成资源没有该类别名称"),
        })
    }
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        self.reads.borrow_mut().push(name.to_vec());
        if name == b"motion.kcs" {
            if self.missing_motion.get() {
                return Ok(None);
            }
            if self.fail_motion.get() {
                return Err(Error::new(0, "合成运动读取失败"));
            }
            return Ok(Some(self.motion.clone()));
        }
        if self.fail_media.get() {
            return Err(Error::new(0, "合成媒体读取失败"));
        }
        Ok(None)
    }
}

struct SceneSetup {
    runtime: Runtime<SoundFactory<GraphicsFactory<Reject>>>,
    scene: Scene,
    list: u32,
    fail_motion: Rc<Cell<bool>>,
    missing_motion: Rc<Cell<bool>>,
    fail_media: Rc<Cell<bool>>,
    reads: Rc<RefCell<Vec<Vec<u8>>>>,
    resolves: Rc<Cell<usize>>,
}

fn scene_setup(source: &[u8], budget: usize) -> SceneSetup {
    let code = movement_code(false);
    let mut motion = b"KCS\0".to_vec();
    for word in [
        2,
        0,
        52,
        code.len() as u32,
        52 + code.len() as u32,
        0,
        0,
        4,
        64,
        52 + code.len() as u32,
        0,
        code.len() as u32,
    ] {
        motion.extend(word.to_le_bytes());
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&code).unwrap();
    motion.extend(encoder.finish().unwrap());
    let fail_motion = Rc::new(Cell::new(false));
    let missing_motion = Rc::new(Cell::new(false));
    let fail_media = Rc::new(Cell::new(false));
    let reads = Rc::new(RefCell::new(Vec::new()));
    let resolves = Rc::new(Cell::new(0));
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(8),
        SoundFactory {
            enabled: true,
            fallback: GraphicsFactory { fallback: Reject },
        },
    );
    runtime.set_budget(budget, 10000);
    runtime.set_resources(Files {
        motion,
        missing_motion: missing_motion.clone(),
        fail_motion: fail_motion.clone(),
        fail_media: fail_media.clone(),
        reads: reads.clone(),
        resolves: resolves.clone(),
    });
    runtime.drawing_mut().enabled = true;
    runtime.drawing_mut().active = true;
    runtime.drawing_mut().target = Some(RenderTarget::new([32, 4]).unwrap());
    let list = runtime.create(0, kind::IMAGE_LIST, b"list", b"").unwrap();
    for (code, a, b) in [(285, 32, 4), (289, 0, 0)] {
        runtime
            .dispatch_command(
                0,
                Command {
                    target: list,
                    sender: 0,
                    code,
                    data: [a, b, 0],
                },
            )
            .unwrap();
    }
    let mut scene = Scene::new(
        fes::Program::from_text(source).unwrap(),
        runtime.host().handle_allocator(),
    )
    .unwrap();
    scene
        .objects_mut()
        .unwrap()
        .set_image_list(list, katze_system2::graphics::Rect::from_xywh(0, 0, 32, 4));
    SceneSetup {
        runtime,
        scene,
        list,
        fail_motion,
        missing_motion,
        fail_media,
        reads,
        resolves,
    }
}

fn update_scene(setup: &mut SceneSetup, now: u32) {
    for _ in 0..256 {
        if setup
            .runtime
            .update_fes_scene(0, &mut setup.scene, now)
            .unwrap()
        {
            return;
        }
    }
    panic!("合成场景没有完成预算续跑");
}

fn counter(objects: &Objects, handle: u32, id: u32) -> u32 {
    u32::from_le_bytes(
        objects
            .get(handle)
            .unwrap()
            .motions()
            .task(id)
            .unwrap()
            .interpreter
            .read(0, 4)
            .unwrap()
            .try_into()
            .unwrap(),
    )
}

#[test]
fn plane_subcommands_read_local_base_and_vram_before_capturing_motion_priority() {
    let source = b"#DEFINE\nCPLANE solid\nPLANE canvas\n#OBJECT POS BASE SIZE VRAM COLOR PRI DISP\nsolid 4,1,2 3,2,9 1,1 0,0 0x80123456 7 0\ncanvas 8,0,3 5,6,10 1,1 11,12 0x40345678 8 0\n#SUBCOMMAND\noneshot move motion PX[0] BX[0] BZ[0] VX[0] RZ[0] SX[0] CA[0] : [0] [1] [2] [3] [4] [5] [6]\n#START\nsolid move @ @ @ @ @ @ @\ncanvas move @ @ @ @ @ @ @\nwait 1000\n";
    for budget in [1, 4096] {
        let mut setup = scene_setup(source, budget);
        update_scene(&mut setup, 100);
        for (name, parameters, priority, vram) in [
            ("solid", [4., 3., 9., 0., 0., 1., 128.], 7, [0.; 3]),
            ("canvas", [8., 5., 10., 11., 0., 1., 64.], 8, [11., 12., 0.]),
        ] {
            let handle = setup.scene.objects().lookup(name, 0).unwrap();
            let object = setup.scene.objects().get(handle).unwrap();
            let task = object.motions().task(1).unwrap();
            assert_eq!(
                task.state().unwrap().parameters[..7],
                parameters.map(f32::to_bits)
            );
            let (plane, context) = match &object.state {
                ObjectState::CPlane(plane) => (&**plane, plane.subcommand_context()),
                ObjectState::Plane(plane) => (&plane.base, plane.subcommand_context()),
                _ => unreachable!(),
            };
            assert_eq!(plane.priority(), priority);
            assert_eq!(plane.base_point()[2], priority as f32);
            assert_eq!(context.velocity, vram);
            assert_eq!(
                plane.color(),
                if name == "solid" {
                    0x80123456
                } else {
                    0x40345678
                }
            );
        }
    }
}

#[test]
fn sound_subcommands_keep_zero_vectors_and_wrapped_volume_before_real_media_updates() {
    let source = b"#DEFINE\nSOUND tone[2]\n#OBJECT CHANNEL\ntone[0] 5\ntone[1] 6\n#SUBCOMMAND\noneshot probe motion CA[0] PX[0] SX[0] : [0] [1] [2] CA\n#START\ntone probe @ @ @\nwait 1000\n";
    for budget in [1, 4096] {
        let mut setup = scene_setup(source, budget);
        let sound_task = setup.runtime.create(0, kind::SOUND, b"sound", b"").unwrap();
        setup.runtime.host_mut().sound_config_enabled = 1;
        setup.runtime.host_mut().sound_config = Some(SoundConfig {
            master_volume: Some(100),
            ..Default::default()
        });
        assert!(
            setup
                .scene
                .prepare(setup.runtime.numbers_mut(), 4096)
                .unwrap()
        );
        let objects = setup.scene.objects_mut().unwrap();
        objects.set_sound_task(sound_task);
        for (bank, volume, pan) in [(0, 329, -37), (1, -1, 51)] {
            let handle = objects.lookup("tone", bank).unwrap();
            let ObjectState::Sound(sound) = &mut objects.get_mut(handle).unwrap().state else {
                panic!()
            };
            sound.volume = volume;
            sound.pan = pan;
            sound.object_type = 2;
            sound.file = b"tone.ogg".to_vec();
        }
        update_scene(&mut setup, 100);
        for (bank, volume, pan) in [(0, 73, 36), (1, 255, 306)] {
            let handle = setup.scene.objects().lookup("tone", bank).unwrap();
            let object = setup.scene.objects().get(handle).unwrap();
            let task = object.motions().task(1).unwrap();
            assert_eq!(
                task.state().unwrap().parameters[..4],
                [volume as f32, 0., 1., volume as f32].map(f32::to_bits)
            );
            let ObjectState::Sound(sound) = &object.state else {
                panic!()
            };
            assert_eq!((sound.volume, sound.pan), (volume, pan));
        }
        for (channel, volume) in [(5, 73.), (6, 255.)] {
            assert_eq!(
                setup
                    .runtime
                    .object(sound_task)
                    .unwrap()
                    .audio_state()
                    .unwrap()
                    .channel(channel)
                    .unwrap()
                    .requested_volume(),
                volume
            );
        }
    }
}

#[test]
fn subcommands_retry_mapped_arrays_once_and_handlers_wait_for_the_next_motion_round() {
    let source = b"#DEFINE\nCIMAGE obj[2]\n#OBJECT SIZE COLOR PRI POS DISP\nobj[0] 1,1 0xff112233 5 10,0 1\nobj[1] 1,1 0xff445566 6 20,0 1\n#SUBCOMMAND\noneshot move motion px[0] : [0]\ncontinue alias MOTION px[0] : [0]\n#START\nobj move ($0+=1)\nwait 1000\n#obj.PUSH_L\n@ alias ($0+=1)\ndecode\n";
    for budget in [1, 4096] {
        let mut setup = scene_setup(source, budget);
        setup.fail_motion.set(true);
        for _ in 0..2 {
            loop {
                match setup.runtime.update_fes_scene(0, &mut setup.scene, 100) {
                    Ok(false) => (),
                    Err(error) => {
                        assert!(error.message.contains("运动读取失败"));
                        break;
                    }
                    result => panic!("{result:?}"),
                }
            }
            assert_eq!(setup.runtime.numbers().shared[0], 1);
        }
        setup.fail_motion.set(false);
        update_scene(&mut setup, 999);
        let handles: Vec<_> = (0..2)
            .map(|bank| setup.scene.objects().lookup("obj", bank).unwrap())
            .collect();
        assert_eq!(setup.runtime.numbers().shared[0], 2);
        for (&handle, expected) in handles.iter().zip([1., 2.]) {
            assert_eq!(position(setup.scene.objects(), handle), expected);
            assert_eq!(counter(setup.scene.objects(), handle, 1), 1);
            assert_eq!(
                setup
                    .scene
                    .objects()
                    .get(handle)
                    .unwrap()
                    .motions()
                    .time_origin(1),
                Some(100)
            );
        }
        assert_eq!(setup.resolves.get(), 1);
        assert_eq!(setup.reads.borrow().len(), 3);
        setup
            .runtime
            .dispatch_command(
                0,
                Command {
                    target: setup.list,
                    sender: 0,
                    code: 298,
                    data: [0; 3],
                },
            )
            .unwrap();
        assert_eq!(
            setup.runtime.drawing().target.as_ref().unwrap().pixel(1, 0),
            Some(0xff112233)
        );
        assert_eq!(
            setup.runtime.drawing().target.as_ref().unwrap().pixel(2, 0),
            Some(0xff445566)
        );

        let target = fes::ObjectReference {
            name: "obj".into(),
            bank: 1,
        };
        loop {
            if let Some(handled) = setup
                .runtime
                .dispatch_fes_event(0, &mut setup.scene, Some(&target), 2)
                .unwrap()
            {
                assert!(handled);
                break;
            }
        }
        assert_eq!(setup.runtime.numbers().shared[0], 3);
        assert_eq!(position(setup.scene.objects(), handles[1]), 2.);
        assert_eq!(counter(setup.scene.objects(), handles[1], 1), 1);
        assert_eq!(
            setup
                .scene
                .objects()
                .get(handles[1])
                .unwrap()
                .motions()
                .len(),
            2
        );
        assert_eq!(setup.reads.borrow().len(), 3);
        assert_eq!(setup.resolves.get(), 2);
        update_scene(&mut setup, 101);
        assert_eq!(position(setup.scene.objects(), handles[1]), 3.);
        assert_eq!(counter(setup.scene.objects(), handles[1], 1), 2);
        assert_eq!(counter(setup.scene.objects(), handles[1], 2), 1);
        let id = setup
            .scene
            .objects_mut()
            .unwrap()
            .start_motion(handles[1], random_program(), &[], 0)
            .unwrap();
        update_scene(&mut setup, 102);
        let mut expected = MotionHost::default();
        assert_eq!(
            counter(setup.scene.objects(), handles[1], id),
            expected.random.next_integer()
        );
        assert_eq!(
            setup.runtime.host_mut().random.next_integer(),
            expected.random.next_integer()
        );
    }
}

#[test]
fn decode_and_final_media_failures_keep_the_single_automatic_motion_phase() {
    for decode in ["", "decode\n"] {
        let source = format!(
            "#DEFINE\nIMAGE obj\n#OBJECT SIZE FILE PATHMODE PRI\nobj 1,1 absent 1 5\n#SUBCOMMAND\noneshot move motion p[2] : [0]\n#START\nobj move 2\n{decode}wait 1000\n"
        );
        for budget in [1, 4096] {
            let mut setup = scene_setup(source.as_bytes(), budget);
            setup.fail_media.set(true);
            for _ in 0..2 {
                loop {
                    match setup.runtime.update_fes_scene(0, &mut setup.scene, 100) {
                        Ok(false) => (),
                        Err(error) => {
                            assert!(error.message.contains("媒体读取失败"));
                            break;
                        }
                        result => panic!("{result:?}"),
                    }
                }
                let handle = setup.scene.objects().lookup("obj", 0).unwrap();
                assert_eq!(
                    counter(setup.scene.objects(), handle, 1),
                    u32::from(decode.is_empty())
                );
            }
            setup.fail_media.set(false);
            update_scene(&mut setup, 999);
            let handle = setup.scene.objects().lookup("obj", 0).unwrap();
            assert_eq!(counter(setup.scene.objects(), handle, 1), 1);
            update_scene(&mut setup, 101);
            assert_eq!(counter(setup.scene.objects(), handle, 1), 2);
        }
    }
}

#[test]
fn missing_motion_resources_continue_the_script_and_can_load_on_a_later_call() {
    let source = b"#DEFINE\nCIMAGE obj[2]\n#OBJECT SIZE COLOR PRI POS DISP\nobj[0] 1,1 0xff112233 5 10,0 1\nobj[1] 1,1 0xff445566 6 20,0 1\n#SUBCOMMAND\noneshot move motion p[1] : [0]\n#START\nobj move ($0+=1)\nwait 1\nobj move ($0+=1)\nwait 1000\n";
    for budget in [1, 4096] {
        let mut setup = scene_setup(source, budget);
        setup.missing_motion.set(true);
        update_scene(&mut setup, 100);
        assert_eq!(setup.runtime.numbers().shared[0], 2);
        for bank in 0..2 {
            let handle = setup.scene.objects().lookup("obj", bank).unwrap();
            assert!(
                setup
                    .scene
                    .objects()
                    .get(handle)
                    .unwrap()
                    .motions()
                    .is_empty()
            );
        }
        setup.missing_motion.set(false);
        update_scene(&mut setup, 101);
        assert_eq!(setup.runtime.numbers().shared[0], 4);
        assert_eq!(setup.resolves.get(), 1);
        assert_eq!(setup.reads.borrow().len(), 3);
        for (bank, expected) in [13., 24.].into_iter().enumerate() {
            let handle = setup.scene.objects().lookup("obj", bank as i32).unwrap();
            assert_eq!(position(setup.scene.objects(), handle), expected);
            assert_eq!(counter(setup.scene.objects(), handle, 1), 1);
        }
    }
}

fn metadata(
    objects: &mut Objects,
    interpreter: &mut fes::Interpreter,
    numbers: &mut State,
    line: &[u8],
) -> Result<()> {
    assert!(objects.dispatch_object_command(
        interpreter,
        &fes::Line {
            offset: 100,
            bytes: line.to_vec()
        },
        numbers,
    )?);
    Ok(())
}

#[test]
fn motion_metadata_keeps_raw_delays_and_preserves_the_destination_at_a_bank_hole() {
    let mut objects = Objects::new(HandleAllocator::default()).unwrap();
    let handles = objects
        .define(&ObjectDefinition {
            kind: "CIMAGE",
            name: "item".into(),
            banks: 4,
        })
        .unwrap();
    objects.get_mut(handles[1]).unwrap().state = ObjectState::Image(Box::default());
    objects.get_mut(handles[2]).unwrap().state = ObjectState::CPlane(Box::default());
    objects.get_mut(handles[3]).unwrap().state = ObjectState::Base;
    objects.set_parent(handles[1], Some(handles[0])).unwrap();
    let mut interpreter =
        fes::Interpreter::new(fes::Program::from_text(b"#START\nwait 1000\n").unwrap()).unwrap();
    objects.publish_bindings(&mut interpreter);
    let mut numbers = State::new(16);
    numbers.current = 13;
    metadata(
        &mut objects,
        &mut interpreter,
        &mut numbers,
        b"item delay ($0+=1) ($0+=10)",
    )
    .unwrap();
    assert_eq!(numbers.shared[0], 1);
    for &handle in &handles {
        assert_eq!(objects.get(handle).unwrap().motion_delay(), 1);
    }
    objects
        .start_motion(handles[1], movement(false), &[2.], 0)
        .unwrap();
    metadata(
        &mut objects,
        &mut interpreter,
        &mut numbers,
        b"item[0] ismot 4",
    )
    .unwrap();
    assert_eq!(numbers.shared[4], 1);
    metadata(
        &mut objects,
        &mut interpreter,
        &mut numbers,
        b"item[3] ismot 4",
    )
    .unwrap();
    assert_eq!(numbers.shared[4], 0);
    metadata(
        &mut objects,
        &mut interpreter,
        &mut numbers,
        b"item[3] delay -3",
    )
    .unwrap();
    assert_eq!(
        objects.get(handles[3]).unwrap().motion_delay(),
        u32::MAX - 2
    );

    objects.remove(handles[2]).unwrap();
    metadata(
        &mut objects,
        &mut interpreter,
        &mut numbers,
        b"item delay 21",
    )
    .unwrap();
    assert_eq!(objects.get(handles[0]).unwrap().motion_delay(), 21);
    assert_eq!(objects.get(handles[1]).unwrap().motion_delay(), 21);
    assert_eq!(
        objects.get(handles[3]).unwrap().motion_delay(),
        u32::MAX - 2
    );
    numbers.shared[2] = 91;
    metadata(
        &mut objects,
        &mut interpreter,
        &mut numbers,
        b"item ismot 2 ($0+=10)",
    )
    .unwrap();
    assert_eq!(numbers.shared[2], 91);
    assert_eq!(numbers.shared[0], 1);
    metadata(
        &mut objects,
        &mut interpreter,
        &mut numbers,
        b"item[1] ismot ($0+=1)",
    )
    .unwrap();
    assert_eq!(numbers.shared[0], 2);
    assert_eq!(numbers.shared[2], 1);
    metadata(&mut objects, &mut interpreter, &mut numbers, b"item ismot").unwrap();
    metadata(
        &mut objects,
        &mut interpreter,
        &mut numbers,
        b"item[3] delay",
    )
    .unwrap();
    assert_eq!(objects.get(handles[3]).unwrap().motion_delay(), 0);
    assert!(
        metadata(
            &mut objects,
            &mut interpreter,
            &mut numbers,
            b"item[99] delay ($0+=1)"
        )
        .is_ok()
    );
    assert_eq!(numbers.shared[0], 3);
    assert_eq!(numbers.current, 13);
}

#[test]
fn scene_motion_delay_defers_vm_execution_and_running_queries_include_unstarted_entries() {
    let source = b"#DEFINE\nCIMAGE obj[2]\n#OBJECT SIZE COLOR PRI POS DISP\nobj[0] 1,1 0xff112233 5 10,0 1\nobj[1] 1,1 0xff445566 6 20,0 1\n#SUBCOMMAND\noneshot move motion p[2] : [0]\n#START\nobj delay ($0+=3)\nobj move 2\nobj ismot 2\nwait 1000\n";
    for budget in [1, 4096] {
        let mut setup = scene_setup(source, budget);
        for now in 100..=103 {
            update_scene(&mut setup, now);
            assert_eq!(setup.runtime.numbers().shared[0], 3);
            assert_eq!(setup.runtime.numbers().shared[2], 1);
            for (bank, initial) in [10., 20.].into_iter().enumerate() {
                let handle = setup.scene.objects().lookup("obj", bank as i32).unwrap();
                let object = setup.scene.objects().get(handle).unwrap();
                assert_eq!(object.motion_delay(), 3);
                assert_eq!(object.motions().task(1).unwrap().base_time, 3);
                assert_eq!(
                    counter(setup.scene.objects(), handle, 1),
                    u32::from(now == 103)
                );
                assert_eq!(
                    position(setup.scene.objects(), handle),
                    initial + if now == 103 { 2. } else { 0. }
                );
            }
        }
    }
}

#[test]
fn stop_updates_delayed_descendants_once_and_only_clears_the_selected_queue() {
    for budget in [0, 1, 4096] {
        for obey in [false, true] {
            let (mut objects, handles) = setup();
            let [parent, child] = handles[..] else {
                panic!()
            };
            let mut interpreter =
                fes::Interpreter::new(fes::Program::from_text(b"#START\nwait 1000\n").unwrap())
                    .unwrap();
            objects.publish_bindings(&mut interpreter);
            metadata(
                &mut objects,
                &mut interpreter,
                &mut State::new(4),
                b"item delay 3",
            )
            .unwrap();
            objects
                .start_motion(parent, movement(false), &[2.], 3)
                .unwrap();
            objects
                .start_motion(
                    child,
                    if obey {
                        finishable_movement()
                    } else {
                        movement(false)
                    },
                    &[3.],
                    3,
                )
                .unwrap();
            let mut host = MotionHost::default();
            let mut complete = objects.stop_motions(parent, &mut host, budget).unwrap();
            if budget <= 1 {
                assert!(!complete);
                assert!(objects.update_motions(100, &mut host, budget).is_err());
                assert!(objects.stop_motions(child, &mut host, budget).is_err());
            }
            for _ in 0..64 {
                if complete {
                    break;
                }
                complete = objects.stop_motions(parent, &mut host, budget).unwrap();
            }
            assert!(complete);
            assert_eq!(position(&objects, parent), 12.);
            assert_eq!(position(&objects, child), 23.);
            assert!(objects.get(parent).unwrap().motions().is_empty());
            assert_eq!(objects.get(parent).unwrap().motion_delay(), 0);
            assert_eq!(objects.get(child).unwrap().motion_delay(), 3);
            assert_eq!(objects.motion_active(parent).unwrap(), !obey);
            if obey {
                assert!(objects.get(child).unwrap().motions().is_empty());
            } else {
                assert_eq!(counter(&objects, child, 1), 1);
                let queue = objects.get(child).unwrap().motions();
                let state = queue.task(1).unwrap().state().unwrap();
                assert_eq!(queue.time_origin(1), Some(0));
                assert_eq!(state.elapsed, u32::MAX - 2);
                assert_eq!(state.finishing, 1);
                while !objects.update_motions(100, &mut host, budget).unwrap() {}
                assert_eq!(counter(&objects, child, 1), 2);
                assert_eq!(position(&objects, child), 23.);
            }
        }
    }
}

#[test]
fn scene_stop_skips_bank_holes_and_event_stop_applies_before_media_submission() {
    let source = b"#DEFINE\nCIMAGE obj[3]\n#OBJECT SIZE COLOR PRI POS DISP\nobj[0] 1,1 0xff112233 5 0,0 1\nobj[1] 1,1 0xff445566 6 2,0 1\nobj[2] 1,1 0xff778899 7 4,0 1\n#SUBCOMMAND\noneshot move motion p[2] : [0]\n#START\nobj delay 3\nobj move 2\nwait 1\nobj[($0+=1)-1] stop ($0+=100)\nobj stop ($0+=100)\nobj ismot 2\n$3+=1\nwait 1000\n#obj.PUSH_L\n@ move 4\n@ delay 9\n@ stop ($0+=100)\n$4+=1\ndecode\n$5+=1\n";
    for budget in [1, 4096] {
        let mut setup = scene_setup(source, budget);
        update_scene(&mut setup, 100);
        let handles: Vec<_> = (0..3)
            .map(|bank| setup.scene.objects().lookup("obj", bank).unwrap())
            .collect();
        setup
            .runtime
            .remove_fes_object(0, setup.scene.objects_mut().unwrap(), handles[1])
            .unwrap();
        setup.runtime.numbers_mut().shared[2] = 91;
        update_scene(&mut setup, 101);
        assert_eq!(&setup.runtime.numbers().shared[..4], &[1, 0, 91, 1]);
        for (handle, x) in [(handles[0], 2.), (handles[2], 6.)] {
            let object = setup.scene.objects().get(handle).unwrap();
            assert_eq!(position(setup.scene.objects(), handle), x);
            assert_eq!(object.motion_delay(), 0);
            assert!(object.motions().is_empty());
        }
        let target = fes::ObjectReference {
            name: "obj".into(),
            bank: 2,
        };
        loop {
            if let Some(handled) = setup
                .runtime
                .dispatch_fes_event(0, &mut setup.scene, Some(&target), 2)
                .unwrap()
            {
                assert!(handled);
                break;
            }
        }
        let object = setup.scene.objects().get(handles[2]).unwrap();
        assert_eq!(position(setup.scene.objects(), handles[2]), 10.);
        assert_eq!(object.motion_delay(), 0);
        assert!(object.motions().is_empty());
        assert_eq!(&setup.runtime.numbers().shared[4..6], &[1, 0]);
        assert_eq!(setup.runtime.numbers().shared[0], 1);
        update_scene(&mut setup, 102);
        setup
            .runtime
            .dispatch_command(
                0,
                Command {
                    target: setup.list,
                    sender: 0,
                    code: 298,
                    data: [0; 3],
                },
            )
            .unwrap();
        assert_eq!(
            setup
                .runtime
                .drawing()
                .target
                .as_ref()
                .unwrap()
                .pixel(10, 0),
            Some(0xff778899)
        );
    }
}

#[test]
fn root_motion_wait_defers_resumption_and_click_requests_finish_without_clearing_descendants() {
    for budget in [1, 4096] {
        for target in ["", " obj"] {
            for click in [false, true] {
                let source = format!(
                    "#DEFINE\nCIMAGE obj[2]\n#OBJECT SIZE COLOR PRI POS DISP\nobj[0] 1,1 0xff112233 5 0,0 1\nobj[1] 1,1 0xff445566 6 2,0 1\n#START\nwait{target}\n$0+=1\nwait 1000\n"
                );
                let mut setup = scene_setup(source.as_bytes(), budget);
                let mut prepared = false;
                for _ in 0..64 {
                    if setup
                        .scene
                        .prepare(setup.runtime.numbers_mut(), budget)
                        .unwrap()
                    {
                        prepared = true;
                        break;
                    }
                }
                assert!(prepared);
                let parent = setup.scene.objects().lookup("obj", 0).unwrap();
                let child = setup.scene.objects().lookup("obj", 1).unwrap();
                let objects = setup.scene.objects_mut().unwrap();
                objects.set_parent(child, Some(parent)).unwrap();
                objects
                    .start_motion(
                        child,
                        if click {
                            finishable_movement()
                        } else {
                            movement(true)
                        },
                        &[3.],
                        0,
                    )
                    .unwrap();
                update_scene(&mut setup, 100);
                assert_eq!(setup.runtime.numbers().shared[0], 0);
                if click {
                    assert_eq!(
                        setup
                            .runtime
                            .dispatch_fes_event(0, &mut setup.scene, None, 2)
                            .unwrap(),
                        Some(false)
                    );
                }
                update_scene(&mut setup, 101);
                if target.is_empty() {
                    assert_eq!(setup.runtime.numbers().shared[0], 0);
                    assert_eq!(
                        setup.scene.deadline(),
                        Some(if click { u32::MAX } else { 0 })
                    );
                    assert!(
                        setup
                            .scene
                            .objects()
                            .get(child)
                            .unwrap()
                            .motions()
                            .is_empty()
                    );
                    update_scene(&mut setup, 102);
                    if click {
                        assert_eq!(setup.runtime.numbers().shared[0], 0);
                        assert_eq!(setup.scene.deadline(), Some(0));
                        update_scene(&mut setup, 103);
                    }
                    assert_eq!(setup.runtime.numbers().shared[0], 1);
                } else {
                    assert_eq!(setup.runtime.numbers().shared[0], 1);
                    assert_eq!(setup.scene.deadline(), Some(1101));
                    assert_eq!(setup.scene.objects().motion_active(parent).unwrap(), click);
                    if click {
                        let queue = setup.scene.objects().get(child).unwrap().motions();
                        assert_eq!(queue.task(1).unwrap().state().unwrap().finishing, 0);
                        assert_eq!(counter(setup.scene.objects(), child, 1), 2);
                    }
                }
            }
        }
    }
}
