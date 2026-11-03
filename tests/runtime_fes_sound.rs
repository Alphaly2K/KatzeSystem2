use katze_system2::{
    Error, HandleAllocator, Result,
    audio::{SoundConfig, SoundPart},
    expression::State,
    fes::{
        self, ObjectAttributes, ObjectDefinition, ObjectInitialization, ObjectReference, Program,
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

fn sound(objects: &Objects, id: u32) -> &SoundState {
    let ObjectState::Sound(state) = &objects.get(id).unwrap().state else {
        panic!()
    };
    state
}

fn object_command(
    objects: &mut Objects,
    interpreter: &mut fes::Interpreter,
    numbers: &mut State,
    bytes: &[u8],
) -> Result<bool> {
    objects.dispatch_object_command(
        interpreter,
        &fes::Line {
            offset: 100,
            bytes: bytes.to_vec(),
        },
        numbers,
    )
}

#[test]
fn sound_load_repeats_mode_evaluation_and_channel_updates_stop_at_missing_banks() {
    let mut objects = Objects::new(HandleAllocator::default()).unwrap();
    let ids = define(&mut objects, "SOUND", "sound", 3);
    objects.get_mut(ids[1]).unwrap().state = ObjectState::CImage(Box::default());
    let child = define(&mut objects, "CIMAGE", "child", 1)[0];
    objects.set_parent(child, Some(ids[2])).unwrap();
    let mut interpreter =
        fes::Interpreter::new(Program::from_text(b"#START\nwait 9\n").unwrap()).unwrap();
    objects.publish_bindings(&mut interpreter);
    let mut numbers = State::new(4);
    object_command(
        &mut objects,
        &mut interpreter,
        &mut numbers,
        b"sound load raw ($0+=1) ($0+=10) ($0+=1000)",
    )
    .unwrap();
    assert_eq!(numbers.shared[0], 25);
    assert_eq!(sound(&objects, ids[0]).load_parameters, [2, 12]);
    assert_eq!(sound(&objects, ids[2]).load_parameters, [15, 25]);
    assert!(sound(&objects, ids[2]).needs_load());
    assert!(!sound(&objects, ids[2]).is_dirty());
    initialize(
        &mut objects,
        0,
        ObjectAttributes {
            file: Some(b"declared".to_vec()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sound(&objects, ids[0]).load_parameters, [2, 12]);
    object_command(
        &mut objects,
        &mut interpreter,
        &mut numbers,
        b"sound channel ($1+=1) ($1+=100)",
    )
    .unwrap();
    assert_eq!(numbers.shared[1], 1);
    assert_eq!(sound(&objects, ids[0]).channel, 1);
    assert_eq!(sound(&objects, ids[2]).channel, 1);
    let ObjectState::CImage(state) = &objects.get(child).unwrap().state else {
        panic!()
    };
    assert!(state.updates.is_dirty());
    objects.get_mut(child).unwrap().state = ObjectState::CImage(Box::default());
    for command in [b"sound channel 1".as_slice(), b"sound stype"] {
        object_command(&mut objects, &mut interpreter, &mut numbers, command).unwrap();
    }
    let ObjectState::CImage(state) = &objects.get(child).unwrap().state else {
        panic!()
    };
    assert!(!state.updates.is_dirty());
    objects.remove(ids[1]).unwrap();
    objects.publish_bindings(&mut interpreter);
    object_command(
        &mut objects,
        &mut interpreter,
        &mut numbers,
        b"sound load raw ($2+=1) ($2+=10)",
    )
    .unwrap();
    assert_eq!(numbers.shared[2], 12);
    assert_eq!(sound(&objects, ids[0]).load_parameters, [2, 12]);
    assert_eq!(sound(&objects, ids[2]).load_parameters, [15, 25]);
}

#[test]
fn sound_play_skips_arrays_and_keeps_the_request_and_completed_arguments_after_errors() {
    let mut objects = Objects::new(HandleAllocator::default()).unwrap();
    let ids = define(&mut objects, "SOUND", "sound", 2);
    let child = define(&mut objects, "CIMAGE", "child", 1)[0];
    objects.set_parent(child, Some(ids[0])).unwrap();
    let ObjectState::Sound(state) = &mut objects.get_mut(ids[0]).unwrap().state else {
        panic!()
    };
    state.play_parameters = [7, 8, 9];
    let mut interpreter =
        fes::Interpreter::new(Program::from_text(b"#START\nwait 9\n").unwrap()).unwrap();
    objects.publish_bindings(&mut interpreter);
    let mut numbers = State::new(4);
    object_command(
        &mut objects,
        &mut interpreter,
        &mut numbers,
        b"sound play ($0+=1) ($0+=10)",
    )
    .unwrap();
    assert_eq!(numbers.shared[0], 0);
    assert_eq!(sound(&objects, ids[0]).action_request(), None);
    object_command(
        &mut objects,
        &mut interpreter,
        &mut numbers,
        b"sound[0] play",
    )
    .unwrap();
    assert_eq!(sound(&objects, ids[0]).play_parameters, [7, 1, 0]);
    assert_eq!(sound(&objects, ids[0]).action_request(), Some(1));
    assert!(!sound(&objects, ids[0]).play_value_changed());
    assert!(sound(&objects, ids[0]).is_dirty());
    objects.get_mut(child).unwrap().state = ObjectState::CImage(Box::default());
    numbers.shared[1] = 4;
    assert!(
        object_command(
            &mut objects,
            &mut interpreter,
            &mut numbers,
            b"sound[0] play ($0+=1) ($1/=0) ($2+=1)",
        )
        .is_err()
    );
    assert_eq!(numbers.shared, [1, 4, 0, 0]);
    assert_eq!(sound(&objects, ids[0]).play_parameters, [1, 1, 0]);
    assert!(sound(&objects, ids[0]).play_value_changed());
    let ObjectState::CImage(state) = &objects.get(child).unwrap().state else {
        panic!()
    };
    assert!(state.updates.is_dirty());
    let mut runtime = voice_runtime(1);
    assert!(
        runtime
            .update_fes_object(0, &mut objects, ids[0])
            .unwrap_err()
            .message
            .contains("声音任务绑定失效")
    );
    assert_eq!(sound(&objects, ids[0]).action_request(), Some(1));
    let single = define(&mut objects, "SOUND", "single", 1)[0];
    objects.publish_bindings(&mut interpreter);
    object_command(
        &mut objects,
        &mut interpreter,
        &mut numbers,
        b"single play ($3+=1)",
    )
    .unwrap();
    assert_eq!(numbers.shared[3], 1);
    assert_eq!(sound(&objects, single).action_request(), Some(1));
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
            enabled: Some(0),
            default_events: Some(b"alias".to_vec()),
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
        assert_eq!(state.enabled, 1);
        assert!(state.event_alias.is_empty());
        assert!(!state.is_dirty());
        assert!(!state.needs_load());
    }
    let mut interpreter =
        fes::Interpreter::new(Program::from_text(b"#START\nwait 9\n").unwrap()).unwrap();
    objects.publish_bindings(&mut interpreter);
    let mut numbers = State::new(0);
    for bytes in [b"sound enable 0".as_slice(), b"sound enable -3"] {
        object_command(&mut objects, &mut interpreter, &mut numbers, bytes).unwrap();
        for &id in &sounds {
            assert_eq!(sound(&objects, id).enabled, 1);
            assert!(!sound(&objects, id).is_dirty());
        }
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
                    .contains("声音资源")
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
            name: b"parent".to_vec(),
            id: 20,
            volume: 83,
            system_voice: 1,
            children: vec![SoundPart {
                name: b"child".to_vec(),
                id: 21,
                volume: 37,
                system_voice: 0,
                children: vec![],
                ..SoundPart::default()
            }],
            ..SoundPart::default()
        }],
        ..SoundConfig::default()
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

#[test]
fn named_volume_queries_retry_missing_values_and_keep_names_and_extra_arguments_as_text() {
    for budget in [1, 4096] {
        let mut runtime = voice_runtime(budget);
        let config = runtime.host_mut().sound_config.take().unwrap();
        runtime.numbers_mut().strings.insert(0, b"PaReNt".to_vec());
        let mut scene = Scene::new(
            Program::from_text(b"#DEFINE\nBASE obj\n#START\ngetsndconfex ($0+=1)+3 volume $str0; ($1+=100)\ngetsndconfex 5 volume ($1+=100)\ngetsndconfex 6 volume master\nwait 77\n#_FES_SLEEP_\ngetsndconfex 6 volume CHILD ($1+=100)\n#obj.PUSH_L\ngetsndconfex 7 volume PARENT ($1+=100)\n").unwrap(),
            runtime.host().handle_allocator(),
        ).unwrap();
        assert!(update(&mut runtime, &mut scene).is_err());
        assert_eq!(runtime.numbers().shared, [0; 8]);
        runtime.host_mut().sound_config = Some(config);
        assert!(
            update(&mut runtime, &mut scene)
                .unwrap_err()
                .message
                .contains("主音量")
        );
        assert_eq!(runtime.numbers().shared, [1, 0, 0, 0, 83, 100, 0, 0]);
        runtime
            .host_mut()
            .sound_config
            .as_mut()
            .unwrap()
            .master_volume = Some(63);
        update(&mut runtime, &mut scene).unwrap();
        assert_eq!(runtime.numbers().shared, [1, 0, 0, 0, 83, 100, 63, 0]);
        for _ in 0..256 {
            if runtime
                .dispatch_fes_system_block(0, &mut scene, "_FES_SLEEP_")
                .unwrap()
            {
                break;
            }
        }
        let target = ObjectReference {
            name: "obj".into(),
            bank: 0,
        };
        let mut done = false;
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
        assert_eq!(runtime.numbers().shared, [1, 0, 0, 0, 83, 100, 37, 83]);
        assert_eq!(scene.deadline(), Some(177));
        assert_eq!(
            runtime.host_mut().random.next_integer(),
            ScriptRandom::seeded(0).next_integer()
        );
    }
}

#[test]
fn sound_config_id_queries_use_separate_path_arguments_and_keep_the_random_stream() {
    let source = b"#DEFINE\nBASE obj\n#START\ngetsndconfex\ngetsndconfex ($0+=1)+3\ngetsndconfex ($0+=1)+3 volume_id ($1+=20) ($2+=21)\ngetsndconfex -1 volume_id ($3+=20)\nwait 99\n#_FES_SLEEP_\ngetsndconfex 6 volume_id 20 999\n#obj.PUSH_L\ngetsndconfex 7 sysplay_id 20 21\n";
    for budget in [1, 4096] {
        let mut runtime = voice_runtime(budget);
        runtime.host_mut().sound_config_enabled = 0;
        let config = runtime.host_mut().sound_config.take().unwrap();
        let mut scene = Scene::new(
            Program::from_text(source).unwrap(),
            runtime.host().handle_allocator(),
        )
        .unwrap();
        assert!(update(&mut runtime, &mut scene).is_err());
        assert_eq!(runtime.numbers().shared[0], 1);
        assert_eq!(runtime.numbers().shared[1..], [0; 7]);
        runtime.host_mut().sound_config = Some(config);
        update(&mut runtime, &mut scene).unwrap();
        assert_eq!(runtime.numbers().shared[..6], [2, 20, 21, 20, 0, 37]);
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
                        bank: 0,
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
        assert_eq!(runtime.numbers().shared[6..], [100, 0]);
        assert_eq!(runtime.host_mut().random.next_integer(), 477480905);
    }
}
