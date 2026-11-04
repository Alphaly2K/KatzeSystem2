use katze_system2::{
    Error, Result,
    expression::State,
    fes::{self, ObjectReference},
    graphics::{GraphicsFactory, Rect, RenderTarget},
    runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, fes::Scene, kind},
    task::{Command, CommandBuffers},
};

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂没有该类型"))
    }
}
type TestRuntime = Runtime<GraphicsFactory<Reject>>;

fn setup(script: &[u8], budget: usize) -> (TestRuntime, Scene, u32, u32) {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(8),
        GraphicsFactory { fallback: Reject },
    );
    runtime.set_budget(budget, 10000);
    runtime.host_mut().surface_size = [4, 3];
    runtime.drawing_mut().enabled = true;
    runtime.drawing_mut().active = true;
    runtime.drawing_mut().target = Some(RenderTarget::new([4, 3]).unwrap());
    let list = runtime.create(0, kind::IMAGE_LIST, b"list", b"").unwrap();
    send(&mut runtime, list, 285, [4, 3, 0]);
    let plane = runtime.tree().children(list).unwrap().next().unwrap();
    let mut scene = Scene::new(
        fes::Program::from_text(script).unwrap(),
        runtime.host().handle_allocator(),
    )
    .unwrap();
    scene
        .objects_mut()
        .unwrap()
        .set_image_list(list, Rect::from_xywh(0, 0, 4, 3));
    (runtime, scene, list, plane)
}

fn send(runtime: &mut TestRuntime, target: u32, code: u32, data: [u32; 3]) {
    runtime
        .dispatch_command(
            0,
            Command {
                target,
                sender: 0,
                code,
                data,
            },
        )
        .unwrap();
}

fn update(runtime: &mut TestRuntime, scene: &mut Scene, now: u32) -> Result<()> {
    for _ in 0..256 {
        if runtime.update_fes_scene(0, scene, now)? {
            return Ok(());
        }
    }
    panic!("合成场景没有结束本轮");
}

fn image(runtime: &mut TestRuntime, list: u32) -> u32 {
    let mut bytes = [0; 4];
    runtime
        .dispatch_command_with_buffers(
            0,
            Command {
                target: list,
                sender: 0,
                code: 291,
                data: [0; 3],
            },
            &mut CommandBuffers {
                data0: None,
                data1: Some(&mut bytes),
                external: None,
            },
        )
        .unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn background_requests_retry_once_and_handlers_queue_changes_until_the_next_update() {
    for budget in [1, 4096] {
        let (mut runtime, mut scene, list, plane) = setup(
            b"#DEFINE\nCIMAGE bg[2]\n#START\nkeyskip off\nbaseimg bg[($0+=1)] ignored ($0+=100)\n$1+=1\nwait 77\n#_FES_SLEEP_\nbaseimg\n$2+=1\n#bg.PUSH_L\nbaseimg bg[0]\n$3+=1\n",
            budget,
        );
        assert!(update(&mut runtime, &mut scene, 100).is_err());
        assert_eq!(&runtime.numbers().shared[..4], &[1, 0, 0, 0]);
        for (bank, color) in [(0, 0xff12_3456), (1, 0xff78_9abc)] {
            let object = scene.objects().lookup("bg", bank).unwrap();
            let wrapper = scene.objects().get(object).unwrap().image_handle().unwrap();
            runtime.images_mut().recreate(wrapper, [1, 1]).unwrap();
            runtime
                .images_mut()
                .get_mut(wrapper)
                .unwrap()
                .pixels_mut()
                .copy_from_slice(&u32::to_le_bytes(color));
        }
        update(&mut runtime, &mut scene, 100).unwrap();
        assert_eq!(&runtime.numbers().shared[..4], &[1, 1, 0, 0]);
        assert_eq!(scene.deadline(), Some(177));
        assert_eq!(runtime.drawables().get(plane).unwrap().display(), 1);
        let output = image(&mut runtime, list);
        send(&mut runtime, list, 296, [0; 3]);
        assert_eq!(
            runtime.images().get(output).unwrap().surface().pixel(0, 2),
            Some(0xff78_9abc)
        );
        while !runtime
            .dispatch_fes_system_block(0, &mut scene, "_FES_SLEEP_")
            .unwrap()
        {}
        assert_eq!(runtime.drawables().get(plane).unwrap().display(), 1);
        update(&mut runtime, &mut scene, 101).unwrap();
        assert_eq!(runtime.drawables().get(plane).unwrap().display(), 0);
        let target = ObjectReference {
            name: "bg".into(),
            bank: 0,
        };
        while runtime
            .dispatch_fes_event(0, &mut scene, Some(&target), 2)
            .unwrap()
            .is_none()
        {}
        assert_eq!(runtime.drawables().get(plane).unwrap().display(), 0);
        update(&mut runtime, &mut scene, 102).unwrap();
        send(&mut runtime, list, 296, [0; 3]);
        assert_eq!(runtime.drawables().get(plane).unwrap().display(), 1);
        assert_eq!(
            runtime.images().get(output).unwrap().surface().pixel(0, 2),
            Some(0xff12_3456)
        );
        assert_eq!(&runtime.numbers().shared[..4], &[1, 1, 0, 0]);
        assert_eq!(scene.deadline(), Some(177));
    }
}

#[test]
fn plane_background_crops_the_current_list_before_later_media_updates() {
    for budget in [1, 4096] {
        let (mut runtime, mut scene, list, _) = setup(
            b"#DEFINE\nPLANE region\n#OBJECT SIZE VRAM DISP\nregion 2,2 1,1 0\n#START\nbaseimg region\n$0+=1\nwait 77\n",
            budget,
        );
        let output = image(&mut runtime, list);
        for (index, pixel) in runtime
            .images_mut()
            .get_mut(output)
            .unwrap()
            .pixels_mut()
            .chunks_exact_mut(4)
            .enumerate()
        {
            pixel.copy_from_slice(&(0xff00_0001 + index as u32).to_le_bytes());
        }
        update(&mut runtime, &mut scene, 100).unwrap();
        assert_eq!(runtime.numbers().shared[0], 1);
        send(&mut runtime, list, 298, [0; 3]);
        let surface = runtime.images().get(output).unwrap().surface();
        for (x, y, color) in [(0, 1, 2), (1, 1, 3), (0, 2, 6), (1, 2, 7)] {
            assert_eq!(surface.pixel(x, y), Some(0xff00_0000 | color));
        }
        assert_eq!(surface.pixel(2, 2), Some(0));
    }
}
