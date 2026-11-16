use katze_system2::{
    Error, Result,
    expression::State,
    fes::{self, ObjectDefinition},
    graphics::GraphicsFactory,
    kcs::{Host, Interpreter, ResourceServices},
    resource::ResourceMode,
    runtime::{
        BaseObject, ObjectFactory, RootObject, Runtime, TaskObject,
        fes::{ObjectState, Objects, ParticleState, Scene},
        kind,
    },
    task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock},
};
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
struct Trace {
    events: Vec<(u32, u32, [u32; 3], Vec<u8>)>,
    resolutions: Vec<(bool, Vec<u8>, Vec<u8>)>,
    fail: Option<u32>,
    playing: u32,
}
#[derive(Clone)]
struct Services(Rc<RefCell<Trace>>);
impl ResourceServices for Services {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        self.0
            .borrow_mut()
            .resolutions
            .push((false, category.to_vec(), name.to_vec()));
        Ok([category, b"/", name].concat())
    }
    fn resolve_file(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        self.0
            .borrow_mut()
            .resolutions
            .push((true, category.to_vec(), name.to_vec()));
        Ok([category, b"/", name].concat())
    }
    fn read(&mut self, _: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
}
struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂没有该类型"))
    }
}
impl ObjectFactory for Services {
    fn create(&mut self, id: u32, kind: u32, context: &[u8]) -> Result<Box<dyn TaskObject>> {
        if kind == kind::PARTICLE_3D {
            Ok(Box::new(Receiver(BaseObject::new(), self.0.clone())))
        } else {
            GraphicsFactory { fallback: Reject }.create(id, kind, context)
        }
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
        if command.code < 320 {
            return self.0.dispatch_command(kind, command, output, host, budget);
        }
        let mut trace = self.1.borrow_mut();
        trace.events.push((
            command.target,
            command.code,
            command.data,
            buffers.data1.as_deref().unwrap_or_default().to_vec(),
        ));
        if trace.fail == Some(command.code) {
            trace.fail = None;
            return Err(Error::new(0, "合成粒子命令失败"));
        }
        *output = if command.code == 324 {
            trace.playing
        } else {
            0
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

fn setup(budget: usize) -> (Runtime<Services>, Rc<RefCell<Trace>>) {
    let trace = Rc::new(RefCell::new(Trace::default()));
    let services = Services(trace.clone());
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(RootObject::new(0, 1)),
        State::new(8),
        services.clone(),
    );
    runtime.set_resources(services);
    runtime.set_budget(budget, 10000);
    (runtime, trace)
}
fn state(objects: &Objects, id: u32) -> &ParticleState {
    let ObjectState::Particle(state) = &objects.get(id).unwrap().state else {
        panic!()
    };
    state
}
fn send(
    objects: &mut Objects,
    interpreter: &mut fes::Interpreter,
    numbers: &mut State,
    bytes: &[u8],
) {
    assert!(
        objects
            .dispatch_object_command(
                interpreter,
                &fes::Line {
                    offset: 0,
                    bytes: bytes.to_vec()
                },
                numbers
            )
            .unwrap()
    );
}
fn update(runtime: &mut Runtime<Services>, scene: &mut Scene, now: u32) {
    for _ in 0..256 {
        if runtime.update_fes_scene(0, scene, now).unwrap() {
            return;
        }
    }
    panic!("合成场景没有完成本轮更新");
}

#[test]
fn parameters_evaluate_once_reset_unused_slots_and_keep_bank_order() {
    let (mut runtime, _) = setup(4096);
    let mut objects = Objects::new(runtime.host().handle_allocator()).unwrap();
    let ids = objects
        .define(&ObjectDefinition {
            kind: "PARTICLE",
            name: "pt".into(),
            banks: 2,
        })
        .unwrap();
    let mut interpreter =
        fes::Interpreter::new(fes::Program::from_text(b"#START\nwait 99\n").unwrap()).unwrap();
    objects.publish_bindings(&mut interpreter);
    for bytes in [
        b"pt init ($0+=1) effect tex ($0+=1) ($0+=1)".as_slice(),
        b"pt param ($0+=1) -16777217 17",
    ] {
        send(&mut objects, &mut interpreter, runtime.numbers_mut(), bytes);
    }
    assert_eq!(runtime.numbers().shared[0], 4);
    for id in &ids {
        let p = state(&objects, *id);
        assert_eq!((p.mode, p.count, p.depth), (1, 2, 3));
        assert_eq!(p.parameters[..4], [4., -16777216., 17., 0.]);
        assert!(p.needs_load() && p.is_dirty());
    }
    send(
        &mut objects,
        &mut interpreter,
        runtime.numbers_mut(),
        b"pt[1] param 9",
    );
    assert_eq!(state(&objects, ids[0]).parameters[1], -16777216.);
    assert_eq!(state(&objects, ids[1]).parameters[..4], [9., 0., 0., 0.]);
    let line = format!(
        "pt[1] param {} ($1+=1)",
        (0..32).map(|i| i.to_string()).collect::<Vec<_>>().join(" ")
    );
    send(
        &mut objects,
        &mut interpreter,
        runtime.numbers_mut(),
        line.as_bytes(),
    );
    assert_eq!(runtime.numbers().shared[1], 0);
    assert_eq!(state(&objects, ids[1]).parameters[31], 31.);
}

#[test]
fn media_retries_keep_completed_loads_and_release_duplicate_tasks() {
    let (mut runtime, trace) = setup(4096);
    let mut objects = Objects::new(runtime.host().handle_allocator()).unwrap();
    let definition = ObjectDefinition {
        kind: "PARTICLE",
        name: "pt".into(),
        banks: 1,
    };
    let id = objects.define(&definition).unwrap()[0];
    objects.define(&definition).unwrap();
    let mut interpreter =
        fes::Interpreter::new(fes::Program::from_text(b"#START\nwait 99\n").unwrap()).unwrap();
    objects.publish_bindings(&mut interpreter);
    send(
        &mut objects,
        &mut interpreter,
        runtime.numbers_mut(),
        b"pt init 1 effect tex 4 -42",
    );
    send(
        &mut objects,
        &mut interpreter,
        runtime.numbers_mut(),
        b"pt param 7 8",
    );
    send(
        &mut objects,
        &mut interpreter,
        runtime.numbers_mut(),
        b"pt start",
    );
    trace.borrow_mut().fail = Some(325);
    assert!(runtime.update_fes_record(0, &mut objects, 0).is_err());
    assert!(!state(&objects, id).needs_load());
    assert!(state(&objects, id).is_dirty());
    trace.borrow_mut().playing = 1;
    runtime.update_fes_record(0, &mut objects, 0).unwrap();
    runtime.update_fes_record(0, &mut objects, 1).unwrap();
    assert!(state(&objects, id).playing());
    let events = trace.borrow().events.clone();
    assert_eq!(
        events.iter().map(|e| e.1).collect::<Vec<_>>(),
        [320, 325, 325, 326, 321, 324, 324]
    );
    let record = &events[0].3;
    assert_eq!(record.len(), 0x290);
    assert_eq!(
        u32::from_le_bytes(record[0x104..0x108].try_into().unwrap()),
        4
    );
    assert_eq!(
        f32::from_le_bytes(record[0x108..0x10c].try_into().unwrap()),
        -42.
    );
    assert!(record[0x10c..].iter().all(|&v| v == 0));
    let tasks: Vec<_> = objects
        .media_records()
        .iter()
        .map(|r| r.task_id().unwrap())
        .collect();
    assert_ne!(tasks[0], tasks[1]);
    runtime.remove_fes_object(0, &mut objects, id).unwrap();
    for task in tasks {
        assert_ne!(
            runtime
                .object_mut(task)
                .unwrap()
                .control()
                .deletion_requested,
            0
        );
    }
}

#[test]
fn scene_start_stop_and_wait_use_cached_particle_playback_with_small_budgets() {
    for budget in [1, 4096] {
        let (mut runtime, trace) = setup(budget);
        trace.borrow_mut().playing = 1;
        let mut scene=Scene::new(fes::Program::from_text(b"#DEFINE\nPARTICLE pt\n#START\npt init 0 effect tex 5 123\npt param 1 2\npt start\nwait frame 1\npt ismot 0\nwait pt\n$1+=1\npt start\npt stop\nwait frame 1\npt ismot 2\nwait 1000\n").unwrap(),runtime.host().handle_allocator()).unwrap();
        update(&mut runtime, &mut scene, 10);
        update(&mut runtime, &mut scene, 11);
        assert_eq!(runtime.numbers().shared[..3], [1, 0, 0]);
        trace.borrow_mut().playing = 0;
        update(&mut runtime, &mut scene, 12);
        update(&mut runtime, &mut scene, 13);
        update(&mut runtime, &mut scene, 14);
        assert_eq!(runtime.numbers().shared[..3], [1, 1, 0]);
        let commands: Vec<_> = trace.borrow().events.iter().map(|e| e.1).collect();
        assert!(commands.windows(2).any(|codes| codes == [321, 322]));
        let id = scene.objects().lookup("pt", 0).unwrap();
        assert!(!state(scene.objects(), id).playing());
    }
}
