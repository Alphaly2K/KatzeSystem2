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

#[test]
fn pixel_handles_stay_distinct_and_live_while_list_wrappers_hold_them() {
    let mut images = Images::default();
    let source = images.create(1, [1, 1]).unwrap();
    let pixels = images.pixel_handle(source).unwrap();
    assert_ne!(source, pixels);
    assert!(images.get(pixels).is_err());
    assert!(images.pixel_handle(pixels).is_err());
    assert!(images.release(pixels).is_err());
    images
        .get_mut(source)
        .unwrap()
        .pixels_mut()
        .copy_from_slice(&0xff12_3456u32.to_le_bytes());
    let mut first = ImageList::new(2, [1, 1], &mut images).unwrap();
    let p = ImageListProperties::default();
    assert!(first.set_pixels(0, source, p, &mut images).is_err());
    first.set_pixels(0, pixels, p, &mut images).unwrap();
    images.release_owner(1);
    assert!(images.get(source).is_err());
    let mut second = ImageList::new(3, [1, 1], &mut images).unwrap();
    second.set_pixels(0, pixels, p, &mut images).unwrap();
    first.release(&mut images).unwrap();
    second.update(&mut images).unwrap();
    assert_eq!(
        images.get(second.image()).unwrap().surface().pixel(0, 0),
        Some(0xff12_3456)
    );
    second.remove(0, &mut images).unwrap();
    assert!(second.set_pixels(0, pixels, p, &mut images).is_err());
    second.release(&mut images).unwrap();
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
        let mut list = ImageList::new(owner, [5, 3], runtime.images_mut()).unwrap();
        list.set(
            ImageList::key(painter, 0),
            source,
            ImageListProperties {
                position: [2., 1.],
                ..Default::default()
            },
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
        list.set_offset([-1, 0]);
        assert_eq!(
            list.dirty_rectangles().collect::<Vec<_>>(),
            [katze_system2::graphics::Rect::from_xywh(2, 1, 2, 2)]
        );
        assert!(runtime.update_image_list(&mut list).unwrap());
        assert!(!runtime.update_image_list(&mut list).unwrap());
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
        assert!(!runtime.update_image_list(&mut list).unwrap());
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
        assert!(!runtime.update_image_list(&mut list).unwrap());
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
        let output = images.share(list.image()).unwrap();
        list.release(&mut images).unwrap();
        assert_eq!(
            images.get(output).unwrap().surface().pixels(),
            [edge, replacement, edge].map(u32::to_le_bytes).concat()
        );
        images.release(output).unwrap();
        images.release(alias).unwrap();
    }
}

#[test]
fn incremental_lists_keep_history_and_repeat_blending_in_overlapping_blocks() {
    use katze_system2::graphics::Rect;
    let mut images = Images::default();
    assert!(ImageList::new_tiled(2, [1, 1], [1, 0], &mut images).is_err());
    assert!(ImageList::new_tiled(2, [65536, 65536], [1, 1], &mut images).is_err());
    let mut empty = ImageList::new_tiled(2, [0, i32::MAX as u32], [1, 1], &mut images).unwrap();
    assert!(empty.update(&mut images).unwrap().is_empty());
    empty.release(&mut images).unwrap();
    let source = images.create(1, [3, 2]).unwrap();
    for (pixel, color) in images
        .get_mut(source)
        .unwrap()
        .pixels_mut()
        .chunks_exact_mut(4)
        .zip([
            0x8012_3459u32,
            0x8012_345a,
            0x8012_345b,
            0x8012_3456,
            0x8012_3457,
            0x8012_3458,
        ])
    {
        pixel.copy_from_slice(&color.to_le_bytes());
    }
    let mut list = ImageList::new_tiled(2, [8, 6], [3, 2], &mut images).unwrap();
    let properties = ImageListProperties {
        position: [2., 1.],
        opacity: 128,
        ..Default::default()
    };
    list.set(7, source, properties, &mut images).unwrap();
    let first = [
        Rect::from_xywh(2, 1, 1, 2),
        Rect::from_xywh(3, 1, 2, 2),
        Rect::from_xywh(2, 2, 1, 1),
        Rect::from_xywh(3, 2, 2, 1),
    ];
    assert_eq!(list.dirty_rectangles().collect::<Vec<_>>(), first);
    assert_eq!(list.update(&mut images).unwrap(), first);
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(2, 4),
        Some(0x4012_3456)
    );
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(2, 3),
        Some(0x7111_3358)
    );
    images
        .get_mut(source)
        .unwrap()
        .fill_rect_from_bottom(Rect::from_xywh(0, 0, 3, 2), 0xff77_8899);
    assert!(list.update(&mut images).unwrap().is_empty());
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(2, 4),
        Some(0x4012_3456)
    );
    list.set(7, source, properties, &mut images).unwrap();
    list.update(&mut images).unwrap();
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(2, 4),
        Some(0x8077_8899)
    );
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(2, 3),
        Some(0xc176_8797)
    );
    let moved = ImageListProperties {
        position: [3., 2.],
        ..properties
    };
    list.set(7, source, moved, &mut images).unwrap();
    let history = [
        Rect::from_xywh(2, 1, 1, 2),
        Rect::from_xywh(3, 1, 3, 2),
        Rect::from_xywh(2, 2, 1, 2),
        Rect::from_xywh(3, 2, 3, 2),
    ];
    assert_eq!(list.update(&mut images).unwrap(), history);
    list.set(
        7,
        source,
        ImageListProperties {
            visible: false,
            ..moved
        },
        &mut images,
    )
    .unwrap();
    assert_eq!(list.update(&mut images).unwrap(), history);
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(1, 4),
        Some(0x0012_3456)
    );
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(6, 3),
        Some(0x0077_8899)
    );
    assert!(list.remove(7, &mut images).unwrap());
    assert!(list.update(&mut images).unwrap().is_empty());
    list.clear(&mut images).unwrap();
    assert!(
        images
            .get(list.image())
            .unwrap()
            .surface()
            .pixels()
            .iter()
            .all(|&b| b == 0)
    );
    assert_eq!(list.dirty_rectangles().count(), 9);
    assert_eq!(list.update(&mut images).unwrap().len(), 9);
    assert!(list.update(&mut images).unwrap().is_empty());
    list.release(&mut images).unwrap();
    images.release(source).unwrap();
}
