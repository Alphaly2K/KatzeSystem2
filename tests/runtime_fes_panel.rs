use katze_system2::{
    Error, Result,
    expression::State,
    fes::Program,
    graphics::GraphicsFactory,
    runtime::{
        BaseObject, ObjectFactory, Runtime, TaskObject,
        fes::{ObjectState, Scene},
        kind,
    },
};

struct Reject;

impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂没有该媒体类型"))
    }
}

fn setup(budget: usize, source: &[u8]) -> (Runtime<GraphicsFactory<Reject>>, Scene) {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(8),
        GraphicsFactory { fallback: Reject },
    );
    runtime.set_budget(budget, 10000);
    let scene = Scene::new(
        Program::from_text(source).unwrap(),
        runtime.host().handle_allocator(),
    )
    .unwrap();
    (runtime, scene)
}

fn selected(scene: &Scene, bank: i32) -> Vec<i32> {
    let handle = scene.objects().lookup("panel", bank).unwrap();
    let ObjectState::SavePanel(panel) = &scene.objects().get(handle).unwrap().state else {
        panic!("合成对象应为存档面板")
    };
    panel.enabled_ids.iter().copied().collect()
}

#[test]
fn panel_selection_keeps_sorted_ids_and_evaluates_ranges_for_each_bank() {
    let source = b"#DEFINE\nSAVEPANEL panel[2]\n#START\npanel ptype 7 [1-3] -1 3\npanel[1] ptype 0 [2-3]\nwait 999\n#_FES_SLEEP_\npanel ptype ($0+=1) ($1+=1) [4-4] [9-7]\n";
    for budget in [1, 4096] {
        let (mut runtime, mut scene) = setup(budget, source);
        assert!((0..256).any(|_| runtime.update_fes_scene(0, &mut scene, 0).unwrap()));
        assert_eq!(selected(&scene, 0), [-1, 1, 2, 3]);
        assert_eq!(selected(&scene, 1), [-1, 1]);
        let deadline = scene.deadline();
        assert!((0..256).any(|_| {
            runtime
                .dispatch_fes_system_block(0, &mut scene, "_FES_SLEEP_")
                .unwrap()
        }));
        assert_eq!(selected(&scene, 0), [-1, 1, 2, 3, 4]);
        assert_eq!(selected(&scene, 1), [-1, 1, 2, 4]);
        assert_eq!(&runtime.numbers().shared[..2], &[1, 2]);
        assert_eq!(scene.deadline(), deadline);
    }
}

#[test]
fn panel_selection_diagnoses_unbounded_ranges_and_keeps_completed_ids() {
    for budget in [1, 4096] {
        for value in ["2147483647", "[0-1000000]", "1+"] {
            let source =
                format!("#DEFINE\nSAVEPANEL panel\n#START\npanel ptype 1 5 {value}\nwait 999\n");
            let (mut runtime, mut scene) = setup(budget, source.as_bytes());
            let error = (0..256)
                .find_map(|_| match runtime.update_fes_scene(0, &mut scene, 0) {
                    Err(error) => Some(error),
                    Ok(false) => None,
                    Ok(true) => panic!("无效编号应返回诊断"),
                })
                .expect("合成面板编号未返回诊断");
            if value != "1+" {
                assert!(error.message.contains("面板编号区间"));
            }
            assert_eq!(selected(&scene, 0), [5]);
        }
    }
}
