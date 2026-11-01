use katze_system2::{
    Error, Result,
    expression::State,
    fes::Program,
    graphics::GraphicsFactory,
    runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, fes::Scene, kind},
};

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂没有该类型"))
    }
}

fn runtime(budget: usize) -> Runtime<GraphicsFactory<Reject>> {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(8),
        GraphicsFactory { fallback: Reject },
    );
    runtime.set_budget(budget, 10000);
    runtime
}

#[test]
fn preparation_stops_before_start_and_exit_notifications_keep_the_scene_scheduled() {
    use katze_system2::fes::{ObjectAttributes, ObjectInitialization, ObjectReference};

    let source = b"#DEFINE\nCPLANE panel\n#OBJECT SIZE COLOR DISP\npanel 2,2 0xff123456 1\n#START\n$0=$0+1\nexit 37\n";
    for budget in [1, 4096] {
        let mut runtime = runtime(budget);
        let mut scene = Scene::new(
            Program::from_text(source).unwrap(),
            runtime.host().handle_allocator(),
        )
        .unwrap();
        let mut prepared = false;
        for _ in 0..16 {
            if scene.prepare(runtime.numbers_mut(), budget).unwrap() {
                prepared = true;
                break;
            }
        }
        assert!(prepared);
        assert!(scene.prepare(runtime.numbers_mut(), budget).unwrap());
        assert_eq!(runtime.numbers().shared[0], 0);
        assert_eq!(scene.exit_code(), None);
        let panel = scene.objects().lookup("panel", 0).unwrap();
        assert_eq!(scene.objects().get(panel).unwrap().task_id(), None);
        for now in [100, 101] {
            let mut completed = false;
            for _ in 0..32 {
                if runtime.update_fes_scene(0, &mut scene, now).unwrap() {
                    completed = true;
                    break;
                }
            }
            assert!(completed);
            assert_eq!(scene.exit_code(), Some(37));
            assert_eq!(runtime.numbers().shared[0], (now - 99) as i32);
            let task = scene.objects().get(panel).unwrap().task_id().unwrap();
            assert_eq!(
                runtime
                    .drawables()
                    .get(task)
                    .unwrap()
                    .transform()
                    .position(),
                [if now == 100 { 0. } else { 7. }, 0., 0.],
            );
            assert_eq!(scene.interpreter().named["_FRAME_"][&0], (now - 99) as i32);
            scene
                .objects_mut()
                .unwrap()
                .initialize(
                    &ObjectInitialization {
                        target: ObjectReference {
                            name: "panel".to_owned(),
                            bank: 0,
                        },
                        attributes: ObjectAttributes {
                            position: [Some(7), None, None],
                            ..ObjectAttributes::default()
                        },
                    },
                    |name| Ok(name.to_vec()),
                )
                .unwrap();
        }
    }
}

#[test]
fn scenes_freeze_budgeted_time_and_resume_zero_waits_and_repeated_blocks_on_later_ticks() {
    let cases = [
        (
            "$0=$0+1\nwait 3\n$1=$1+1\n",
            [
                (1, 0, 103),
                (1, 0, 103),
                (1, 0, 103),
                (1, 1, 104),
                (2, 1, 107),
                (2, 1, 107),
                (2, 1, 107),
            ],
        ),
        (
            "$0=$0+1\nwait 0\n$1=$1+1\n",
            [
                (1, 0, 101),
                (1, 1, 102),
                (2, 1, 103),
                (2, 2, 104),
                (3, 2, 105),
                (3, 3, 106),
                (4, 3, 107),
            ],
        ),
        (
            "$0=$0+1\n$1=$1+1\n",
            [
                (1, 1, 101),
                (2, 2, 102),
                (3, 3, 103),
                (4, 4, 104),
                (5, 5, 105),
                (6, 6, 106),
                (7, 7, 107),
            ],
        ),
    ];
    for budget in [1, 4096] {
        for (body, expected) in cases {
            let mut runtime = runtime(budget);
            let program = Program::from_text(format!("#START\n{body}").as_bytes()).unwrap();
            let mut scene = Scene::new(program, runtime.host().handle_allocator()).unwrap();
            for (index, (a, b, deadline)) in expected.into_iter().enumerate() {
                let now = 100 + index as u32;
                let mut completed = false;
                for retry in 0..32 {
                    if runtime
                        .update_fes_scene(0, &mut scene, now + if retry == 0 { 0 } else { 1000 })
                        .unwrap()
                    {
                        completed = true;
                        break;
                    }
                    assert!(scene.is_pending());
                    assert!(scene.objects_mut().is_err());
                }
                assert!(completed);
                assert_eq!(&runtime.numbers().shared[..2], [a, b]);
                assert_eq!(scene.deadline(), Some(deadline));
                assert_eq!(scene.exit_code(), None);
                assert_eq!(scene.interpreter().named["_FRAME_"][&0], index as i32 + 1);
            }
        }
    }
}

#[test]
fn unsupported_commands_preserve_the_event_and_do_not_repeat_prior_script_side_effects() {
    let mut runtime = runtime(4096);
    let mut scene = Scene::new(
        Program::from_text(b"#START\n$0=$0+1\nsprite play\n").unwrap(),
        runtime.host().handle_allocator(),
    )
    .unwrap();
    for _ in 0..2 {
        assert!(runtime.update_fes_scene(0, &mut scene, 100).is_err());
        assert_eq!(runtime.numbers().shared[0], 1);
        assert!(scene.is_pending());
    }
    assert!(runtime.update_fes_scene(99, &mut scene, 100).is_err());
    assert_eq!(runtime.numbers().shared[0], 1);
}
