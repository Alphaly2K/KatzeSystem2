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

fn update(runtime: &mut Runtime<GraphicsFactory<Reject>>, scene: &mut Scene, now: u32) {
    for _ in 0..128 {
        if runtime.update_fes_scene(0, scene, now).unwrap() {
            return;
        }
    }
    panic!("合成场景没有完成本轮更新");
}

#[test]
fn frame_waits_use_absolute_scene_time_and_negative_durations_still_yield_one_round() {
    let source =
        b"#START\nwait frame ($2+=3)\n$0+=1\nwait frame 3\n$1+=1\nwait -9\n$3+=1\nwait 1000\n";
    for budget in [1, 4096] {
        let mut runtime = runtime(budget);
        let mut scene = Scene::new(
            Program::from_text(source).unwrap(),
            runtime.host().handle_allocator(),
        )
        .unwrap();
        for (now, values, deadline) in [
            (100, [0, 0, 3, 0], u32::MAX - 1),
            (101, [0, 0, 3, 0], u32::MAX - 1),
            (102, [1, 0, 3, 0], u32::MAX - 1),
            (103, [1, 1, 3, 0], 104),
            (104, [1, 1, 3, 1], 1104),
        ] {
            update(&mut runtime, &mut scene, now);
            assert_eq!(runtime.numbers().shared[..4], values);
            assert_eq!(scene.deadline(), Some(deadline));
            assert_eq!(scene.interpreter().named["_FRAME_"][&0], now as i32 - 99);
        }
    }
}

#[test]
fn object_waits_evaluate_banks_once_and_scan_past_missing_banks() {
    let source =
        b"#DEFINE\nBASE obj[3]\n#START\nwait obj[($2+=1)-1]\n$0+=1\nwait obj\n$1+=1\nwait 1000\n";
    for budget in [1, 4096] {
        let mut runtime = runtime(budget);
        let mut scene = Scene::new(
            Program::from_text(source).unwrap(),
            runtime.host().handle_allocator(),
        )
        .unwrap();
        let mut prepared = false;
        for _ in 0..32 {
            if scene.prepare(runtime.numbers_mut(), budget).unwrap() {
                prepared = true;
                break;
            }
        }
        assert!(prepared);
        let missing = scene.objects().lookup("obj", 1).unwrap();
        let running = scene.objects().lookup("obj", 2).unwrap();
        let objects = scene.objects_mut().unwrap();
        objects.remove(missing).unwrap();
        objects
            .start_motion(
                running,
                katze_system2::kcs::Program::from_code(&[0x3b], vec![], 64).unwrap(),
                &[],
                0,
            )
            .unwrap();
        for (now, expected) in [(100, [0, 0, 1]), (101, [1, 0, 1]), (102, [1, 0, 1])] {
            update(&mut runtime, &mut scene, now);
            assert_eq!(runtime.numbers().shared[..3], expected);
            assert_eq!(scene.deadline(), Some(u32::MAX - 3));
        }
        while !scene
            .objects_mut()
            .unwrap()
            .stop_motions(
                running,
                &mut katze_system2::kcs::MotionHost::default(),
                budget,
            )
            .unwrap()
        {}
        update(&mut runtime, &mut scene, 103);
        assert_eq!(&runtime.numbers().shared[..3], &[1, 1, 1]);
        assert_eq!(scene.deadline(), Some(1103));
    }
}

#[test]
fn keyskip_force_preserves_active_mode_and_cont_reuses_clicks_until_reconfigured() {
    for budget in [1, 4096] {
        let (mut forced_runtime, mut scene) = {
            let runtime = runtime(budget);
            let scene = Scene::new(
                Program::from_text(b"#START\nkeyskip OFF CTRL CONT FORCE ($0+=1)\n$1=_CLICK_L_\nwait 1\nkeyskip force 0\n$2=_CLICK_L_\n$3+=1\nwait frame 1000\n$4+=1\nwait 1000\n").unwrap(),
                runtime.host().handle_allocator(),
            ).unwrap();
            (runtime, scene)
        };
        update(&mut forced_runtime, &mut scene, 100);
        assert_eq!(&forced_runtime.numbers().shared[..5], &[1, -1, 0, 0, 0]);
        assert!(scene.forced_key_skip());
        assert_eq!(scene.raw_input(), 0);
        update(&mut forced_runtime, &mut scene, 101);
        assert_eq!(&forced_runtime.numbers().shared[..5], &[1, -1, -1, 1, 0]);
        assert!(!scene.forced_key_skip());
        assert_eq!(scene.deadline(), Some(u32::MAX - 1));
        update(&mut forced_runtime, &mut scene, 102);
        assert_eq!(forced_runtime.numbers().shared[4], 1);
        assert_eq!(scene.deadline(), Some(1102));

        for continuous in [false, true] {
            let mut runtime = runtime(budget);
            let source = format!(
                "#START\nkeyskip{}\nwait 1000\n$0+=1\nwait 1000\n$1+=1\nkeyskip\nwait 1000\n$2+=1\n",
                if continuous { " cont" } else { "" }
            );
            let mut scene = Scene::new(
                Program::from_text(source.as_bytes()).unwrap(),
                runtime.host().handle_allocator(),
            )
            .unwrap();
            update(&mut runtime, &mut scene, 100);
            assert_eq!(
                runtime.dispatch_fes_event(0, &mut scene, None, 2).unwrap(),
                Some(false)
            );
            update(&mut runtime, &mut scene, 101);
            assert_eq!(&runtime.numbers().shared[..3], &[1, 0, 0]);
            update(&mut runtime, &mut scene, 102);
            assert_eq!(
                &runtime.numbers().shared[..3],
                &[1, i32::from(continuous), 0]
            );
            update(&mut runtime, &mut scene, 103);
            assert_eq!(runtime.numbers().shared[2], 0);
        }
    }
}

#[test]
fn keyskip_off_blocks_timed_and_root_cancellation_but_plain_object_clicks_still_continue() {
    for budget in [1, 4096] {
        for (wait, mode, ctrl, continues) in [
            ("wait obj", false, false, true),
            ("wait obj", true, false, false),
            ("wait obj", true, true, true),
            ("wait", false, false, false),
            ("wait frame 1000", false, false, false),
            ("wait 1000", false, false, false),
        ] {
            let mut runtime = runtime(budget);
            let source = format!(
                "#DEFINE\nBASE obj\n#START\nkeyskip off{}\n{wait}\n$0+=1\nwait 1000\n",
                if ctrl { " ctrl" } else { "" }
            );
            let mut scene = Scene::new(
                Program::from_text(source.as_bytes()).unwrap(),
                runtime.host().handle_allocator(),
            )
            .unwrap();
            let mut prepared = false;
            for _ in 0..32 {
                if scene.prepare(runtime.numbers_mut(), budget).unwrap() {
                    prepared = true;
                    break;
                }
            }
            assert!(prepared);
            let handle = scene.objects().lookup("obj", 0).unwrap();
            scene
                .objects_mut()
                .unwrap()
                .start_motion(
                    handle,
                    katze_system2::kcs::Program::from_code(&[0x3b], vec![], 64).unwrap(),
                    &[],
                    0,
                )
                .unwrap();
            update(&mut runtime, &mut scene, 100);
            assert_eq!(
                runtime
                    .dispatch_fes_event(0, &mut scene, None, if mode { 19 } else { 2 })
                    .unwrap(),
                Some(false)
            );
            update(&mut runtime, &mut scene, 101);
            assert_eq!(
                runtime.numbers().shared[0],
                i32::from(continues),
                "{wait} mode={mode} ctrl={ctrl}"
            );
            let queue = scene.objects().get(handle).unwrap().motions();
            assert_eq!(queue.task(1).unwrap().state().unwrap().finishing, 0);
        }
    }
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
        Program::from_text(b"#DEFINE\nBASE sprite\n#START\n$0=$0+1\nsprite wipe\n").unwrap(),
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
