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
        for tick in 1..=3 {
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
            Event::BodyOptions(b"1,,3".to_vec()),
            Event::BodyOptions(b",2".to_vec()),
            Event::Open(b"MODEL,1,2,6,,9".to_vec()),
            Event::Drop,
        ]);
        assert_eq!(*events.borrow(), expected);
    }
}
