use katze_system2::{
    Result, Step,
    expression::State,
    fes,
    graphics::{
        GraphicsFactory, ImageList, ImageListProperties, Images, Projection, RenderTarget, Surface,
        TextureFilter, Transform,
    },
    kcs::Program,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, fes::Objects, kind},
};

#[test]
fn list_redraw_clips_relative_to_the_source_and_keeps_separate_wrapper_lifetimes() {
    let mut images = Images::default();
    let source = images.create(1, [3, 2]).unwrap();
    for (pixel, color) in images
        .get_mut(source)
        .unwrap()
        .pixels_mut()
        .chunks_exact_mut(4)
        .zip([
            0xff12_3456u32,
            0x0011_2233,
            0xff98_7654,
            0x0012_3456,
            0x0177_8899,
            0x80ab_cdef,
        ])
    {
        pixel.copy_from_slice(&color.to_le_bytes());
    }
    let mut list = ImageList::new(2, [4, 4], &mut images).unwrap();
    assert_eq!(ImageList::key(0x11234, 0x8001), 0x8001_1234);
    let properties = ImageListProperties {
        position: [0.75, 2.75],
        base_point: [1.5, 1.5],
        opacity: 128,
        visible: true,
    };
    list.set(0, source, properties, &mut images).unwrap();
    list.set(0, source, properties, &mut images).unwrap();
    images.release_owner(1);
    assert!(images.get(source).is_err());
    list.redraw(&mut images).unwrap();
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixels(),
        [
            0,
            0,
            0,
            0,
            0x0011_2233,
            0x8098_7654,
            0x0098_7654,
            0,
            0x0177_8899,
            0x40ab_cdef,
            0x00ab_cdef,
            0,
            0,
            0,
            0,
            0,
        ]
        .map(u32::to_le_bytes)
        .concat()
    );
    let opaque = images.create(3, [1, 1]).unwrap();
    images
        .get_mut(opaque)
        .unwrap()
        .pixels_mut()
        .copy_from_slice(&0xff00_aa00u32.to_le_bytes());
    let top = ImageListProperties {
        position: [0., 1.],
        ..ImageListProperties::default()
    };
    list.set(0x8000_0000, opaque, top, &mut images).unwrap();
    list.set(
        u32::MAX,
        opaque,
        ImageListProperties {
            visible: false,
            ..top
        },
        &mut images,
    )
    .unwrap();
    assert!(list.set(0, source, properties, &mut images).is_err());
    list.redraw(&mut images).unwrap();
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(0, 2),
        Some(0xff00_aa00)
    );
    list.set(
        0,
        opaque,
        ImageListProperties { opacity: 0, ..top },
        &mut images,
    )
    .unwrap();
    assert_eq!(list.len(), 3);
    list.redraw(&mut images).unwrap();
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(1, 2),
        Some(0)
    );
    assert!(!list.remove(7, &mut images).unwrap());
    assert!(list.remove(0x8000_0000, &mut images).unwrap());
    list.redraw(&mut images).unwrap();
    assert!(
        images
            .get(list.image())
            .unwrap()
            .surface()
            .pixels()
            .iter()
            .all(|&v| v == 0)
    );
    list.clear(&mut images).unwrap();
    assert!(list.is_empty());
    let output = list.image();
    list.release(&mut images).unwrap();
    list.release(&mut images).unwrap();
    assert!(images.get(output).is_err());
    assert!(list.redraw(&mut images).is_err());
    assert!(list.set(0, opaque, top, &mut images).is_err());
    assert!(images.get(opaque).is_ok());
    let owned = ImageList::new(3, [1, 1], &mut images).unwrap();
    images.release_owner(3);
    assert!(images.get(owned.image()).is_err());
    assert!(images.get(opaque).is_err());
}

struct Programs;
impl ProgramLoader for Programs {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        let mut code = Vec::new();
        let mut ins = |op, args: &[u32]| {
            code.push(op);
            for value in args {
                code.push(3);
                code.extend(value.to_le_bytes());
            }
        };
        ins(0x25, &[2]);
        ins(0x25, &[2]);
        ins(0x2b, &[112, 8]);
        ins(0x26, &[0]);
        code.extend([0x25, 0x13, 0, 0, 0, 0]);
        for value in [0u32, 0, 2, 2, 0xff12_4578] {
            code.push(0x25);
            code.push(3);
            code.extend(value.to_le_bytes());
        }
        code.push(0x2b);
        code.push(3);
        code.extend(117u32.to_le_bytes());
        code.push(3);
        code.extend(24u32.to_le_bytes());
        code.push(0x3b);
        Ok(Some(Program::from_code(&code, vec![0; 16], 128)?))
    }
}

#[test]
fn runtime_lists_keep_kcs_images_alive_and_feed_fes_canvas_planes() {
    for budget in [1, 4096] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            GraphicsFactory {
                fallback: KcsFactory::new(Programs),
            },
        );
        runtime.set_budget(budget, 1 << 20);
        let painter = runtime.create(0, kind::ORDINARY, b"paint", b"").unwrap();
        let owner = runtime.create(0, kind::PLANE, b"list", b"").unwrap();
        runtime.update(0, 0).unwrap();
        let source = u32::from_le_bytes(
            runtime
                .object(painter)
                .unwrap()
                .interpreter()
                .unwrap()
                .read(0, 4)
                .unwrap()
                .try_into()
                .unwrap(),
        );
        let mut list = ImageList::new(owner, [2, 2], runtime.images_mut()).unwrap();
        list.set(
            ImageList::key(painter, 0),
            source,
            ImageListProperties::default(),
            runtime.images_mut(),
        )
        .unwrap();
        runtime.destroy(painter).unwrap();
        assert!(runtime.images().get(source).is_err());
        let canvas = runtime.drawing_mut();
        canvas.enabled = true;
        canvas.active = true;
        let mut tile = Surface::new([2, 3]).unwrap();
        tile.clear(0xff01_0203);
        canvas.target = Some(
            RenderTarget::from_tiles([2, 3], [2, 1], vec![Some(tile.clone()), Some(tile)]).unwrap(),
        );
        list.set_offset([1, 1]);
        assert!(runtime.redraw_image_list(&mut list).unwrap());
        assert!(
            runtime
                .drawing()
                .target
                .as_ref()
                .unwrap()
                .dirty_tiles()
                .iter()
                .all(|&dirty| dirty)
        );
        let mut objects = Objects::new(runtime.host().handle_allocator()).unwrap();
        let mut fes = fes::Interpreter::new(
            fes::Program::from_text(
                b"#DEFINE\nPLANE view\n#OBJECT SIZE DISP\nview 4,3 1\n#START\nexit 0\n",
            )
            .unwrap(),
        )
        .unwrap();
        loop {
            match fes.run(runtime.numbers_mut(), budget).unwrap() {
                Step::Yield => (),
                Step::Event(fes::Event::CreateObject(definition)) => {
                    objects.define(&definition).unwrap();
                    fes.resume();
                }
                Step::Event(fes::Event::InitializeObject(request)) => {
                    objects.initialize(&request, |_| unreachable!()).unwrap();
                    fes.resume();
                }
                Step::Event(fes::Event::Exit(0)) => fes.resume(),
                Step::Halted => break,
                other => panic!("{other:?}"),
            }
        }
        let plane = objects.lookup("view", 0).unwrap();
        runtime.update_fes_object(0, &mut objects, plane).unwrap();
        let mut frame = RenderTarget::new([4, 3]).unwrap();
        runtime
            .render_frame(
                &mut frame,
                &Projection::default(),
                *Transform::default().matrix(),
                TextureFilter::Point,
            )
            .unwrap();
        for y in 0..3 {
            for x in 0..4 {
                assert_eq!(
                    frame.pixel(x, y),
                    Some(if (1..3).contains(&x) && y > 0 {
                        0xff12_4578
                    } else {
                        0xff01_0203
                    })
                );
            }
        }
        list.clear(runtime.images_mut()).unwrap();
        runtime.drawing_mut().enabled = false;
        assert!(!runtime.redraw_image_list(&mut list).unwrap());
        assert!(
            runtime
                .images()
                .get(list.image())
                .unwrap()
                .surface()
                .pixels()
                .iter()
                .all(|&v| v == 0)
        );
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(1, 1),
            Some(0xff12_4578)
        );
        runtime.drawing_mut().enabled = true;
        assert!(runtime.redraw_image_list(&mut list).unwrap());
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(1, 1),
            Some(0)
        );
        runtime.destroy(owner).unwrap();
        assert!(runtime.images().get(list.image()).is_err());
        assert!(runtime.redraw_image_list(&mut list).is_err());
    }
}

#[test]
fn list_alpha_classification_stays_with_shared_pixels_and_waits_for_visible_geometry() {
    for (initial, replacement, first_x, edge) in [
        (0x8011_2233u32, 0xff44_5566u32, 1., 0x0044_5566),
        (0xff11_2233, 0x8044_5566, 1., 0),
        (0xff11_2233, 0x8044_5566, 9., 0x0044_5566),
    ] {
        let mut images = Images::default();
        let source = images.create(1, [1, 1]).unwrap();
        images
            .get_mut(source)
            .unwrap()
            .pixels_mut()
            .copy_from_slice(&initial.to_le_bytes());
        let mut list = ImageList::new(2, [3, 1], &mut images).unwrap();
        list.set(
            7,
            source,
            ImageListProperties {
                position: [first_x, 0.],
                ..ImageListProperties::default()
            },
            &mut images,
        )
        .unwrap();
        list.redraw(&mut images).unwrap();
        let alias = images.share(source).unwrap();
        images.release_owner(1);
        assert!(images.get(source).is_err());
        images.get_mut(alias).unwrap().fill_rect_from_bottom(
            katze_system2::graphics::Rect::from_xywh(0, 0, 1, 1),
            replacement,
        );
        list.set(
            7,
            alias,
            ImageListProperties {
                position: [1., 0.],
                ..ImageListProperties::default()
            },
            &mut images,
        )
        .unwrap();
        list.redraw(&mut images).unwrap();
        assert_eq!(
            images.get(list.image()).unwrap().surface().pixels(),
            [edge, replacement, edge].map(u32::to_le_bytes).concat()
        );
        list.release(&mut images).unwrap();
        images.release(alias).unwrap();
    }
}
