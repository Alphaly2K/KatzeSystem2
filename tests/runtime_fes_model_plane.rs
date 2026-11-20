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
    Frame(u32),
    Drop,
}

struct Models(Rc<RefCell<Vec<Event>>>);

impl ModelServices for Models {
    fn open(&mut self, selection: &[u8]) -> Result<Option<Box<dyn ModelPlayer>>> {
        self.0.borrow_mut().push(Event::Open(selection.to_vec()));
        Ok(Some(Box::new(Player(self.0.clone()))))
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
        self.0.borrow_mut().push(Event::ReadModel);
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

struct Player(Rc<RefCell<Vec<Event>>>);

impl ModelPlayer for Player {
    fn frame(&mut self, time: u32) -> Result<Option<ModelFrame>> {
        self.0.borrow_mut().push(Event::Frame(time));
        Ok(Some(ModelFrame {
            pixels: Arc::new(Surface::from_bgra([8, 12], [16, 32, 64, 255].repeat(96))?),
            origin: [-2, -3],
        }))
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.0.borrow_mut().push(Event::Drop);
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
            runtime.set_model_services(Models(events.clone()));
        }
        let mut scene = Scene::new(
            Program::from_text(source).unwrap(),
            runtime.host().handle_allocator(),
        )
        .unwrap();
        for (tick, displayed) in [(1, false), (2, true), (3, true), (4, false)] {
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
                assert_eq!(drawable.transform().position(), [18.0, 37.0, 0.0]);
                assert_eq!(drawable.transform().base_point()[2], 5.0);
            }
            let frame = runtime
                .with_frame(|draw, canvas| SoftwareBackend.render(draw, canvas, [64; 2]))
                .unwrap();
            assert_eq!(
                frame.pixel(20, 40),
                Some(if available && displayed {
                    0xff40_2010
                } else {
                    0
                })
            );
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
