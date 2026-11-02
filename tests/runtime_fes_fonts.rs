use katze_system2::{
    Error, Result,
    expression::State,
    fes::{ObjectReference, Program},
    message::{FontRequest, FontServices, GlyphMask},
    runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, fes::Scene, kind},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂没有该类型"))
    }
}

#[derive(Clone, Default)]
struct Fonts {
    names: Rc<RefCell<Vec<Vec<u8>>>>,
    fail: Rc<Cell<bool>>,
}
impl FontServices for Fonts {
    fn add_resource(&mut self, name: &[u8]) -> Result<bool> {
        self.names.borrow_mut().push(name.to_vec());
        if name == b"two.otf" && self.fail.get() {
            return Err(Error::new(0, "合成字体服务暂时不可用"));
        }
        Ok(false)
    }
    fn validate(&mut self, _: &FontRequest) -> Result<()> {
        Err(Error::new(0, "合成服务只提供字体注册"))
    }
    fn measure(&mut self, _: &FontRequest, _: char, _: u32) -> Result<i32> {
        Err(Error::new(0, "合成服务只提供字体注册"))
    }
    fn rasterize(&mut self, _: &FontRequest, _: char, _: u32) -> Result<GlyphMask> {
        Err(Error::new(0, "合成服务只提供字体注册"))
    }
}

fn runtime(budget: usize) -> Runtime<Reject> {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(8),
        Reject,
    );
    runtime.set_budget(budget, 10000);
    runtime
}

fn update(runtime: &mut Runtime<Reject>, scene: &mut Scene) -> Result<()> {
    for _ in 0..256 {
        if runtime.update_fes_scene(0, scene, 100)? {
            return Ok(());
        }
    }
    panic!("合成场景没有完成本轮更新");
}

#[test]
fn font_add_routes_expanded_names_in_main_and_handlers_and_retries_only_the_failed_call() {
    let source = b"#DEFINE\nBASE obj\n#START\n$0+=1\nfontcom add \"$str0;\" trailing\nfontcom add two.otf\n$1+=1\nwait 100\n#_FES_SLEEP_\nfontcom add sleep.ttf\n#obj.PUSH_L\nfontcom add event.otf\n";
    for budget in [1, 4096] {
        let mut runtime = runtime(budget);
        let fonts = Fonts::default();
        fonts.fail.set(true);
        runtime.set_fonts(fonts.clone());
        runtime
            .numbers_mut()
            .strings
            .insert(0, b"font one.ttf".to_vec());
        let mut scene = Scene::new(
            Program::from_text(source).unwrap(),
            runtime.host().handle_allocator(),
        )
        .unwrap();
        assert!(update(&mut runtime, &mut scene).is_err());
        assert_eq!(runtime.numbers().shared[..2], [1, 0]);
        assert_eq!(
            *fonts.names.borrow(),
            [b"font one.ttf".to_vec(), b"two.otf".to_vec()]
        );
        fonts.fail.set(false);
        update(&mut runtime, &mut scene).unwrap();
        assert_eq!(runtime.numbers().shared[..2], [1, 1]);
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
        let target = ObjectReference {
            name: "obj".to_owned(),
            bank: 0,
        };
        done = false;
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
        assert_eq!(scene.deadline(), deadline);
        assert_eq!(
            *fonts.names.borrow(),
            [
                b"font one.ttf".to_vec(),
                b"two.otf".to_vec(),
                b"two.otf".to_vec(),
                b"sleep.ttf".to_vec(),
                b"event.otf".to_vec()
            ]
        );
    }
}

#[test]
fn font_add_skips_empty_names_and_unknown_operations_but_requires_a_registration_service() {
    let source = b"#START\nfontcom\nfontcom add\nfontcom add \"\"\nfontcom ADD ignored.ttf\nfontcom unknown ignored.ttf\n$0+=1\nfontcom add real.ttf\n$1+=1\nwait 10\n";
    let mut runtime = runtime(4096);
    let mut scene = Scene::new(
        Program::from_text(source).unwrap(),
        runtime.host().handle_allocator(),
    )
    .unwrap();
    assert!(update(&mut runtime, &mut scene).is_err());
    assert_eq!(runtime.numbers().shared[..2], [1, 0]);
    let fonts = Fonts::default();
    runtime.set_fonts(fonts.clone());
    update(&mut runtime, &mut scene).unwrap();
    assert_eq!(*fonts.names.borrow(), [b"real.ttf".to_vec()]);
    assert_eq!(runtime.numbers().shared[..2], [1, 1]);
    for operation in ["init", "count", "get", "search", "default"] {
        let source = format!("#START\nfontcom {operation} 0\n");
        let mut scene = Scene::new(
            Program::from_text(source.as_bytes()).unwrap(),
            runtime.host().handle_allocator(),
        )
        .unwrap();
        assert!(
            update(&mut runtime, &mut scene)
                .unwrap_err()
                .message
                .contains("字体枚举")
        );
    }
}
