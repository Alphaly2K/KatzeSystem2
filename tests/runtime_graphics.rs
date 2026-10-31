use katze_system2::{
    Error, Result,
    expression::State,
    graphics::{Drawable, GraphicsFactory},
    runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, kind},
    task::Command,
};

struct BaseFactory;

impl ObjectFactory for BaseFactory {
    fn create(&mut self, _id: u32, object_kind: u32, _name: &[u8]) -> Result<Box<dyn TaskObject>> {
        if object_kind == kind::BASE {
            Ok(Box::new(BaseObject::new()))
        } else {
            Err(Error::new(0, "合成工厂未提供该类型"))
        }
    }
}

#[test]
fn plane_factory_commands_and_lifecycle_share_the_runtime_drawing_registry() {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory {
            fallback: BaseFactory,
        },
    );
    let id = runtime.create(0, kind::PLANE, b"plane", b"").unwrap();
    assert_eq!(id, 1);
    assert!(runtime.object(id).unwrap().interpreter().is_none());
    assert!(runtime.drawables().get(id).is_none());
    runtime
        .drawables_mut()
        .insert(id, Drawable::default())
        .unwrap();
    let command = |code, a, b| Command {
        target: id,
        sender: 0,
        code,
        data: [a, b, 0],
    };
    assert_eq!(
        runtime.dispatch_command(0, command(13, 0, 0)).unwrap(),
        kind::PLANE
    );
    runtime
        .dispatch_command(0, command(59, u32::MAX, 0x0012_3456))
        .unwrap();
    assert_eq!(
        runtime.drawables().get(id).unwrap().color(),
        (0x0012_3456, u32::MAX)
    );
    runtime.dispatch_command(0, command(63, 0, 0x1234)).unwrap();
    assert_eq!(
        runtime.drawables().get(id).unwrap().color(),
        (0x3412_3456, 1)
    );
    runtime.dispatch_command(0, command(81, 0, 5)).unwrap();
    assert_eq!(runtime.drawables().get(id).unwrap().blend_mode(), 5);
    runtime
        .dispatch_command(0, command(81, 0, u32::MAX))
        .unwrap();
    assert_eq!(runtime.drawables().get(id).unwrap().blend_mode(), 0);
    runtime.dispatch_command(0, command(24, 77, 19)).unwrap();
    assert_eq!(runtime.dispatch_command(0, command(23, 0, 0)).unwrap(), 19);
    runtime
        .dispatch_command(0, command(29, 0, u32::MAX))
        .unwrap();
    assert_eq!(runtime.object(id).unwrap().clock().paused, -1);
    for code in [60, 71, 85, 86] {
        runtime
            .dispatch_command(0, command(code, u32::MAX, 0))
            .unwrap();
    }
    let error = runtime
        .dispatch_command(0, command(49, 0x1234, 0))
        .unwrap_err();
    assert!(error.message.contains("二维对象命令 49"));
    runtime.dispatch_command(0, command(68, 0, 0)).unwrap();
    assert!(runtime.drawables().get(id).is_none());
    assert!(runtime.object(id).is_some());
    runtime
        .drawables_mut()
        .insert(id, Drawable::default())
        .unwrap();
    runtime.dispatch_command(0, command(3, 0, 0)).unwrap();
    assert!(runtime.drawables().get(id).is_none());
    assert_eq!(runtime.object(id).unwrap().control().deletion_requested, 1);
    assert_eq!(runtime.dispatch_command(0, command(13, 0, 0)).unwrap(), 0);
    runtime.update(10, 1).unwrap();
    assert!(runtime.object(id).is_none());
    assert_eq!(runtime.create(0, kind::PLANE, b"next", b"").unwrap(), id);
    runtime
        .drawables_mut()
        .insert(id, Drawable::default())
        .unwrap();
    runtime.destroy(id).unwrap();
    assert!(runtime.drawables().get(id).is_none());
    runtime.update(20, 1).unwrap();
    assert!(
        runtime
            .create(0, kind::PLANE, b"context", b"unsupported")
            .is_err()
    );
    assert_eq!(runtime.create(0, kind::BASE, b"base", b"").unwrap(), id);
}
