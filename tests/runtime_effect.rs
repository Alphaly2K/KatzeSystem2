use katze_system2::{
    Result,
    expression::State,
    graphics::{Drawable, GraphicsFactory},
    kcs::Program,
    runtime::{BaseObject, EffectPlayer, EffectServices, KcsFactory, ProgramLoader, Runtime, kind},
    task::{Command, CommandBuffers},
};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

struct Loader(Program);

impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Load(Vec<u8>),
    Unload,
    Play(u32),
    Color(u32, [u8; 4]),
    Stop(u32),
}

#[derive(Default)]
struct Backend {
    events: Vec<Event>,
    next_instance: u32,
    active: BTreeSet<u32>,
    initial_positions: Vec<[f32; 3]>,
    locations: Vec<(u32, [f32; 3])>,
    rotations: Vec<(u32, [f32; 3])>,
    base_points: Vec<([f32; 3], bool)>,
    cameras: Vec<([f32; 16], [f32; 16])>,
}

struct Player(Rc<RefCell<Backend>>);

impl EffectPlayer for Player {
    fn load(&mut self, name: &[u8]) -> Result<bool> {
        self.0.borrow_mut().events.push(Event::Load(name.to_vec()));
        Ok(true)
    }

    fn unload(&mut self) {
        self.0.borrow_mut().events.push(Event::Unload);
    }

    fn play(&mut self, position: [f32; 3]) -> Option<u32> {
        let mut backend = self.0.borrow_mut();
        backend.initial_positions.push(position);
        let instance = backend.next_instance;
        backend.next_instance += 1;
        backend.active.insert(instance);
        backend.events.push(Event::Play(instance));
        Some(instance)
    }

    fn stop(&mut self, instance: u32) {
        let mut backend = self.0.borrow_mut();
        backend.active.remove(&instance);
        backend.events.push(Event::Stop(instance));
    }

    fn is_playing(&mut self, instance: u32) -> bool {
        self.0.borrow().active.contains(&instance)
    }

    fn set_color(&mut self, instance: u32, rgba: [u8; 4]) {
        self.0
            .borrow_mut()
            .events
            .push(Event::Color(instance, rgba));
    }

    fn set_location(&mut self, instance: u32, position: [f32; 3]) {
        self.0.borrow_mut().locations.push((instance, position));
    }

    fn set_rotation(&mut self, instance: u32, rotation: [f32; 3]) {
        self.0.borrow_mut().rotations.push((instance, rotation));
    }

    fn set_base_point(&mut self, point: [f32; 3], source: bool) {
        self.0.borrow_mut().base_points.push((point, source));
    }

    fn set_camera(&mut self, projection: [f32; 16], view: [f32; 16]) {
        self.0.borrow_mut().cameras.push((projection, view));
    }
}

impl EffectServices for Player {
    fn create_player(&mut self, _: u32) -> Result<Option<Box<dyn EffectPlayer>>> {
        Ok(Some(Box::new(Player(self.0.clone()))))
    }
}

type EffectRuntime = Runtime<GraphicsFactory<KcsFactory<Loader>>>;

fn runtime(calls: &[(u32, &[u32])]) -> Result<EffectRuntime> {
    let mut code = Vec::new();
    for &(index, args) in calls {
        for arg in args {
            code.extend([0x25, 3]);
            code.extend(arg.to_le_bytes());
        }
        code.extend([0x2b, 3]);
        code.extend(index.to_le_bytes());
        code.push(3);
        code.extend((args.len() as u32 * 4).to_le_bytes());
    }
    code.push(0x3b);
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory {
            fallback: KcsFactory::new(Loader(Program::from_code(
                &code,
                b"a%%.efk\0".to_vec(),
                64,
            )?)),
        },
    );
    runtime.set_budget(1, 1024);
    Ok(runtime)
}

fn command(runtime: &mut EffectRuntime, code: u32) -> Result<u32> {
    runtime.dispatch_command(
        0,
        Command {
            target: 1,
            sender: 0,
            code,
            data: [0; 3],
        },
    )
}

fn transform<const N: usize>(
    runtime: &mut EffectRuntime,
    target: u32,
    code: u32,
    source: u32,
    value: [f32; N],
) -> Result<()> {
    let mut bytes: Vec<_> = value.into_iter().flat_map(f32::to_le_bytes).collect();
    runtime.dispatch_command_with_buffers(
        0,
        Command {
            target,
            sender: 0,
            code,
            data: [source, 0, 0],
        },
        &mut CommandBuffers {
            data1: Some(&mut bytes),
            ..Default::default()
        },
    )?;
    Ok(())
}

#[test]
fn effect_transforms_separate_backend_coordinates_from_child_matrices() -> Result<()> {
    let backend = Rc::new(RefCell::new(Backend::default()));
    let mut runtime = runtime(&[(1014, &[1, 0]), (1016, &[1, 1])])?;
    runtime.host_mut().surface_size = [1280, 720];
    runtime.set_effect_services(Player(backend.clone()));
    let effect = runtime.create(0, kind::EFFEKSEER, b"", b"")?;
    let child = runtime.create(effect, kind::PLANE, b"", b"")?;
    assert!(
        runtime
            .drawables_mut()
            .insert(child, Drawable::new(vec![])?)?
    );
    transform(&mut runtime, child, 53, 0, [0.; 3])?;
    transform(&mut runtime, effect, 49, 1, [9., 4., 450.])?;
    assert_eq!(
        runtime
            .drawables()
            .get(child)
            .unwrap()
            .transform()
            .base_point(),
        [0., 0., 5000.]
    );
    transform(&mut runtime, effect, 49, 0, [5., 3., 400.])?;
    transform(&mut runtime, effect, 51, 1, [32., -16., 100.])?;
    transform(&mut runtime, effect, 55, 0, [1., 2., 3.])?;
    assert!(backend.borrow().rotations.is_empty());
    let child_transform = runtime.drawables().get(child).unwrap().transform();
    assert_eq!(child_transform.base_point(), [5., 3., 400.]);
    assert_eq!(child_transform.position(), [0.; 3]);
    assert_eq!(child_transform.rotation(), [1., 2., 3.]);

    runtime.create(0, kind::ORDINARY, b"caller", b"")?;
    runtime.update(0, 1)?;
    assert_eq!(backend.borrow().initial_positions, [[2., 1., 100.]]);
    transform(&mut runtime, effect, 51, 0, [-16., 8., 10.])?;
    transform(&mut runtime, effect, 55, 0, [7., 8., 9.])?;
    transform(&mut runtime, effect, 66, 0, [2., -0.5, 9.])?;
    let matrix = std::array::from_fn(|i| i as f32);
    transform(&mut runtime, effect, 57, 0, matrix)?;
    let child_transform = runtime.drawables().get(child).unwrap().transform();
    assert_eq!(child_transform.scale(), [2., -0.5, 9.]);
    assert_eq!(child_transform.matrix_override(), Some(&matrix));
    {
        let backend = backend.borrow();
        assert_eq!(
            backend.base_points,
            [([9., 4., 450.], true), ([5., 3., 400.], false)]
        );
        assert_eq!(backend.locations, [(0, [-1., -0.5, 10.])]);
        assert_eq!(backend.rotations, [(0, [7., 8., 9.])]);
        assert_eq!(backend.cameras.len(), 2);
        let (projection, view) = backend.cameras[1];
        assert_eq!(projection[0], 0.05);
        assert_eq!(projection[5].to_bits(), 0xbcb60b61);
        assert_eq!(projection[10], 1. / 299.);
        assert_eq!(view[14].to_bits(), 0x42c7ffcb);
        assert_eq!(view[13].to_bits(), 0x80000000);
    }
    backend.borrow_mut().active.remove(&0);
    runtime.update(1, 1)?;
    assert_eq!(
        backend.borrow().initial_positions,
        [[2., 1., 100.], [-1., -0.5, 10.]]
    );
    assert_eq!(backend.borrow().rotations.len(), 1);
    Ok(())
}

#[test]
fn effect_natives_play_restart_unload_and_stop() -> Result<()> {
    let backend = Rc::new(RefCell::new(Backend::default()));
    let mut runtime = runtime(&[
        (1014, &[0]),
        (1016, &[0]),
        (1076, &[0]),
        (1076, &[1]),
        (1016, &[1, 1]),
        (1014, &[1, 0]),
        (1016, &[1, 0]),
        (1076, &[1]),
        (1017, &[1]),
        (1076, &[1]),
        (1015, &[1]),
        (1076, &[1]),
        (1014, &[1, 0]),
        (1016, &[1, 0x8000_0000]),
    ])?;
    runtime.set_effect_services(Player(backend.clone()));
    assert_eq!(runtime.create(0, kind::EFFEKSEER, b"", b"")?, 1);
    let caller = runtime.create(0, kind::ORDINARY, b"caller", b"")?;
    runtime.update(0, 1)?;
    let vm = runtime.object(caller).unwrap().interpreter().unwrap();
    assert_eq!(
        vm.stack_bytes(),
        [0, 0, 1, 0, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        backend.borrow().events,
        [
            Event::Load(b"a%.efk".to_vec()),
            Event::Play(0),
            Event::Color(0, [255; 4]),
            Event::Stop(0),
            Event::Unload,
            Event::Load(b"a%.efk".to_vec()),
            Event::Play(1),
            Event::Color(1, [255; 4])
        ]
    );

    runtime.dispatch_command(
        0,
        Command {
            target: 1,
            sender: 0,
            code: 59,
            data: [0, 0x8172_6354, 0],
        },
    )?;
    backend.borrow_mut().active.remove(&1);
    runtime.update(1, 1)?;
    assert_eq!(command(&mut runtime, 582)?, 1);
    assert_eq!(
        backend.borrow().events[8..],
        [
            Event::Color(1, [0x54, 0x63, 0x72, 0x81]),
            Event::Stop(1),
            Event::Play(2),
            Event::Color(2, [0x54, 0x63, 0x72, 0x81])
        ]
    );
    command(&mut runtime, 579)?;
    assert_eq!(command(&mut runtime, 582)?, 0);
    assert_eq!(backend.borrow().events.last(), Some(&Event::Unload));
    assert!(backend.borrow().active.contains(&2));
    runtime.destroy(1)?;
    assert_eq!(backend.borrow().events.last(), Some(&Event::Stop(2)));
    Ok(())
}

#[test]
fn effect_query_diagnoses_unwritten_output() -> Result<()> {
    for receiver in [Some(kind::EFFEKSEER), Some(kind::IMAGE_IMPORT), None] {
        let target = if receiver.is_some() { 1 } else { 99 };
        let mut runtime = runtime(&[(1076, &[target])])?;
        if let Some(receiver) = receiver {
            assert_eq!(runtime.create(0, receiver, b"", b"")?, target);
        }
        runtime.create(0, kind::ORDINARY, b"caller", b"")?;
        let error = runtime.update(0, 1).unwrap_err();
        assert!(error.to_string().contains("效果播放查询"), "{error}");
    }
    Ok(())
}
