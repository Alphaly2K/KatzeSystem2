use katze_system2::{
    Error, Result,
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
fn sound_load_routes_complete_records_and_consumes_only_finished_load_requests() {
    let (mut runtime, mut objects, id, trace) = setup();
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
            .contains("播放属性")
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
        ]
    );
    trace.borrow_mut().events.clear();
    runtime.update_fes_object(0, &mut objects, id).unwrap_err();
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
