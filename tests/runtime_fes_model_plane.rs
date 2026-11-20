use katze_system2::{
    Error, Result,
    expression::State,
    fes::Program,
    host::{
        graphics::Surface,
        model::{ModelFrame, ModelPlayer, ModelServices},
    },
    runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, fes::Scene, kind},
};
use std::{cell::RefCell, rc::Rc, sync::Arc};

#[derive(Debug, PartialEq)]
enum Event {
    Open(Vec<u8>),
    Frame(u32),
    Drop,
}

struct Models(Rc<RefCell<Vec<Event>>>);

impl ModelServices for Models {
    fn open(&mut self, selection: &[u8]) -> Result<Option<Box<dyn ModelPlayer>>> {
        self.0.borrow_mut().push(Event::Open(selection.to_vec()));
        Ok(Some(Box::new(Player(self.0.clone()))))
    }
}

struct Player(Rc<RefCell<Vec<Event>>>);

impl ModelPlayer for Player {
    fn frame(&mut self, time: u32) -> Result<Option<ModelFrame>> {
        self.0.borrow_mut().push(Event::Frame(time));
        Ok(Some(ModelFrame {
            pixels: Arc::new(Surface::new([8, 12])?),
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
    for available in [false, true] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            NoTasks,
        );
        let events = Rc::new(RefCell::new(Vec::new()));
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
        }
        let id = scene.objects().lookup("actor", 0).unwrap();
        runtime
            .remove_fes_object(0, scene.objects_mut().unwrap(), id)
            .unwrap();
        assert!(runtime.drawables().get(id).is_none());
        if available {
            assert_eq!(
                *events.borrow(),
                [
                    Event::Open(b"model,1,0,5".to_vec()),
                    Event::Frame(40),
                    Event::Frame(60),
                    Event::Drop
                ]
            );
        } else {
            assert!(events.borrow().is_empty());
        }
    }
}
