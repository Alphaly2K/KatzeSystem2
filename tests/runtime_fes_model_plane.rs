use katze_system2::{
    Error, Result,
    expression::State,
    fes::Program,
    graphics::{Projection, RenderTarget, SoftwareBackend, Transform},
    host::{
        graphics::{GraphicsBackend, Surface, TextureFilter},
        model::{ModelFrame, ModelPlayer, ModelSelection, ModelServices},
        resources::{ResourceMode, ResourceServices},
    },
    runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, fes::Scene, kind},
};
use std::{cell::RefCell, rc::Rc, sync::Arc};

#[derive(Debug, PartialEq)]
enum Event {
    Open(Vec<u8>),
    ReadModel,
    Options(Vec<u8>),
    BodyOptions(Vec<u8>),
    EffectsMode(u32),
    Frame(u32),
    Drop,
}

struct Models {
    events: Rc<RefCell<Vec<Event>>>,
    apply_options: bool,
}

impl ModelServices for Models {
    fn open(&mut self, selection: &[u8]) -> Result<Option<Box<dyn ModelPlayer>>> {
        self.events
            .borrow_mut()
            .push(Event::Open(selection.to_vec()));
        Ok(Some(Box::new(Player {
            events: self.events.clone(),
            apply_options: self.apply_options,
        })))
    }

    fn open_with_resources(
        &mut self,
        selection: &[u8],
        resources: &mut dyn ResourceServices,
    ) -> Result<Option<Box<dyn ModelPlayer>>> {
        let selection_fields = ModelSelection::parse(selection).unwrap();
        assert_eq!(selection_fields.options, b"5");
        let source = selection_fields.open(resources)?.unwrap();
        let mut signature = [0; 4];
        source.psz.read_exact_at(0, &mut signature)?;
        assert_eq!(&signature, b"PSZ\0");
        assert_eq!(source.initial_motion.as_deref(), Some(b"stand".as_slice()));
        assert_eq!(source.expression_config, None);
        assert_eq!(source.body_config.as_deref(), Some(b"body.dif".as_slice()));
        self.events.borrow_mut().push(Event::ReadModel);
        self.open(selection)
    }
}

struct Resources;

impl ResourceServices for Resources {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        Ok([category, b"/", name].concat())
    }

    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        Ok(match name {
            b"m2config/model.pfl" => {
                Some(b"#charaname\n1\tactor\tactor.psz\n#motname\n1\t0\tstand\t\tbody.dif".to_vec())
            }
            b"m2psb/actor.psz" => Some(b"PSZ\0synthetic bytes".to_vec()),
            _ => None,
        })
    }
}

struct Player {
    events: Rc<RefCell<Vec<Event>>>,
    apply_options: bool,
}

impl ModelPlayer for Player {
    fn set_options(&mut self, options: &[u8]) -> Result<bool> {
        self.events
            .borrow_mut()
            .push(Event::Options(options.to_vec()));
        Ok(self.apply_options)
    }

    fn set_body_options(&mut self, options: &[u8]) -> Result<()> {
        self.events
            .borrow_mut()
            .push(Event::BodyOptions(options.to_vec()));
        Ok(())
    }

    fn set_effects_mode(&mut self, mode: u32) -> Result<()> {
        self.events.borrow_mut().push(Event::EffectsMode(mode));
        Ok(())
    }

    fn frame(&mut self, time: u32) -> Result<Option<ModelFrame>> {
        self.events.borrow_mut().push(Event::Frame(time));
        Ok(Some(ModelFrame {
            pixels: Arc::new(Surface::from_bgra([8, 12], [16, 32, 64, 255].repeat(96))?),
            origin: [-2, -3],
        }))
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.events.borrow_mut().push(Event::Drop);
    }
}

struct NoTasks;

impl ObjectFactory for NoTasks {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成场景不创建任务"))
    }
}

#[test]
fn model_plane_samples_a_visible_player_and_removes_it_with_the_object() {
    let source = b"#DEFINE
M2PLANE actor
#OBJECT PRI DISP POS
actor 5 0 20,40
#START
actor load model,1,0,5
wait 1
actor disp 1
wait 1
actor load MODEL,1,0,5
wait 1
actor disp 0
wait 100
";
    for (available, bound_resources) in [(false, false), (true, false), (true, true)] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            NoTasks,
        );
        runtime.drawing_mut().target = Some(RenderTarget::new([64; 2]).unwrap());
        runtime.drawing_mut().enabled = true;
        runtime.set_frame_view(
            Projection::default(),
            *Transform::default().matrix(),
            TextureFilter::Point,
        );
        let events = Rc::new(RefCell::new(Vec::new()));
        if bound_resources {
            runtime.set_resources(Resources);
        }
        if available {
            runtime.set_model_services(Models {
                events: events.clone(),
                apply_options: false,
            });
        }
        let mut scene = Scene::new(
            Program::from_text(source).unwrap(),
            runtime.host().handle_allocator(),
        )
        .unwrap();
        for (tick, displayed) in [(1, false), (2, true), (3, true), (4, false)] {
            if tick == 3 {
                let id = scene.objects().lookup("actor", 0).unwrap();
                let mut code = vec![0x15, 3];
                code.extend((0x8000_0000u32 + 25 * 4).to_le_bytes());
                code.push(3);
                code.extend(2f32.to_le_bytes());
                code.extend([0x2d, 0x3b]);
                scene
                    .objects_mut()
                    .unwrap()
                    .start_motion(
                        id,
                        katze_system2::kcs::Program::from_code(&code, vec![], 64).unwrap(),
                        &[],
                        0,
                    )
                    .unwrap();
            }
            runtime.host_mut().system_time_ms = tick * 20;
            assert!(runtime.update_fes_scene(0, &mut scene, tick).unwrap());
            let id = scene.objects().lookup("actor", 0).unwrap();
            let drawable = runtime.drawables().get(id);
            assert_eq!(
                drawable.is_some_and(|drawable| drawable.display() != 0),
                available && displayed
            );
            if available && displayed {
                let drawable = drawable.unwrap();
                assert_eq!(drawable.transform().base_point()[2], 5.0);
            }
            let frame = runtime
                .with_frame(|draw, canvas| SoftwareBackend.render(draw, canvas, [64; 2]))
                .unwrap();
            assert_eq!(
                frame.pixel(17, 40),
                Some(if available && tick == 3 {
                    0xff40_2010
                } else {
                    0
                })
            );
            assert_eq!(
                frame.pixel(20, 40),
                Some(if available && displayed {
                    0xff40_2010
                } else {
                    0
                })
            );
            if tick == 3 {
                scene
                    .objects_mut()
                    .unwrap()
                    .stop_motions(id, &mut Default::default(), 64)
                    .unwrap();
            }
        }
        let id = scene.objects().lookup("actor", 0).unwrap();
        runtime
            .remove_fes_object(0, scene.objects_mut().unwrap(), id)
            .unwrap();
        assert!(runtime.drawables().get(id).is_none());
        if available {
            let expected = [
                Event::Open(b"model,1,0,5".to_vec()),
                Event::Frame(40),
                Event::Frame(60),
                Event::Drop,
            ];
            let events = events.borrow();
            if bound_resources {
                assert_eq!(events[0], Event::ReadModel);
                assert_eq!(events[1..], expected);
            } else {
                assert_eq!(*events, expected);
            }
        } else {
            assert!(events.borrow().is_empty());
        }
    }
}

#[test]
fn model_options_can_keep_a_player_until_its_motion_selection_changes() {
    let source = b"#DEFINE
M2PLANE actor
#START
actor load model,1,0,5
wait 1
actor load MODEL,1,0,6,,9
actor m2body 1,,3
actor m2body ,2
wait 1
actor m2body GLOW,,
actor m2body glow,,
wait 1
actor m2body Glow,,
wait 1
actor m2body
wait 1
actor m2body $str76
wait 1
actor m2body $str76
wait 1
actor load MODEL,1,2,6,,9
wait 100
";
    for apply_options in [true, false] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            NoTasks,
        );
        let events = Rc::new(RefCell::new(Vec::new()));
        runtime.set_model_services(Models {
            events: events.clone(),
            apply_options,
        });
        let mut scene = Scene::new(
            Program::from_text(source).unwrap(),
            runtime.host().handle_allocator(),
        )
        .unwrap();
        runtime.numbers_mut().strings.insert(76, b"$str77".to_vec());
        for tick in 1..=8 {
            assert!(runtime.update_fes_scene(0, &mut scene, tick).unwrap());
        }
        let mut expected = vec![
            Event::Open(b"model,1,0,5".to_vec()),
            Event::Options(b"6,,9".to_vec()),
        ];
        if !apply_options {
            expected.extend([Event::Open(b"MODEL,1,0,6,,9".to_vec()), Event::Drop]);
        }
        expected.extend([
            Event::BodyOptions(b",2".to_vec()),
            Event::BodyOptions(b"GLOW,,".to_vec()),
            Event::BodyOptions(b"GLOW,,".to_vec()),
            Event::BodyOptions(Vec::new()),
            Event::Open(b"MODEL,1,2,6,,9".to_vec()),
            Event::Drop,
        ]);
        assert_eq!(*events.borrow(), expected);
    }
}

#[test]
fn model_effects_mode_uses_the_raw_value_and_survives_player_replacement() {
    let source = b"#DEFINE
M2PLANE actor
#START
actor load model,1,0
wait 1
actor m2enable_eff 0
actor m2enable_eff -1
wait 1
actor m2enable_eff -1
wait 1
actor load model,1,2
wait 1
actor m2enable_eff
wait 1
actor m2enable_eff
wait 100
";
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        NoTasks,
    );
    let events = Rc::new(RefCell::new(Vec::new()));
    runtime.set_model_services(Models {
        events: events.clone(),
        apply_options: true,
    });
    let mut scene = Scene::new(
        Program::from_text(source).unwrap(),
        runtime.host().handle_allocator(),
    )
    .unwrap();
    for tick in 1..=6 {
        assert!(runtime.update_fes_scene(0, &mut scene, tick).unwrap());
    }
    assert_eq!(
        *events.borrow(),
        [
            Event::Open(b"model,1,0".to_vec()),
            Event::EffectsMode(u32::MAX),
            Event::Open(b"model,1,2".to_vec()),
            Event::Drop,
            Event::EffectsMode(u32::MAX),
            Event::EffectsMode(1),
        ]
    );
}

fn table_record(key: i32, children: &[Vec<u8>], choice: Option<(&[u8], &[u8])>) -> Vec<u8> {
    let payload_len = if choice.is_some() { 100 } else { 4 };
    let mut record = vec![0; 12 + payload_len];
    if !children.is_empty() {
        record[4..8].copy_from_slice(&((12 + payload_len) as u32).to_le_bytes());
    }
    record[8..12].copy_from_slice(&(payload_len as u32).to_le_bytes());
    record[12..16].copy_from_slice(&key.to_le_bytes());
    if let Some((profile, options)) = choice {
        record[48..48 + profile.len()].copy_from_slice(profile);
        record[80..80 + options.len()].copy_from_slice(options);
    }
    for (index, child) in children.iter().enumerate() {
        let start = record.len();
        record.extend_from_slice(child);
        if index + 1 < children.len() {
            record[start..start + 4].copy_from_slice(&(child.len() as u32).to_le_bytes());
        }
    }
    record
}

struct TableResources(Vec<u8>);

impl ResourceServices for TableResources {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        Resources.resolve(category, name)
    }

    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        if name == b"config.int/fixture.set" {
            assert_eq!(mode, ResourceMode::LooseFirst);
            return Ok(Some(self.0.clone()));
        }
        Resources.read(name, mode)
    }
}

#[test]
fn character_table_drives_model_resource_loading_and_clears_failed_selections() {
    let shape = table_record(0, &[table_record(7, &[], Some((b"@", b"1@4")))], None);
    let expression = table_record(1, &[table_record(9, &[], Some((b"model", b"@@5")))], None);
    let body = table_record(3, &[expression, shape], None);
    let table = table_record(5, &[body], None);
    let source = b"#DEFINE
CHTABLE2 table
M2PLANE actor
#OBJECT TABLE
table config.int/fixture.set
#OBJECT PRI DISP POS
actor 5 1 20,40
#START
table max 10
table max 11 5
table max 12 5 3
table max 13 5 3 0
table max 14 99
table baseopt s
table getfile 601 5 3 7 9
table baseopt
table getfile 600 5 3 7 9
actor tone ($0+=1) ($1+=1) ($2+=1)
actor load $str600;
strvar 602 old
table getfile 602 99 3 7 9
wait 100
";
    for prepare in [false, true] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(16),
            NoTasks,
        );
        runtime.set_resources(TableResources(table.clone()));
        let events = Rc::new(RefCell::new(Vec::new()));
        runtime.set_model_services(Models {
            events: events.clone(),
            apply_options: false,
        });
        let mut scene = Scene::new(
            Program::from_text(source).unwrap(),
            runtime.host().handle_allocator(),
        )
        .unwrap();
        if prepare {
            assert!(runtime.prepare_fes_scene(0, &mut scene).unwrap());
        }
        assert!(runtime.update_fes_scene(0, &mut scene, 1).unwrap());
        let numbers = runtime.numbers();
        assert_eq!(&numbers.shared[..3], &[1, 0, 0]);
        assert_eq!(&numbers.shared[10..15], &[1, 1, 2, 1, 0]);
        assert_eq!(numbers.strings[&600], b"model,1,0,5");
        assert_eq!(numbers.strings[&601], b"models,1,0,5");
        assert!(numbers.strings[&602].is_empty());
        let events = events.borrow();
        assert_eq!(events[0], Event::ReadModel);
        assert_eq!(events[1], Event::Open(b"model,1,0,5".to_vec()));
        assert!(events.iter().any(|event| matches!(event, Event::Frame(_))));
    }
}
