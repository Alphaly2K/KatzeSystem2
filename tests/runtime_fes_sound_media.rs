use katze_system2::{
    Error, Result,
    audio::{SoundConfig, SoundPart},
    expression::State,
    fes::{ObjectAttributes, ObjectDefinition, ObjectInitialization, ObjectReference},
    kcs::{Host, Interpreter, ResourceServices},
    resource::ResourceMode,
    runtime::{
        BaseObject, ObjectFactory, RootObject, Runtime, TaskObject,
        fes::{ObjectState, Objects},
        kind,
    },
    task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock},
};
use std::{cell::RefCell, rc::Rc};

#[derive(Debug, PartialEq)]
enum Event {
    Resolve(Vec<u8>),
    Command(u32, [u32; 3], Vec<u8>),
}

#[derive(Default)]
struct Trace {
    events: Vec<Event>,
    fail: Option<u32>,
    playing: u32,
}

#[derive(Clone)]
struct Services(Rc<RefCell<Trace>>);

impl ResourceServices for Services {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(category, b"sound");
        self.0
            .borrow_mut()
            .events
            .push(Event::Resolve(name.to_vec()));
        Ok([b"audio/".as_slice(), name].concat())
    }

    fn read(&mut self, _: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
        panic!("FES 只补全名称，声音任务负责读取资源")
    }
}

impl ObjectFactory for Services {
    fn create(&mut self, _: u32, kind: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        assert_eq!(kind, kind::SOUND);
        Ok(Box::new(Receiver(BaseObject::new(), self.0.clone())))
    }
}

struct Receiver(BaseObject, Rc<RefCell<Trace>>);

impl TaskObject for Receiver {
    fn control(&self) -> &Control {
        self.0.control()
    }
    fn clock(&self) -> &TaskClock {
        self.0.clock()
    }
    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        self.0.state()
    }
    fn interpreter(&self) -> Option<&Interpreter> {
        None
    }
    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        None
    }
    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.0.set_creation_context(context)
    }
    fn update_body(&mut self, host: &mut dyn Host, increment: u32, budget: usize) -> Result<bool> {
        self.0.update_body(host, increment, budget)
    }
    fn dispatch_command(
        &mut self,
        kind: u32,
        command: &Command,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        self.dispatch_command_with_buffers(
            kind,
            command,
            &mut CommandBuffers::default(),
            output,
            host,
            budget,
        )
    }
    fn dispatch_command_with_buffers(
        &mut self,
        kind: u32,
        command: &Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        if command.code < 180 {
            return self.0.dispatch_command(kind, command, output, host, budget);
        }
        let mut trace = self.1.borrow_mut();
        trace.events.push(Event::Command(
            command.code,
            command.data,
            buffers.data1.as_deref().unwrap_or_default().to_vec(),
        ));
        if trace.fail == Some(command.code) {
            trace.fail = None;
            return Err(Error::new(0, "合成声音装载失败"));
        }
        *output = match command.code {
            210 => 23,
            206 => 31,
            201 => 513,
            186 => trace.playing,
            _ => 0,
        };
        Ok(CommandOutcome::finished(false))
    }
    fn enqueue_message(&mut self, message: [u32; 4]) -> Result<()> {
        self.0.enqueue_message(message)
    }
    fn dispatch_message(
        &mut self,
        message: &mut [u32; 4],
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<bool> {
        self.0.dispatch_message(message, output, host, budget)
    }
    fn request_delete(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool> {
        self.0.request_delete(host, budget)
    }
}

fn setup() -> (Runtime<Services>, Objects, u32, Rc<RefCell<Trace>>) {
    let trace = Rc::new(RefCell::new(Trace::default()));
    let services = Services(trace.clone());
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(RootObject::new(0, 1)),
        State::new(0),
        services.clone(),
    );
    runtime.set_resources(services);
    assert_eq!(runtime.create(0, kind::SOUND, b"sound", b"").unwrap(), 1);
    let mut objects = Objects::new(runtime.host().handle_allocator()).unwrap();
    let id = objects
        .define(&ObjectDefinition {
            kind: "SOUND",
            name: "sound".into(),
            banks: 1,
        })
        .unwrap()[0];
    (runtime, objects, id, trace)
}

fn apply(objects: &mut Objects, attributes: ObjectAttributes) {
    objects
        .initialize(
            &ObjectInitialization {
                target: ObjectReference {
                    name: "sound".into(),
                    bank: 0,
                },
                attributes,
            },
            |_| panic!("声音初始化不查询图像"),
        )
        .unwrap();
}

fn state(objects: &Objects, id: u32) -> &katze_system2::fes::SoundState {
    let ObjectState::Sound(sound) = &objects.get(id).unwrap().state else {
        panic!()
    };
    sound
}

fn record(channel: u32, group: u32) -> Vec<u8> {
    let mut bytes = vec![0; 0x124];
    bytes[..4].copy_from_slice(&channel.to_le_bytes());
    bytes[4..8].copy_from_slice(&group.to_le_bytes());
    bytes[12..22].copy_from_slice(b"audio/tone");
    bytes[0x110..0x114].copy_from_slice(&1u32.to_le_bytes());
    bytes
}

#[test]
fn play_policy_stops_the_prior_channel_before_properties_and_runs_the_own_handler() {
    use katze_system2::{fes::Program, runtime::fes::Scene};
    for budget in [1, 4096] {
        for (handler, expected, transferred, repeat) in [
            ("", 2, false, false),
            ("wait 3\n$0=999\n", 2, false, false),
            ("call SUB\n$0=$0*10+5\n", 245, false, false),
            ("next DEST\n$0=999\n", 2, true, false),
            ("@ play 9\n", 2, false, true),
        ] {
            let (mut runtime, _, _, trace) = setup();
            runtime.numbers_mut().shared.resize(4, 0);
            runtime.set_budget(budget, 10_000);
            runtime.host_mut().system_time_ms = 12345;
            runtime.host_mut().sound_config_enabled = 1;
            runtime.host_mut().sound_config = Some(SoundConfig {
                master_volume: Some(47),
                parts: vec![SoundPart {
                    file_prefix: b"tone".to_vec(),
                    volume: 65,
                    play_enabled: 0,
                    system_voice: 1,
                    mute: 0,
                    ..Default::default()
                }],
                ..Default::default()
            });
            trace.borrow_mut().playing = 1;
            let source = format!(
                "#DEFINE\nSOUND sound\n#OBJECT CHANNEL STYPE ENABLE DEFAULT\nsound 5 1 0 alias\n#START\nsound play 0\nwait 99\n$3+=1\n#sound.PLAY\n$0=$0*10+2\n{handler}#SUB\n$0=$0*10+4\n#DEST\n$0=$0*10+3\nwait 99\n"
            );
            let mut scene = Scene::new(
                Program::from_text(source.as_bytes()).unwrap(),
                runtime.host().handle_allocator(),
            )
            .unwrap();
            assert!(scene.prepare(runtime.numbers_mut(), 4096).unwrap());
            let objects = scene.objects_mut().unwrap();
            objects.set_sound_task(1);
            let id = objects.lookup("sound", 0).unwrap();
            let ObjectState::Sound(sound) = &mut objects.get_mut(id).unwrap().state else {
                panic!()
            };
            sound.file = b"tone".to_vec();
            let mut done = false;
            for _ in 0..128 {
                if runtime.update_fes_scene(0, &mut scene, 10).unwrap() {
                    done = true;
                    break;
                }
            }
            assert!(done);
            assert_eq!(runtime.numbers().shared, [expected, 0, 0, 0]);
            let mut expected_events = vec![
                Event::Command(186, [5, 0, 0], vec![]),
                Event::Command(185, [5, 0, 0], vec![]),
                Event::Command(187, [5, 30, 0], vec![]),
                Event::Command(190, [5, 0, 0], vec![]),
                Event::Command(183, [5, 0, 0], vec![]),
            ];
            if repeat {
                expected_events.push(Event::Command(219, [5, 9, 0], vec![]));
            }
            assert_eq!(trace.borrow().events, expected_events);
            let sound = state(scene.objects(), id);
            assert_eq!(sound.started_tick(), 12345);
            assert_eq!(sound.action_request(), None);
            assert!(!sound.play_value_changed());
            assert_eq!(sound.is_dirty(), repeat);
            assert_eq!(scene.deadline(), if transferred { None } else { Some(109) });
            assert!(scene.interpreter().event_context().is_none());
            if transferred {
                done = false;
                for _ in 0..128 {
                    if runtime.update_fes_scene(0, &mut scene, 10).unwrap() {
                        done = true;
                        break;
                    }
                }
                assert!(done);
                assert_eq!(runtime.numbers().shared[0], 23);
                assert_eq!(runtime.numbers().shared[3], 0);
            }
        }
    }
}

#[test]
fn play_handler_retry_preserves_completed_playback_and_script_instructions() {
    use katze_system2::{fes::Program, runtime::fes::Scene};
    for budget in [1, 4096] {
        let (mut runtime, _, _, trace) = setup();
        runtime.numbers_mut().shared.resize(4, 0);
        runtime.numbers_mut().shared[1] = 5;
        runtime.set_budget(budget, 10_000);
        runtime.host_mut().sound_config_enabled = 1;
        runtime.host_mut().sound_config = Some(SoundConfig {
            master_volume: Some(100),
            ..Default::default()
        });
        let mut scene = Scene::new(
            Program::from_text(b"#DEFINE\nSOUND sound\n#OBJECT CHANNEL\nsound 5\n#START\nsound play 1\nwait 99\n#sound.PLAY\n$0+=1\n@ channel ($1/=$3)\n$2+=1\n").unwrap(),
            runtime.host().handle_allocator(),
        ).unwrap();
        assert!(scene.prepare(runtime.numbers_mut(), 4096).unwrap());
        scene.objects_mut().unwrap().set_sound_task(1);
        let id = scene.objects().lookup("sound", 0).unwrap();
        let mut failed = false;
        for _ in 0..128 {
            match runtime.update_fes_scene(0, &mut scene, 10) {
                Err(_) => {
                    failed = true;
                    break;
                }
                Ok(false) => (),
                Ok(true) => panic!("处理器应在除零处返回诊断"),
            }
        }
        assert!(failed);
        assert_eq!(runtime.numbers().shared[0], 1);
        assert_eq!(state(scene.objects(), id).action_request(), Some(1));
        runtime.numbers_mut().shared[3] = 1;
        let mut done = false;
        for _ in 0..128 {
            if runtime.update_fes_scene(0, &mut scene, 10).unwrap() {
                done = true;
                break;
            }
        }
        assert!(done);
        assert_eq!(runtime.numbers().shared, [1, 5, 1, 1]);
        assert_eq!(
            trace.borrow().events,
            [Event::Command(183, [5, 1, 0], vec![])]
        );
        assert_eq!(state(scene.objects(), id).action_request(), None);
        assert!(scene.interpreter().event_context().is_none());
    }
}

#[test]
fn sound_load_routes_complete_records_and_consumes_only_finished_load_requests() {
    let (mut runtime, mut objects, id, trace) = setup();
    runtime.host_mut().sound_config_enabled = 1;
    runtime.host_mut().sound_config = Some(SoundConfig {
        master_volume: Some(47),
        parts: vec![SoundPart {
            file_prefix: b"tone".to_vec(),
            volume: 65,
            mute: 0,
            ..Default::default()
        }],
        ..Default::default()
    });
    trace.borrow_mut().fail = Some(190);
    apply(
        &mut objects,
        ObjectAttributes {
            file: Some(b"tone".to_vec()),
            channel: Some(5),
            ..Default::default()
        },
    );
    assert!(
        runtime
            .update_fes_object(0, &mut objects, id)
            .unwrap_err()
            .message
            .contains("合成声音装载失败")
    );
    assert_eq!(objects.sound_task(), Some(1));
    assert_eq!(state(&objects, id).channel, 5);
    assert_eq!(state(&objects, id).channel_result(), 513);
    assert!(!state(&objects, id).needs_load());
    assert_eq!(
        trace.borrow().events,
        [
            Event::Resolve(b"tone".to_vec()),
            Event::Command(182, [5, 0, 0], vec![]),
            Event::Command(180, [0; 3], record(5, 0)),
            Event::Command(201, [5, 0, 0], vec![]),
            Event::Command(187, [5, 30, 0], vec![]),
            Event::Command(190, [5, 0, 0], vec![]),
        ]
    );
    trace.borrow_mut().events.clear();
    runtime.update_fes_object(0, &mut objects, id).unwrap();
    assert_eq!(
        trace.borrow().events,
        [
            Event::Command(187, [5, 30, 0], vec![]),
            Event::Command(190, [5, 0, 0], vec![]),
        ]
    );
    trace.borrow_mut().events.clear();
    runtime.update_fes_object(0, &mut objects, id).unwrap();
    assert!(trace.borrow().events.is_empty());
    apply(
        &mut objects,
        ObjectAttributes {
            file: Some(Vec::new()),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, id).unwrap();
    assert_eq!(
        trace.borrow().events,
        [Event::Command(182, [5, 0, 0], vec![])]
    );
    assert!(!state(&objects, id).needs_load());
    assert!(!state(&objects, id).is_dirty());
    assert_eq!(state(&objects, id).channel_result(), 513);
}

#[test]
fn sound_properties_combine_config_in_order_and_keep_the_global_refresh_flag() {
    let (mut runtime, mut objects, id, trace) = setup();
    runtime.host_mut().sound_config_enabled = 1;
    let ObjectState::Sound(sound) = &mut objects.get_mut(id).unwrap().state else {
        unreachable!()
    };
    sound.file = b"tone".to_vec();
    sound.volume = 131;
    sound.pan = -43;
    sound
        .apply(&ObjectAttributes {
            channel: Some(5),
            ..Default::default()
        })
        .unwrap();
    assert!(runtime.update_fes_object(0, &mut objects, id).is_err());
    assert!(state(&objects, id).is_dirty());
    assert!(trace.borrow().events.is_empty());
    runtime.host_mut().sound_config = Some(SoundConfig {
        master_volume: Some(47),
        parts: vec![
            SoundPart {
                file_prefix: b"tone".to_vec(),
                volume: 65,
                mute: -1,
                ..Default::default()
            },
            SoundPart {
                name: b"group".to_vec(),
                volume: 37,
                ..Default::default()
            },
            SoundPart {
                name: b"master".to_vec(),
                volume: 83,
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    runtime.host_mut().sound_settings_dirty = 2;
    for (file, group, object_type, master, mute, expected) in [
        (b"tone".as_slice(), b"group".as_slice(), 0, 47, 0, 14),
        (b"tone", b"master", 0, 47, -1, 32),
        (b"missing", b"group", 0, 47, 0, 0),
        (b"missing", b"group", 1, 47, 0, 61),
        (b"tone", b"group", -1, -47, 0, -61),
        (b"tone", b"group", 0, 47, 2, 0),
        (b"tone", b"group", 0, i32::MAX, 0, 18_285_822),
    ] {
        let config = runtime.host_mut().sound_config.as_mut().unwrap();
        config.master_volume = Some(master);
        config.master_mute = mute;
        let ObjectState::Sound(sound) = &mut objects.get_mut(id).unwrap().state else {
            unreachable!()
        };
        sound.file = file.to_vec();
        sound.group = group.to_vec();
        sound.object_type = object_type;
        trace.borrow_mut().events.clear();
        runtime.update_fes_object(0, &mut objects, id).unwrap();
        assert_eq!(
            trace.borrow().events,
            [
                Event::Command(187, [5, expected as u32, 0], vec![]),
                Event::Command(190, [5, (-43i32) as u32, 0], vec![]),
            ]
        );
        assert!(!state(&objects, id).is_dirty());
        assert_eq!(runtime.host().sound_settings_dirty, 2);
    }
    runtime.host_mut().sound_settings_dirty = 0;
    trace.borrow_mut().events.clear();
    runtime.update_fes_object(0, &mut objects, id).unwrap();
    assert!(trace.borrow().events.is_empty());
    let ObjectState::Sound(sound) = &mut objects.get_mut(id).unwrap().state else {
        unreachable!()
    };
    sound.file.clear();
    sound
        .apply(&ObjectAttributes {
            channel: Some(-1),
            ..Default::default()
        })
        .unwrap();
    runtime.host_mut().sound_config = None;
    runtime.update_fes_object(0, &mut objects, id).unwrap();
    assert!(!state(&objects, id).is_dirty());
    assert!(trace.borrow().events.is_empty());
}

#[test]
fn grouped_load_retries_after_query_failure_with_the_already_assigned_channel() {
    let (mut runtime, mut objects, id, trace) = setup();
    objects.set_sound_task(1);
    apply(
        &mut objects,
        ObjectAttributes {
            file: Some(b"tone".to_vec()),
            group: Some(b"music".to_vec()),
            ..Default::default()
        },
    );
    trace.borrow_mut().fail = Some(201);
    assert_eq!(
        runtime
            .update_fes_object(0, &mut objects, id)
            .unwrap_err()
            .message,
        "合成声音装载失败"
    );
    assert!(state(&objects, id).needs_load());
    assert_eq!(state(&objects, id).channel, 31);
    assert_eq!(state(&objects, id).channel_result(), 0);
    assert_eq!(
        trace.borrow().events,
        [
            Event::Resolve(b"tone".to_vec()),
            Event::Command(210, [0; 3], b"music\0".to_vec()),
            Event::Command(206, [0; 3], record(u32::MAX, 23)),
            Event::Command(201, [31, 0, 0], vec![]),
        ]
    );
    trace.borrow_mut().events.clear();
    runtime.update_fes_object(0, &mut objects, id).unwrap_err();
    assert!(!state(&objects, id).needs_load());
    assert_eq!(state(&objects, id).channel_result(), 513);
    assert_eq!(
        trace.borrow().events,
        [
            Event::Resolve(b"tone".to_vec()),
            Event::Command(182, [31, 0, 0], vec![]),
            Event::Command(210, [0; 3], b"music\0".to_vec()),
            Event::Command(206, [0; 3], record(31, 23)),
            Event::Command(201, [31, 0, 0], vec![]),
        ]
    );
}
