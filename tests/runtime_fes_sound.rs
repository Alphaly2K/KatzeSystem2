use katze_system2::{
    Error, HandleAllocator, Result,
    audio::{SoundConfig, SoundPart},
    expression::State,
    fes::{
        ObjectAttributes, ObjectDefinition, ObjectInitialization, ObjectReference, Program,
        SoundState,
    },
    kcs::{MotionHost, Program as MotionProgram, ScriptRandom},
    runtime::{
        BaseObject, ObjectFactory, Runtime, TaskObject,
        fes::{ObjectState, Objects, Scene},
        kind,
    },
};

fn define(objects: &mut Objects, kind: &'static str, name: &str, banks: u32) -> Vec<u32> {
    objects
        .define(&ObjectDefinition {
            kind,
            name: name.into(),
            banks,
        })
        .unwrap()
}

fn initialize(objects: &mut Objects, bank: i32, attributes: ObjectAttributes) -> Result<()> {
    objects.initialize(
        &ObjectInitialization {
            target: ObjectReference {
                name: "sound".into(),
                bank,
            },
            attributes,
        },
        |_| panic!("声音声明不调用图像名称转换"),
    )
}

#[test]
fn sound_attributes_keep_raw_names_bank_values_and_distinct_update_requests() {
    let mut objects = Objects::new(HandleAllocator::default()).unwrap();
    let sounds = define(&mut objects, "SOUND", "sound", 2);
    let child = define(&mut objects, "CIMAGE", "child", 1)[0];
    objects.set_parent(child, Some(sounds[1])).unwrap();
    initialize(
        &mut objects,
        -1,
        ObjectAttributes {
            object_type: Some(-7),
            group: Some(b"voice\0tail".to_vec()),
            blend: Some(87),
            position: [Some(99); 3],
            ..Default::default()
        },
    )
    .unwrap();
    for sound in &sounds {
        let ObjectState::Sound(state) = &objects.get(*sound).unwrap().state else {
            panic!()
        };
        assert_eq!(
            (state.channel, state.sound_type, state.object_type),
            (-1, 0, -7)
        );
        assert_eq!(state.group, b"voice");
        assert_eq!((state.volume, state.pan), (100, 0));
        assert!(!state.is_dirty());
        assert!(!state.needs_load());
    }
    initialize(
        &mut objects,
        1,
        ObjectAttributes {
            file: Some(b"raw.ogg\0tail".to_vec()),
            ..Default::default()
        },
    )
    .unwrap();
    let ObjectState::Sound(state) = &objects.get(sounds[1]).unwrap().state else {
        panic!()
    };
    assert_eq!(state.file, b"raw.ogg");
    assert!(state.needs_load());
    assert!(!state.is_dirty());
    let ObjectState::CImage(image) = &objects.get(child).unwrap().state else {
        panic!()
    };
    assert!(!image.updates.is_dirty());
    initialize(
        &mut objects,
        1,
        ObjectAttributes {
            sound_type: Some(-3),
            channel: Some(61),
            ..Default::default()
        },
    )
    .unwrap();
    let ObjectState::CImage(image) = &objects.get(child).unwrap().state else {
        panic!()
    };
    assert!(image.updates.is_dirty());
    let ObjectState::Sound(state) = &objects.get(sounds[0]).unwrap().state else {
        panic!()
    };
    assert!(!state.is_dirty());
    let ObjectState::Sound(state) = &objects.get(sounds[1]).unwrap().state else {
        panic!()
    };
    assert_eq!((state.sound_type, state.channel), (-3, 61));
    assert!(state.is_dirty());

    let mut state = SoundState::default();
    state
        .apply(&ObjectAttributes {
            file: Some(vec![b'a'; 259]),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(state.file.len(), 259);
    assert!(
        state
            .apply(&ObjectAttributes {
                file: Some(vec![b'b'; 260]),
                ..Default::default()
            })
            .is_err()
    );
    assert_eq!(state.file, vec![b'a'; 259]);
    assert!(
        state
            .apply(&ObjectAttributes {
                sound_type: Some(9),
                group: Some(vec![b'g'; 32]),
                ..Default::default()
            })
            .is_err()
    );
    assert_eq!(state.sound_type, 9);
    assert!(state.is_dirty());
}

#[test]
fn sound_motion_uses_pan_and_volume_only_and_preserves_x86_integer_conversion() {
    let mut state = SoundState::default();
    let properties = state.motion_properties();
    assert_eq!(properties[28], 100 << 24);
    assert!(
        properties
            .iter()
            .enumerate()
            .all(|(i, &v)| i == 28 || v == 0)
    );
    assert!(!state.apply_motion_properties(&properties));
    for (pan, expected) in [
        (0x80000000, 0),
        (1.75f32.to_bits(), 1),
        ((-2.75f32).to_bits(), -2),
        (f32::INFINITY.to_bits(), i32::MIN),
        (f32::NAN.to_bits(), i32::MIN),
        (2147483648.0f32.to_bits(), i32::MIN),
        (2147483520.0f32.to_bits(), 2147483520),
    ] {
        let mut properties = [u32::MAX; 36];
        properties[6] = pan;
        properties[28] = 0xab123456;
        state.apply_motion_properties(&properties);
        assert_eq!((state.pan, state.volume), (expected, 0xab));
        let captured = state.motion_properties();
        assert_eq!(captured[6], (expected as f32).to_bits());
        assert_eq!(captured[28], 0xab000000);
        assert!(
            captured
                .iter()
                .enumerate()
                .all(|(i, &v)| matches!(i, 6 | 28) || v == 0)
        );
    }
    for budget in [1, 4096] {
        let mut objects = Objects::new(HandleAllocator::default()).unwrap();
        let sound = define(&mut objects, "SOUND", "sound", 1)[0];
        let child = define(&mut objects, "CIMAGE", "child", 1)[0];
        objects.set_parent(child, Some(sound)).unwrap();
        let mut code = Vec::new();
        for (opcode, operands) in [
            (0x16, vec![(3, 0)]),
            (0x2d, vec![]),
            (1, vec![(0x14, 0x8000_0018), (0x14, 0x8000_009c)]),
            (0x15, vec![(4, 0x8000_0018), (0x44, 0)]),
            (0x2d, vec![]),
            (0x3b, vec![]),
        ] {
            code.push(opcode);
            for (flags, value) in operands {
                code.push(flags);
                code.extend(u32::to_le_bytes(value));
            }
        }
        let program = MotionProgram::from_code(&code, vec![0; 4], 64).unwrap();
        objects.start_motion(sound, program, &[2.], 0).unwrap();
        let mut host = MotionHost::default();
        let mut done = false;
        for _ in 0..64 {
            if objects.update_motions(10, &mut host, budget).unwrap() {
                done = true;
                break;
            }
        }
        assert!(done);
        let ObjectState::Sound(state) = &objects.get(sound).unwrap().state else {
            panic!()
        };
        assert_eq!((state.pan, state.volume), (2, 100));
        assert!(state.is_dirty());
        let ObjectState::CImage(image) = &objects.get(child).unwrap().state else {
            panic!()
        };
        assert!(image.updates.is_dirty());
    }
}

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂没有该类型"))
    }
}

#[test]
fn silent_sound_declarations_allow_scene_updates_and_load_errors_keep_requests_for_retry() {
    for budget in [1, 4096] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(4),
            Reject,
        );
        runtime.set_budget(budget, 10000);
        let source = b"#DEFINE\nSOUND sound[2]\n#OBJECT GROUP STYPE TYPE\nsound voice 2 7\n#START\n$0+=1\nwait 99\n";
        let mut scene = Scene::new(
            Program::from_text(source).unwrap(),
            runtime.host().handle_allocator(),
        )
        .unwrap();
        let mut done = false;
        for _ in 0..64 {
            if runtime.update_fes_scene(0, &mut scene, 10).unwrap() {
                done = true;
                break;
            }
        }
        assert!(done);
        assert_eq!(runtime.numbers().shared[0], 1);
        assert_eq!(scene.deadline(), Some(109));
        let id = scene.objects().lookup("sound", 1).unwrap();
        let ObjectState::Sound(sound) = &scene.objects().get(id).unwrap().state else {
            panic!()
        };
        assert!(!sound.is_dirty());
        assert!(!sound.needs_load());
        let ObjectState::Sound(sound) =
            &mut scene.objects_mut().unwrap().get_mut(id).unwrap().state
        else {
            panic!()
        };
        sound
            .apply(&ObjectAttributes {
                file: Some(b"asset.ogg".to_vec()),
                ..Default::default()
            })
            .unwrap();
        for _ in 0..2 {
            assert!(
                runtime
                    .update_fes_object(0, scene.objects_mut().unwrap(), id)
                    .unwrap_err()
                    .message
                    .contains("文件请求")
            );
            let ObjectState::Sound(sound) = &scene.objects().get(id).unwrap().state else {
                panic!()
            };
            assert!(sound.needs_load());
        }
    }
}

fn update(runtime: &mut Runtime<Reject>, scene: &mut Scene) -> Result<()> {
    for _ in 0..256 {
        if runtime.update_fes_scene(0, scene, 100)? {
            return Ok(());
        }
    }
    panic!("合成声音场景没有完成更新");
}

fn voice_runtime(budget: usize) -> Runtime<Reject> {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(8),
        Reject,
    );
    runtime.set_budget(budget, 10000);
    runtime.host_mut().sound_config_enabled = 1;
    runtime.host_mut().random = ScriptRandom::seeded(0);
    runtime.host_mut().sound_config = Some(SoundConfig {
        parts: vec![SoundPart {
            id: 20,
            system_voice: 1,
            children: vec![SoundPart {
                id: 21,
                system_voice: 0,
                children: vec![],
            }],
        }],
    });
    runtime
}

#[test]
fn sysvoiceex_shares_random_stream_and_runs_in_main_system_and_object_handlers() {
    let source = b"#DEFINE\nBASE obj[2]\n#START\nsysvoiceex\nsysvoiceex 4 ($0+=20),($1+=21)\nsysvoiceex 5 10 20 20\nsysvoiceex -1 999\nwait 100\n#_FES_SLEEP_\nsysvoiceex 6 20,\n#obj.PUSH_L\nsysvoiceex 7 @\n";
    for budget in [1, 4096] {
        let mut runtime = voice_runtime(budget);
        let mut scene = Scene::new(
            Program::from_text(source).unwrap(),
            runtime.host().handle_allocator(),
        )
        .unwrap();
        update(&mut runtime, &mut scene).unwrap();
        assert_eq!(runtime.numbers().shared[..6], [20, 21, 0, 0, -1, 20]);
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
        done = false;
        for _ in 0..256 {
            if let Some(handled) = runtime
                .dispatch_fes_event(
                    0,
                    &mut scene,
                    Some(&ObjectReference {
                        name: "obj".into(),
                        bank: 1,
                    }),
                    2,
                )
                .unwrap()
            {
                assert!(handled);
                done = true;
                break;
            }
        }
        assert!(done);
        assert_eq!(runtime.numbers().shared[6..], [0, 1]);
        assert_eq!(scene.deadline(), deadline);
        let mut expected = ScriptRandom::seeded(0);
        for _ in 0..4 {
            expected.next_integer();
        }
        assert_eq!(
            runtime.host_mut().random.next_integer(),
            expected.next_integer()
        );
    }
}

#[test]
fn sysvoiceex_disabled_mode_skips_candidates_and_missing_configuration_retries_before_side_effects()
{
    for budget in [1, 4096] {
        let mut runtime = voice_runtime(budget);
        runtime.host_mut().sound_config = None;
        let mut scene = Scene::new(
            Program::from_text(b"#START\nsysvoiceex ($0+=1)+3 ($1+=10)\nwait 99\n").unwrap(),
            runtime.host().handle_allocator(),
        )
        .unwrap();
        assert!(update(&mut runtime, &mut scene).is_err());
        assert_eq!(runtime.numbers().shared, [0; 8]);
        runtime.host_mut().sound_config = Some(SoundConfig::default());
        update(&mut runtime, &mut scene).unwrap();
        assert_eq!(runtime.numbers().shared[..5], [1, 10, 0, 0, 10]);
        let mut expected = ScriptRandom::seeded(0);
        expected.next_integer();
        assert_eq!(
            runtime.host_mut().random.next_integer(),
            expected.next_integer()
        );

        runtime.host_mut().sound_config_enabled = 0;
        runtime.host_mut().sound_config = None;
        runtime.host_mut().random = ScriptRandom::seeded(0);
        let mut scene = Scene::new(
            Program::from_text(b"#START\nsysvoiceex ($0+=1)+3 ($1+=10)\nwait 99\n").unwrap(),
            runtime.host().handle_allocator(),
        )
        .unwrap();
        update(&mut runtime, &mut scene).unwrap();
        assert_eq!(runtime.numbers().shared[..6], [2, 10, 0, 0, 10, -1]);
        assert_eq!(runtime.host_mut().random.next_integer(), 477480905);
    }
}
