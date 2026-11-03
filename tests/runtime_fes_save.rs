use katze_system2::{
    Error, Result,
    expression::State,
    fes::{ObjectReference, Program},
    runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, fes::Scene, kind},
    save::SaveServices,
};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

struct Reject;

impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        panic!("存档查询不创建下层媒体任务")
    }
}

#[derive(Default)]
struct Files {
    times: BTreeMap<Vec<u8>, u64>,
    calls: Vec<(u32, Vec<u8>)>,
    fail: Option<Vec<u8>>,
}

struct Saves(Rc<RefCell<Files>>);

impl SaveServices for Saves {
    fn read(&mut self, _: u32, _: &[u8]) -> Result<Option<Vec<u8>>> {
        panic!("修改时间查询不读取 CSS 内容")
    }

    fn modified_time(&mut self, owner: u32, name: &[u8]) -> Result<Option<u64>> {
        let mut files = self.0.borrow_mut();
        files.calls.push((owner, name.to_vec()));
        if files.fail.as_deref() == Some(name) {
            return Err(Error::new(0, "合成修改时间查询失败"));
        }
        Ok(files.times.get(name).copied())
    }
}

fn runtime(budget: usize) -> Runtime<Reject> {
    let mut runtime = Runtime::new(
        77,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(16),
        Reject,
    );
    runtime.set_budget(budget, 10000);
    runtime
}

fn new_scene(runtime: &Runtime<Reject>, text: &[u8]) -> Scene {
    Scene::new(
        Program::from_text(text).unwrap(),
        runtime.host().handle_allocator(),
    )
    .unwrap()
}

fn update(runtime: &mut Runtime<Reject>, scene: &mut Scene, now: u32) -> Result<()> {
    for _ in 0..256 {
        if runtime.update_fes_scene(77, scene, now)? {
            return Ok(());
        }
    }
    panic!("合成存档查询未在预算续跑后完成")
}

fn times(entries: &[(i32, u64)]) -> BTreeMap<Vec<u8>, u64> {
    entries
        .iter()
        .map(|&(slot, time)| (format!("save{slot:04}.dat").into_bytes(), time))
        .collect()
}

#[test]
fn save_queries_preserve_signed_slots_ties_and_parameter_order_in_all_fes_entries() {
    let script = b"#DEFINE\nBASE obj[2]\n#START\nnewsave ($8+=1) ($9+=1)\nnewsave ($0+=1)+3 ($1+=1)-2 ($2+=1)+1 ($3+=1)\nnewsave 10 4 3\nnewsave 11 -4 -3\nwait 99\n#_FES_SLEEP_\nnewsave 6 -3 -2\n$12+=1\n#obj.PUSH_L\nnewsave 7 1 2\n$13=@\n";
    for budget in [1, 4096] {
        let mut runtime = runtime(budget);
        let files = Rc::new(RefCell::new(Files {
            times: times(&[
                (-4, u64::MAX - 1),
                (-3, 0),
                (-2, 0),
                (-1, u64::MAX),
                (0, 1),
                (1, 2),
                (2, 2),
            ]),
            ..Default::default()
        }));
        runtime.set_save_services(Saves(files.clone()));
        let mut scene = new_scene(&runtime, script);
        update(&mut runtime, &mut scene, 100).unwrap();
        assert_eq!(&runtime.numbers().shared[..4], &[1, 1, 1, 0]);
        assert_eq!(runtime.numbers().shared[4], 1);
        assert_eq!(&runtime.numbers().shared[8..12], &[1, 1, -1, -4]);
        let deadline = scene.deadline();
        let mut done = false;
        for _ in 0..256 {
            if runtime
                .dispatch_fes_system_block(77, &mut scene, "_FES_SLEEP_")
                .unwrap()
            {
                done = true;
                break;
            }
        }
        assert!(done);
        let target = ObjectReference {
            name: "obj".into(),
            bank: 1,
        };
        done = false;
        for _ in 0..256 {
            if let Some(handled) = runtime
                .dispatch_fes_event(77, &mut scene, Some(&target), 2)
                .unwrap()
            {
                assert!(handled);
                done = true;
                break;
            }
        }
        assert!(done);
        assert_eq!(runtime.numbers().shared[6], -3);
        assert_eq!(runtime.numbers().shared[7], 1);
        assert_eq!(&runtime.numbers().shared[12..14], &[1, 1]);
        assert_eq!(scene.deadline(), deadline);
        let expected = [-1, 0, 1, 2, -4, -3, -3, -2, 1, 2]
            .map(|slot| (77, format!("save{slot:04}.dat").into_bytes()));
        assert_eq!(files.borrow().calls, expected);
    }
}

#[test]
fn failed_save_queries_keep_parameters_and_completed_files_until_the_round_finishes() {
    for budget in [1, 4096] {
        let mut runtime = runtime(budget);
        runtime.numbers_mut().shared[4] = 77;
        let mut scene = new_scene(
            &runtime,
            b"#START\n$10+=1\nnewsave 4 ($0+=1)-1 ($1+=1)+1\n$11+=1\nwait 99\n",
        );
        let error = update(&mut runtime, &mut scene, 100).unwrap_err();
        assert!(error.message.contains("修改时间查询服务"));
        assert!(scene.is_pending());
        assert!(scene.objects_mut().is_err());
        assert_eq!(&runtime.numbers().shared[..2], &[1, 1]);
        assert_eq!(runtime.numbers().shared[4], 77);
        assert_eq!(&runtime.numbers().shared[10..12], &[1, 0]);
        let files = Rc::new(RefCell::new(Files {
            times: times(&[(0, 1), (1, 2), (2, 2)]),
            fail: Some(b"save0001.dat".to_vec()),
            ..Default::default()
        }));
        runtime.set_save_services(Saves(files.clone()));
        let error = update(&mut runtime, &mut scene, 900).unwrap_err();
        assert!(error.message.contains("合成修改时间查询失败"));
        assert_eq!(runtime.numbers().shared[4], 77);
        files.borrow_mut().fail = None;
        files
            .borrow_mut()
            .times
            .insert(b"save0000.dat".to_vec(), u64::MAX);
        update(&mut runtime, &mut scene, 999).unwrap();
        assert_eq!(&runtime.numbers().shared[..2], &[1, 1]);
        assert_eq!(runtime.numbers().shared[4], 1);
        assert_eq!(&runtime.numbers().shared[10..12], &[1, 1]);
        assert_eq!(scene.deadline(), Some(199));
        assert_eq!(
            files.borrow().calls,
            [0, 1, 1, 2].map(|slot| (77, format!("save{slot:04}.dat").into_bytes()))
        );
        for text in [
            b"#START\nnewsave 0 0 1000000\n".as_slice(),
            b"#START\nnewsave 0 2147483647 2147483647\n",
        ] {
            let mut invalid = new_scene(&runtime, text);
            let error = update(&mut runtime, &mut invalid, 1000).unwrap_err();
            assert!(error.message.contains("存档扫描范围"));
            assert_eq!(files.borrow().calls.len(), 4);
        }
    }
}
