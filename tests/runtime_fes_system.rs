use katze_system2::{
    Error, Result,
    expression::State,
    fes::Program,
    runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, fes::Scene, kind},
};

struct NoTasks;
impl ObjectFactory for NoTasks {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成场景不创建任务"))
    }
}
fn setup(source: &str, budget: usize) -> (Runtime<NoTasks>, Scene) {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(8),
        NoTasks,
    );
    runtime.set_budget(budget, 10000);
    let scene = Scene::new(
        Program::from_text(source.as_bytes()).unwrap(),
        runtime.host().handle_allocator(),
    )
    .unwrap();
    (runtime, scene)
}
fn update(runtime: &mut Runtime<NoTasks>, scene: &mut Scene, now: u32) {
    for _ in 0..64 {
        if runtime.update_fes_scene(0, scene, now).unwrap() {
            return;
        }
    }
    panic!("合成场景没有完成本轮");
}
fn system(runtime: &mut Runtime<NoTasks>, scene: &mut Scene, name: &str) {
    for _ in 0..64 {
        if runtime.dispatch_fes_system_block(0, scene, name).unwrap() {
            return;
        }
    }
    panic!("合成系统块没有完成");
}

fn event(runtime: &mut Runtime<NoTasks>, scene: &mut Scene) -> bool {
    let target = katze_system2::fes::ObjectReference {
        name: "button".into(),
        bank: 0,
    };
    for _ in 0..64 {
        if let Some(handled) = runtime
            .dispatch_fes_event(0, scene, Some(&target), 2)
            .unwrap()
        {
            return handled;
        }
    }
    panic!("合成对象事件没有完成");
}

#[test]
fn object_events_preserve_waits_queue_exits_and_transfer_only_the_main_cursor() {
    for budget in [1, 4096] {
        for (body, expected, transferred, exit) in [
            ("$1=1\n", 1, false, None),
            ("$1=1\nwait 5\n$1=99\n", 1, false, None),
            ("$1=1\ndecode\n$1=99\n", 1, false, None),
            ("$1=1\nexit 9\n$1=99\n", 1, false, Some(9)),
            ("$1=1\nnext DEST\n#DEST\n$1=99\nwait 5\n", 1, true, None),
            ("call SUB\n$1=$1*10+2\n#SUB\n$1=3\nwait 5\n", 3, false, None),
        ] {
            let source =
                format!("#DEFINE\nBASE button\n#START\nwait 77\n$7=$7+1\n#button.PUSH_L\n{body}");
            let (mut runtime, mut scene) = setup(&source, budget);
            update(&mut runtime, &mut scene, 100);
            assert!(event(&mut runtime, &mut scene));
            assert_eq!(runtime.numbers().shared[1], expected);
            assert_eq!(scene.deadline(), if transferred { None } else { Some(177) });
            assert_eq!(scene.exit_code(), None);
            assert_eq!(
                scene.interpreter().named["_CLICK_L_"][&0],
                scene.objects().lookup("button", 0).unwrap() as i32
            );
            update(&mut runtime, &mut scene, 101);
            assert_eq!(scene.exit_code(), exit);
            assert_eq!(scene.interpreter().named["_CLICK_L_"][&0], 0);
            assert_eq!(runtime.numbers().shared[7], if transferred { 0 } else { 1 });
        }
    }
}

#[test]
fn failed_object_events_keep_their_cursor_and_reject_other_dispatches() {
    let (mut runtime, mut scene) = setup(
        "#DEFINE\nBASE button\n#START\nwait 77\n#button.PUSH_L\n$1=$1+1\nunknown operation\n",
        1,
    );
    update(&mut runtime, &mut scene, 100);
    let target = katze_system2::fes::ObjectReference {
        name: "button".into(),
        bank: 0,
    };
    assert_eq!(
        runtime
            .dispatch_fes_event(0, &mut scene, Some(&target), 2)
            .unwrap(),
        None
    );
    assert!(scene.is_pending());
    assert!(runtime.dispatch_fes_event(0, &mut scene, None, 2).is_err());
    assert!(
        runtime
            .dispatch_fes_event(0, &mut scene, Some(&target), 3)
            .is_err()
    );
    assert!(
        runtime
            .dispatch_fes_system_block(0, &mut scene, "_FES_SLEEP_")
            .is_err()
    );
    assert!(runtime.update_fes_scene(0, &mut scene, 101).is_err());
    for _ in 0..2 {
        assert!(
            runtime
                .dispatch_fes_event(0, &mut scene, Some(&target), 2)
                .unwrap_err()
                .message
                .contains("对象命令")
        );
        assert_eq!(runtime.numbers().shared[1], 1);
        assert_eq!(scene.deadline(), Some(177));
    }
}

#[test]
fn system_input_values_keep_object_identity_without_event_handlers() {
    let (mut runtime, mut scene) = setup("#DEFINE\nBASE button\n#START\nwait 77\n", 1);
    update(&mut runtime, &mut scene, 100);
    let target = katze_system2::fes::ObjectReference {
        name: "button".into(),
        bank: 0,
    };
    let missing = katze_system2::fes::ObjectReference {
        name: "absent".into(),
        bank: 0,
    };
    let handle = scene.objects().lookup("button", 0).unwrap() as i32;
    for (target, code, name, expected) in [
        (Some(&target), 3, "_CLICK_R_", handle),
        (None, 3, "_CLICK_R_", -1),
        (Some(&target), 3, "_CLICK_R_", handle),
        (Some(&missing), 3, "_CLICK_R_", handle),
        (Some(&target), 5, "_CLICK_R_", 0),
        (Some(&target), 6, "_SCRL_U_", handle),
        (Some(&target), 21, "_TOUCH_", handle),
    ] {
        assert_eq!(
            runtime
                .dispatch_fes_event(0, &mut scene, target, code)
                .unwrap(),
            Some(false)
        );
        assert_eq!(scene.interpreter().named[name][&0], expected);
    }
}

#[test]
fn system_blocks_preserve_the_main_wait_and_stop_at_local_wait_or_break() {
    let cases = [
        ("$1=$1*10+1\n", 1, 0),
        ("call SUB\n$1=$1*10+1\n#SUB\n$1=$1*10+3\n", 31, 0),
        ("$1=1\nwait 5\n$1=12\n", 1, 0),
        ("$1=1\ndecode\n$1=12\n", 1, 0),
        ("$1=1\nbreak 7\n$1=12\n", 1, 7),
    ];
    for budget in [1, 4096] {
        for (body, expected, result) in cases {
            let source = format!("#START\n$0=$0+1\nwait 77\n$7=$7+1\n#_FES_SLEEP_\n{body}");
            let (mut runtime, mut scene) = setup(&source, budget);
            update(&mut runtime, &mut scene, 100);
            assert_eq!(scene.deadline(), Some(177));
            system(&mut runtime, &mut scene, "_FES_SLEEP_");
            assert_eq!(runtime.numbers().shared[0], 1);
            assert_eq!(runtime.numbers().shared[1], expected);
            assert_eq!(scene.interpreter().return_value, result);
            assert_eq!(scene.deadline(), Some(177));
            assert_eq!(scene.interpreter().named["_FRAME_"][&0], 1);
            assert!(!scene.is_pending());
            assert!(scene.objects_mut().is_ok());
            system(&mut runtime, &mut scene, "_FES_WAKE_");
            assert_eq!(scene.deadline(), Some(177));
            update(&mut runtime, &mut scene, 101);
            assert_eq!(runtime.numbers().shared[7], 0);
            update(&mut runtime, &mut scene, 177);
            assert_eq!(runtime.numbers().shared[7], 1);
        }
    }
}

#[test]
fn system_next_replaces_the_main_position_and_clears_its_wait() {
    for budget in [1, 4096] {
        let (mut runtime, mut scene) = setup(
            "#START\nwait 77\n$7=99\n#_FES_SLEEP_\n$1=1\nnext DEST\n$1=999\n#DEST\n$2=$2+3\nwait 5\n",
            budget,
        );
        update(&mut runtime, &mut scene, 100);
        system(&mut runtime, &mut scene, "_FES_SLEEP_");
        assert_eq!(runtime.numbers().shared[1], 1);
        assert_eq!(runtime.numbers().shared[2], 0);
        assert_eq!(scene.deadline(), None);
        update(&mut runtime, &mut scene, 101);
        assert_eq!(runtime.numbers().shared[2], 3);
        assert_eq!(runtime.numbers().shared[7], 0);
        assert_eq!(scene.deadline(), Some(106));
    }
}

#[test]
fn pending_or_failed_system_blocks_keep_their_cursor_and_reject_other_work() {
    let (mut runtime, mut scene) = setup(
        "#START\nwait 77\n#_FES_SLEEP_\n$1=$1+1\nunknown operation\n",
        1,
    );
    update(&mut runtime, &mut scene, 100);
    assert!(
        !runtime
            .dispatch_fes_system_block(0, &mut scene, "_FES_SLEEP_")
            .unwrap()
    );
    assert!(scene.is_pending());
    assert!(runtime.update_fes_scene(0, &mut scene, 101).is_err());
    assert!(
        runtime
            .dispatch_fes_system_block(0, &mut scene, "_FES_WAKE_")
            .is_err()
    );
    assert!(scene.objects_mut().is_err());
    for _ in 0..2 {
        assert!(
            runtime
                .dispatch_fes_system_block(0, &mut scene, "_FES_SLEEP_")
                .unwrap_err()
                .message
                .contains("对象命令")
        );
        assert_eq!(runtime.numbers().shared[1], 1);
        assert_eq!(scene.deadline(), Some(177));
    }
}

#[test]
fn system_blocks_between_exit_notifications_preserve_the_main_cursor() {
    for budget in [1, 4096] {
        let (mut runtime, mut scene) =
            setup("#START\n$0=$0+1\nexit 37\n#_FES_WAKE_\n$1=$1+1\n", budget);
        update(&mut runtime, &mut scene, 100);
        system(&mut runtime, &mut scene, "_FES_WAKE_");
        assert_eq!(scene.exit_code(), Some(37));
        update(&mut runtime, &mut scene, 101);
        assert_eq!(&runtime.numbers().shared[..2], &[2, 1]);
        assert_eq!(scene.exit_code(), Some(37));
    }
}

#[test]
fn system_transfers_local_waits_and_exits_keep_the_native_call_stack_effects() {
    for budget in [1, 4096] {
        for (operation, notification) in [("wait 5", None), ("exit 9", Some(9)), ("decode", None)] {
            let source = format!(
                "#START\nwait 77\n$7=$7+1\n#_FES_SLEEP_\ncall SUB\n$1=$1*10+1\n#SUB\n$1=$1*10+3\n{operation}\n$1=999\n"
            );
            let (mut runtime, mut scene) = setup(&source, budget);
            update(&mut runtime, &mut scene, 100);
            system(&mut runtime, &mut scene, "_FES_SLEEP_");
            assert_eq!(runtime.numbers().shared[1], 3);
            assert_eq!(scene.deadline(), Some(177));
            assert_eq!(scene.exit_code(), None);
            update(&mut runtime, &mut scene, 101);
            assert_eq!(scene.exit_code(), notification);
            update(&mut runtime, &mut scene, 177);
            assert_eq!(runtime.numbers().shared[1], 31);
            assert_eq!(runtime.numbers().shared[7], 1);
        }

        let (mut runtime, mut scene) = setup(
            "#START\ncall MAIN\n$7=999\n#MAIN\nwait 77\n$7=$7+1\n#_FES_SLEEP_\n$1=$1*10+1\nnext DEST\n#DEST\n$1=$1*10+3\n",
            budget,
        );
        update(&mut runtime, &mut scene, 100);
        system(&mut runtime, &mut scene, "_FES_SLEEP_");
        assert_eq!(runtime.numbers().shared[1], 13);
        assert_eq!(scene.deadline(), Some(177));
        update(&mut runtime, &mut scene, 177);
        assert_eq!(runtime.numbers().shared[7], 1);
    }
}

#[test]
fn pause_keeps_the_return_slot_and_compares_the_full_raw_value() {
    use katze_system2::{
        kcs::CoreHost,
        runtime::fes::FesObject,
        task::{Command, CommandOutcome},
    };
    let mut task = FesObject::new(9);
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    let mut output = 0x13579bdf;
    for value in [0, 1, 1, 2, 0x80000000, 0, 0] {
        assert_eq!(
            task.dispatch_command(
                kind::FES,
                &Command {
                    target: 9,
                    sender: 0,
                    code: 348,
                    data: [0, value, 1]
                },
                &mut output,
                &mut host,
                1,
            )
            .unwrap(),
            CommandOutcome::finished(true),
        );
        assert_eq!(output, 0x13579bdf);
        assert_eq!(task.paused(), value);
    }
}
