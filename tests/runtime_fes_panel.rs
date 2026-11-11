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

fn cell(scene: &Scene, bank: i32, id: i32) -> &katze_system2::fes::PanelCell {
    let handle = scene.objects().lookup("panel", bank).unwrap();
    let ObjectState::SavePanel(panel) = &scene.objects().get(handle).unwrap().state else {
        panic!("合成对象应为存档面板")
    };
    &panel.cells[&id]
}

fn word(cell: &katze_system2::fes::PanelCell, offset: usize) -> u32 {
    u32::from_le_bytes(cell.record()[offset..offset + 4].try_into().unwrap())
}

#[test]
fn panel_slots_clip_flags_and_preserve_previous_data_after_missing_or_invalid_files() {
    use katze_system2::save::{DataContainer, SaveServices};
    use std::{cell::RefCell, rc::Rc};

    type Calls = Rc<RefCell<Vec<(bool, Vec<u8>)>>>;
    struct Files {
        regular: Vec<u8>,
        short: Vec<u8>,
        calls: Calls,
    }
    impl SaveServices for Files {
        fn read(&mut self, owner: u32, name: &[u8]) -> Result<Option<Vec<u8>>> {
            assert_eq!(owner, 0);
            self.calls.borrow_mut().push((false, name.to_vec()));
            Ok(match name {
                b"save0010.dat" => Some(self.regular.clone()),
                b"save0011.dat" => Some(b"BAD!".to_vec()),
                b"save0013.dat" => Some(self.short.clone()),
                _ => None,
            })
        }
        fn modified_date(&mut self, owner: u32, name: &[u8]) -> Result<Option<[u32; 6]>> {
            assert_eq!(owner, 0);
            self.calls.borrow_mut().push((true, name.to_vec()));
            Ok(Some([2026, 11, 9, 12, 34, 56]))
        }
    }
    fn panel(scene: &Scene, bank: i32) -> &katze_system2::fes::SavePanelState {
        let handle = scene.objects().lookup("panel", bank).unwrap();
        let ObjectState::SavePanel(panel) = &scene.objects().get(handle).unwrap().state else {
            panic!("合成对象应为存档面板")
        };
        panel
    }
    let mut data = DataContainer::new(7);
    let values: Vec<_> = [-7i32, 22, 33]
        .into_iter()
        .flat_map(i32::to_le_bytes)
        .collect();
    data.append(77, &values).unwrap();
    let regular = data.to_css().unwrap();
    let mut data = DataContainer::new(8);
    data.append(77, &[88, 0, 0, 0, 255]).unwrap();
    let short = data.to_css().unwrap();
    let script = b"#DEFINE\nSAVEPANEL panel[2]\n#START\npanel dispid 1 7\npanel ptype NORM 7\npanel setsave ($6+=1) ($6+=1) ($6+=1)\npanel getflag ($6+=1) ($6+=1)\npanel[1] setsave ($0+=1)+9 77 1022\npanel[1] getflag 1022 3\npanel[0] getflag 1022 4\npanel[1] getflag 1024 5\npanel[1] getflag ($1+=1)\nwait 999\n#_FES_SLEEP_\n$7+=1\nif ($7==1)\npanel[1] setsave 11 99 0\npanel[1] getflag 1023 2\nelse\npanel[1] setsave 13 77 1023\npanel[1] getflag -1 -1\nendif\n#_FES_WAKE_\npanel[1] setsave 12 77 0\n";
    for budget in [1, 4096] {
        let (mut runtime, mut scene) = setup(budget, script);
        let calls = Rc::new(RefCell::new(Vec::new()));
        runtime.set_save_services(Files {
            regular: regular.clone(),
            short: short.clone(),
            calls: calls.clone(),
        });
        assert!((0..256).any(|_| runtime.update_fes_scene(0, &mut scene, 0).unwrap()));
        assert_eq!(&runtime.numbers().shared[..7], &[1, 1, 0, -7, 0, 0, 0]);
        assert_eq!(panel(&scene, 0).flags, vec![0; 1024]);
        assert!(panel(&scene, 0).data.is_none());
        assert_eq!(&panel(&scene, 1).flags[1022..], &[-7, 22]);
        assert_eq!(panel(&scene, 1).date, [2026, 11, 9, 12, 34, 56]);
        let loaded = panel(&scene, 1).data.clone();
        for (block, name, record) in [
            ("_FES_SLEEP_", b"save0011.dat", 99),
            ("_FES_WAKE_", b"save0012.dat", 77),
        ] {
            assert!((0..256).any(|_| {
                runtime
                    .dispatch_fes_system_block(0, &mut scene, block)
                    .unwrap()
            }));
            let panel = panel(&scene, 1);
            assert_eq!(panel.save_name, name);
            assert_eq!(panel.record_id, record);
            assert_eq!(panel.date, [0; 6]);
            assert_eq!(panel.data, loaded);
            assert_eq!(&panel.flags[1022..], &[-7, 22]);
        }
        assert!((0..256).any(|_| {
            runtime
                .dispatch_fes_system_block(0, &mut scene, "_FES_SLEEP_")
                .unwrap()
        }));
        assert_eq!(runtime.numbers().shared[2], 22);
        assert_eq!(&panel(&scene, 1).flags[1022..], &[-7, 88]);
        assert_eq!(panel(&scene, 1).data.as_ref().unwrap().tag(), 8);
        assert_eq!(selected(&scene, 1), [7]);
        assert_eq!(cell(&scene, 1, 7).record(), &[0; 188]);
        assert_eq!(
            *calls.borrow(),
            [
                (false, b"save0010.dat".to_vec()),
                (true, b"save0010.dat".to_vec()),
                (false, b"save0011.dat".to_vec()),
                (false, b"save0012.dat".to_vec()),
                (false, b"save0013.dat".to_vec()),
                (true, b"save0013.dat".to_vec()),
            ]
        );
    }
}

#[test]
fn panel_cells_parse_once_and_replace_one_bank_without_changing_selection() {
    let source = b"#DEFINE\nSAVEPANEL panel[2]\nBASE source[2]\n#START\npanel dispid 1 9\npanel ptype SIMAGE ($0+=1)+8 ($1+=1)+1 32 19\npanel ptype STR 12 FLAG.3:4 ($2+=1) 20 30 3 17 2 5 %04d\npanel ptype NUM_O2 13 source[-1] DATE_Y:2\npanel ptype IMAGEOBJ 14 source[1]\npanel ptype NUM_S 15\npanel ptype NUM_I 16\npanel ptype NORM 17\npanel ptype IMAGE 18 ($7+=1)\npanel ptype SSTR 19 6 10 20 30\npanel ptype unknown ($7+=1)\npanel ptype STR 12 FLAG\npanel setnum 9 ($3+=1)*7\npanel setnum 99 ($4+=1)\npanel setnum ($5+=1)+98\npanel[0] setnum 19 \"\"\nwait 999\n#_FES_SLEEP_\npanel[1] setnum 9 -3\n#_FES_WAKE_\npanel[1] ptype NORM 9\n";
    for budget in [1, 4096] {
        let (mut runtime, mut scene) = setup(budget, source);
        assert!((0..256).any(|_| runtime.update_fes_scene(0, &mut scene, 0).unwrap()));
        assert_eq!(&runtime.numbers().shared[..6], &[1, 1, 1, 1, 1, 1]);
        assert_eq!(runtime.numbers().shared[7], 0);
        for bank in [0, 1] {
            assert_eq!(selected(&scene, bank), [9]);
            let image = cell(&scene, bank, 9);
            assert_eq!(
                (image.kind(), image.source_kind(), word(image, 0x10)),
                (7, 10, 2)
            );
            assert_eq!((word(image, 0xb4), word(image, 0xb8)), (31, 18));
            let text = cell(&scene, bank, 12);
            assert_eq!((text.kind(), text.source_kind()), (3, 0));
            assert_eq!((word(text, 8), word(text, 12)), (3, 4));
            assert_eq!(
                (word(text, 0x14), word(text, 0x18), word(text, 0x1c)),
                (1, 30, 20)
            );
            assert_eq!((word(text, 0x48), word(text, 0x4c)), (1, 1));
            assert_eq!(&text.record()[0x64..0x68], b"%04d");
            let number = cell(&scene, bank, 13);
            assert_eq!((number.kind(), number.source_kind()), (5, 3));
            assert_eq!((word(number, 0x40), word(number, 0x54)), (u32::MAX, 1));
            assert_eq!(word(cell(&scene, bank, 14), 0x40), 1);
            for (id, kind) in [(15, 4), (16, 6), (17, 0), (18, 1)] {
                assert_eq!(cell(&scene, bank, id).kind(), kind);
            }
            let text = cell(&scene, bank, 19);
            assert_eq!((text.source_kind(), word(text, 0x10)), (2, 6));
            assert_eq!(word(text, 0x60), 6);
        }
        assert!((0..256).any(|_| {
            runtime
                .dispatch_fes_system_block(0, &mut scene, "_FES_SLEEP_")
                .unwrap()
        }));
        assert_eq!(word(cell(&scene, 0, 9), 0x60), 7);
        assert_eq!(word(cell(&scene, 1, 9), 0x60), (-3i32) as u32);
        assert!((0..256).any(|_| {
            runtime
                .dispatch_fes_system_block(0, &mut scene, "_FES_WAKE_")
                .unwrap()
        }));
        assert_eq!(cell(&scene, 0, 9).kind(), 7);
        assert_eq!(cell(&scene, 1, 9).record(), &[0; 188]);
        assert_eq!(selected(&scene, 1), [9]);
    }
}

#[test]
fn panel_selection_keeps_sorted_ids_and_evaluates_ranges_for_each_bank() {
    let source = b"#DEFINE\nSAVEPANEL panel[2]\n#START\npanel dispid 7 [1-3] -1 3\npanel[1] dispid 0 [2-3]\nwait 999\n#_FES_SLEEP_\npanel dispid ($0+=1) ($1+=1) [4-4] [9-7]\n";
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
                format!("#DEFINE\nSAVEPANEL panel\n#START\npanel dispid 1 5 {value}\nwait 999\n");
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
