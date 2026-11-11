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
    runtime.host_mut().surface_size = [8, 6];
    runtime.drawing_mut().enabled = true;
    runtime.drawing_mut().active = true;
    runtime.drawing_mut().target =
        Some(katze_system2::graphics::RenderTarget::new([8, 6]).unwrap());
    let list = runtime.create(0, kind::IMAGE_LIST, b"list", b"").unwrap();
    runtime
        .dispatch_command(
            0,
            katze_system2::task::Command {
                target: list,
                sender: 0,
                code: 285,
                data: [8, 6, 0],
            },
        )
        .unwrap();
    let mut scene = Scene::new(
        Program::from_text(source).unwrap(),
        runtime.host().handle_allocator(),
    )
    .unwrap();
    scene
        .objects_mut()
        .unwrap()
        .set_image_list(list, katze_system2::graphics::Rect::from_xywh(0, 0, 8, 6));
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

fn pixel_file(id: u32) -> Vec<u8> {
    use std::io::Write;

    let compress = |bytes: &[u8]| {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), Default::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    };
    let pixels = compress(&[6, 42, 27, 64]);
    let control = compress(&[9, 1]);
    let mut image = b"HG-3".to_vec();
    for value in [12u32, 0x300, 0, id] {
        image.extend(value.to_le_bytes());
    }
    image.extend(b"stdinfo\0");
    for value in [56u32, 40, 1, 1, 32, 0, 0, 1, 1, 0, 0, 0] {
        image.extend(value.to_le_bytes());
    }
    image.extend(b"img0000\0");
    for value in [
        0,
        (24 + pixels.len() + control.len()) as u32,
        0,
        1,
        pixels.len() as u32,
        4,
        control.len() as u32,
        2,
    ] {
        image.extend(value.to_le_bytes());
    }
    image.extend(pixels);
    image.extend(control);

    image
}

#[test]
fn panel_text_wraps_using_character_advances_and_retries_a_failed_font() {
    use katze_system2::{
        kcs::ResourceServices,
        message::{FontRequest, FontServices, GlyphMask},
        resource::ResourceMode,
        save::DataContainer,
    };
    use std::{cell::RefCell, rc::Rc};

    struct Files(Vec<u8>);
    impl ResourceServices for Files {
        fn resolve(&mut self, _: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            Ok(name.to_vec())
        }
        fn read(&mut self, _: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            Ok(Some(self.0.clone()))
        }
    }
    struct Fonts(Rc<RefCell<Vec<char>>>);
    impl FontServices for Fonts {
        fn validate(&mut self, request: &FontRequest) -> Result<()> {
            assert_eq!(request.name, "Fixture".encode_utf16().collect::<Vec<_>>());
            Ok(())
        }
        fn measure(&mut self, _: &FontRequest, _: char, _: u32) -> Result<i32> {
            unreachable!()
        }
        fn rasterize(&mut self, _: &FontRequest, character: char, size: u32) -> Result<GlyphMask> {
            assert_eq!(size, 4);
            let mut calls = self.0.borrow_mut();
            calls.push(character);
            if calls.len() == 2 {
                return Err(Error::new(0, "合成字形失败"));
            }
            GlyphMask::new(
                [1; 2],
                [0; 2],
                if character == 'B' { 0 } else { 3 },
                vec![64],
            )
        }
    }
    let (mut runtime, mut scene) = setup(4096, b"#DEFINE\nSAVEPANEL panel\n#START\npanel ptype STR 0 88 4 1 0x123456 0 0 2 1 Fixture\npanel dispid 1 0\nwait 999\n");
    let mut file = pixel_file(0);
    file[48..52].copy_from_slice(&(-2i32).to_le_bytes());
    file[52..56].copy_from_slice(&(-2i32).to_le_bytes());
    file[56..60].copy_from_slice(&8u32.to_le_bytes());
    file[60..64].copy_from_slice(&6u32.to_le_bytes());
    runtime.set_resources(Files(file));
    let calls = Rc::new(RefCell::new(Vec::new()));
    runtime.set_fonts(Fonts(calls.clone()));
    assert!(runtime.update_fes_scene(0, &mut scene, 0).unwrap());
    let objects = scene.objects_mut().unwrap();
    objects
        .initialize(
            &katze_system2::fes::ObjectInitialization {
                target: katze_system2::fes::ObjectReference {
                    name: "panel".into(),
                    bank: 0,
                },
                attributes: katze_system2::fes::ObjectAttributes {
                    file: Some(b"cells".to_vec()),
                    ..Default::default()
                },
            },
            |name| Ok(name.to_vec()),
        )
        .unwrap();
    let handle = objects.lookup("panel", 0).unwrap();
    let ObjectState::SavePanel(panel) = &mut objects.get_mut(handle).unwrap().state else {
        unreachable!()
    };
    let mut data = DataContainer::new(0);
    data.append(88, b"ABCDE\0").unwrap();
    panel.data = Some(data);
    panel.apply_data().unwrap();
    assert!(runtime.update_fes_object(0, objects, handle).is_err());
    let wrapper = objects.get(handle).unwrap().image_handle().unwrap();
    assert_eq!(runtime.images().pixel_handle(wrapper).unwrap(), 0);
    runtime.update_fes_object(0, objects, handle).unwrap();
    let image = runtime.images().get(wrapper).unwrap();
    for y in 0..6 {
        for x in 0..8 {
            assert_eq!(
                image.surface().pixel(x, y).unwrap() >> 24,
                if x == 0 && [0, 3].contains(&y) {
                    255
                } else {
                    0
                },
                "({x}, {y})"
            );
        }
    }
    assert_eq!(image.surface().pixel(0, 0), Some(0xff123456));
    assert_eq!(*calls.borrow(), ['A', 'B', 'A', 'B']);
    runtime.update_fes_object(0, objects, handle).unwrap();
    assert_eq!(*calls.borrow(), ['A', 'B', 'A', 'B']);
}

#[test]
fn panel_images_compose_enabled_cells_in_the_shared_list_and_refresh_numbers() {
    use katze_system2::{kcs::ResourceServices, resource::ResourceMode, save::DataContainer};

    struct Files(Vec<u8>);
    impl ResourceServices for Files {
        fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            assert_eq!(category, b"image");
            Ok(name.to_vec())
        }
        fn read(&mut self, _: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            Ok(Some(self.0.clone()))
        }
    }
    let mut file = b"HG-3".to_vec();
    file.extend(12u32.to_le_bytes());
    file.extend(0x300u32.to_le_bytes());
    for id in 0..4 {
        let mut frame = pixel_file(id).split_off(12);
        let next = if id == 3 { 0 } else { frame.len() as u32 };
        frame[..4].copy_from_slice(&next.to_le_bytes());
        for (at, value) in [
            (36, [0, 2, 3, 4][id as usize]),
            (40, [1, 0, 1, 0][id as usize]),
            (44, 8u32),
            (48, 6),
        ] {
            frame[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        file.extend(frame);
    }
    let (mut runtime, mut scene) = setup(4096, b"#DEFINE\nSAVEPANEL panel\nIMAGE source\n#START\npanel ptype NORM 0\npanel ptype SIMAGE 1 77\npanel ptype NUM_O2 2 source[0] FLAG.0\npanel ptype NORM 3\npanel dispid 1 0 1 2\nwait 999\n#_FES_SLEEP_\npanel setnum 2 1\npanel dispid 0 0\n#_FES_WAKE_\npanel dispid 1 -1\n");
    runtime.set_resources(Files(file));
    assert!(runtime.update_fes_scene(0, &mut scene, 0).unwrap());
    let objects = scene.objects_mut().unwrap();
    for name in ["panel", "source"] {
        objects
            .initialize(
                &katze_system2::fes::ObjectInitialization {
                    target: katze_system2::fes::ObjectReference {
                        name: name.into(),
                        bank: 0,
                    },
                    attributes: katze_system2::fes::ObjectAttributes {
                        file: Some(b"cells".to_vec()),
                        priority: Some(if name == "panel" { 5 } else { 6 }),
                        display: Some(1),
                        ..Default::default()
                    },
                },
                |name| Ok(name.to_vec()),
            )
            .unwrap();
    }
    let handle = objects.lookup("panel", 0).unwrap();
    let ObjectState::SavePanel(panel) = &mut objects.get_mut(handle).unwrap().state else {
        unreachable!()
    };
    let mut data = DataContainer::new(0);
    data.append(77, &pixel_file(0)).unwrap();
    panel.data = Some(data);
    panel.flags[0] = 3;
    panel.apply_data().unwrap();
    runtime.update_fes_object(0, objects, handle).unwrap();
    let wrapper = objects.get(handle).unwrap().image_handle().unwrap();
    let image = runtime.images().get(wrapper).unwrap();
    assert_eq!(image.info().size, [8, 6]);
    for x in 0..8 {
        assert_eq!(
            image.surface().pixel(x, 1).unwrap() >> 24,
            if [0, 7].contains(&x) { 255 } else { 0 },
            "x={x}"
        );
    }
    assert_eq!(image.surface().pixel(2, 0).unwrap() >> 24, 255);
    let list = objects.image_list().unwrap();
    runtime
        .dispatch_command(
            0,
            katze_system2::task::Command {
                target: list,
                sender: 0,
                code: 294,
                data: [0; 3],
            },
        )
        .map(|count| assert_eq!(count, 1))
        .unwrap();
    assert!(
        runtime
            .dispatch_fes_system_block(0, &mut scene, "_FES_SLEEP_")
            .unwrap()
    );
    runtime
        .update_fes_object(0, scene.objects_mut().unwrap(), handle)
        .unwrap();
    let image = runtime.images().get(wrapper).unwrap();
    assert_eq!(image.surface().pixel(0, 1).unwrap() >> 24, 0);
    assert_eq!(image.surface().pixel(5, 1).unwrap() >> 24, 255);
    assert_eq!(image.surface().pixel(7, 1).unwrap() >> 24, 0);
    assert!(
        runtime
            .dispatch_fes_system_block(0, &mut scene, "_FES_WAKE_")
            .unwrap()
    );
    runtime
        .update_fes_object(0, scene.objects_mut().unwrap(), handle)
        .unwrap();
    assert!(
        runtime
            .images()
            .get(wrapper)
            .unwrap()
            .surface()
            .pixels()
            .iter()
            .all(|&b| b == 0)
    );
    runtime
        .remove_fes_object(0, scene.objects_mut().unwrap(), handle)
        .unwrap();
    assert!(runtime.images().get(wrapper).is_err());
}

#[test]
fn panel_images_use_frame_zero_and_keep_pixels_when_a_replacement_cannot_load() {
    use katze_system2::save::DataContainer;

    let mut image = pixel_file(7);

    let (mut runtime, mut scene) = setup(
        4096,
        b"#DEFINE\nSAVEPANEL panel\n#START\npanel ptype SIMAGE 7 77 2 3\nwait 999\n#_FES_SLEEP_\npanel ptype SIMAGE 7 88 5 6\n#_FES_WAKE_\npanel ptype IMAGE 7\n",
    );
    assert!(runtime.update_fes_scene(0, &mut scene, 0).unwrap());
    fn panel(scene: &mut Scene) -> &mut katze_system2::fes::SavePanelState {
        let handle = scene.objects().lookup("panel", 0).unwrap();
        let ObjectState::SavePanel(panel) =
            &mut scene.objects_mut().unwrap().get_mut(handle).unwrap().state
        else {
            unreachable!()
        };
        panel
    }
    let mut data = DataContainer::new(0);
    data.append(77, &image).unwrap();
    panel(&mut scene).data = Some(data);
    panel(&mut scene).apply_data().unwrap();
    assert!(panel(&mut scene).contents.is_empty());
    image[16..20].copy_from_slice(&0u32.to_le_bytes());
    let mut data = DataContainer::new(0);
    data.append(77, &image).unwrap();
    panel(&mut scene).data = Some(data);
    panel(&mut scene).apply_data().unwrap();
    let content = panel(&mut scene).contents[&7].clone();
    assert_eq!(
        content.image().unwrap().surface().pixel(0, 0),
        Some(0xff123456)
    );
    assert_eq!(&content.record()[0xb4..0xbc], &[1, 0, 0, 0, 2, 0, 0, 0]);

    assert!(
        runtime
            .dispatch_fes_system_block(0, &mut scene, "_FES_SLEEP_")
            .unwrap()
    );
    let mut data = DataContainer::new(0);
    data.append(88, b"invalid image").unwrap();
    panel(&mut scene).data = Some(data);
    panel(&mut scene).apply_data().unwrap();
    let retained = panel(&mut scene).contents[&7].clone();
    assert_eq!(retained.image(), content.image());
    assert_eq!(&retained.record()[0xb4..0xbc], &[4, 0, 0, 0, 5, 0, 0, 0]);
    panel(&mut scene).data = None;
    panel(&mut scene).apply_data().unwrap();
    assert_eq!(panel(&mut scene).contents[&7], retained);
    assert!(
        runtime
            .dispatch_fes_system_block(0, &mut scene, "_FES_WAKE_")
            .unwrap()
    );
    panel(&mut scene).apply_data().unwrap();
    assert!(panel(&mut scene).contents[&7].image().is_none());
    assert_eq!(panel(&mut scene).contents[&7].kind(), 1);
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
    data.append(88, b"SAVED TEXT\0").unwrap();
    let regular = data.to_css().unwrap();
    let mut data = DataContainer::new(8);
    data.append(77, &[88, 0, 0, 0, 255]).unwrap();
    let short = data.to_css().unwrap();
    let script = b"#DEFINE\nSAVEPANEL panel[2]\n#START\npanel dispid 1 7\npanel ptype NORM 7\npanel ptype NUM_O2 8 panel[0] FLAG.1022:4\npanel ptype STR 9 FLAG.1023 18 100 -1 3 0x112233 2 4 Family\npanel ptype STR 10 DATE_Y 18 100 -1\npanel ptype STR 11 88 18 100 -1\npanel setsave ($6+=1) ($6+=1) ($6+=1)\npanel getflag ($6+=1) ($6+=1)\npanel[1] setsave ($0+=1)+9 77 1022\npanel[1] setnum 8 123\npanel[1] getflag 1022 3\npanel[0] getflag 1022 4\npanel[1] getflag 1024 5\npanel[1] getflag ($1+=1)\nwait 999\n#_FES_SLEEP_\n$7+=1\nif ($7==1)\npanel[1] setsave 11 99 0\npanel[1] getflag 1023 2\nelse\npanel[1] setsave 13 77 1023\npanel[1] getflag -1 -1\nendif\n#_FES_WAKE_\npanel[1] setsave 12 77 0\n";
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
        assert!(panel(&scene, 0).contents.is_empty());
        assert_eq!(panel(&scene, 1).contents.len(), 5);
        assert_eq!(panel(&scene, 1).contents[&8].number(), 123);
        assert_eq!(panel(&scene, 1).contents[&9].text(), b"22");
        assert_eq!(panel(&scene, 1).contents[&10].text(), b"2026");
        assert_eq!(panel(&scene, 1).contents[&11].text(), b"SAVED TEXT");
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
            assert_eq!(panel.contents[&8].number(), -7);
            assert_eq!(panel.contents[&9].text(), b"22");
            assert_eq!(panel.contents[&10].text(), b"0");
            assert_eq!(panel.contents[&11].text(), b"SAVED TEXT");
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
        assert_eq!(panel(&scene, 1).contents[&9].text(), b"88");
        assert_eq!(panel(&scene, 1).contents[&10].text(), b"2026");
        assert_eq!(panel(&scene, 1).contents[&11].text(), b"SAVED TEXT");
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
    let source = b"#DEFINE\nSAVEPANEL panel[2]\nBASE source[2]\n#START\npanel dispid 1 9\npanel ptype SIMAGE ($0+=1)+8 ($1+=1)+1 32 19\npanel ptype STR 12 FLAG.3:4 ($2+=1) 20 30 3 17 2 5 Family\npanel ptype NUM_O2 13 source[-1] DATE_Y:2\npanel ptype IMAGEOBJ 14 source[1]\npanel ptype NUM_S 15\npanel ptype NUM_I 16\npanel ptype NORM 17\npanel ptype IMAGE 18 ($7+=1)\npanel ptype SSTR 19 6 10 20 30\npanel ptype unknown ($7+=1)\npanel ptype STR 12 FLAG\npanel setnum 9 ($3+=1)*7\npanel setnum 99 ($4+=1)\npanel setnum ($5+=1)+98\npanel[0] setnum 19 \"\"\nwait 999\n#_FES_SLEEP_\npanel[1] setnum 9 -3\n#_FES_WAKE_\npanel[1] ptype NORM 9\n";
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
            assert_eq!(&text.record()[0x64..0x6a], b"Family");
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
