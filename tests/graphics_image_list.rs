use katze_system2::{
    Result, Step,
    expression::State,
    fes,
    graphics::{
        GraphicsFactory, Image, ImageList, ImageListProperties, ImageListTransform, Images,
        Projection, RenderTarget, Surface, TextureFilter, Transform,
    },
    hg3::{DecodedFrame, StandardInfo},
    kcs::Program,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, fes::Objects, kind},
};

#[test]
fn alpha_masks_preserve_destination_colors_and_select_bgra_channels() {
    let mut images = Images::default();
    let background = images.create(1, [2, 1]).unwrap();
    images
        .get_mut(background)
        .unwrap()
        .pixels_mut()
        .copy_from_slice(&[0x80123456u32, 0xffabcdef].map(u32::to_le_bytes).concat());
    let mask = images.create(1, [2, 1]).unwrap();
    images
        .get_mut(mask)
        .unwrap()
        .pixels_mut()
        .copy_from_slice(&[0xff800000u32, 0x007fffff].map(u32::to_le_bytes).concat());
    let mut list = ImageList::new(2, [2, 1], &mut images).unwrap();
    list.set(0, background, ImageListProperties::default(), &mut images)
        .unwrap();
    list.set(
        1 << 16,
        mask,
        ImageListProperties {
            opacity: 1,
            ..Default::default()
        },
        &mut images,
    )
    .unwrap();
    for (channel, expected) in [
        (1, [0x00123456u32, 0xffabcdef]),
        (2, [0x00123456, 0xffabcdef]),
        (3, [0x41123456, 0x7fabcdef]),
        (4, [0x80123456, 0x00abcdef]),
    ] {
        images
            .get_mut(mask)
            .unwrap()
            .set_alpha_mask(channel)
            .unwrap();
        list.redraw(&mut images).unwrap();
        assert_eq!(
            images.get(list.image()).unwrap().surface().pixels(),
            expected.map(u32::to_le_bytes).concat()
        );
    }
    assert!(images.get_mut(mask).unwrap().set_alpha_mask(5).is_err());
}

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
        transform: None,
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
fn affine_list_matches_reference_pixels_and_clears_previous_bounds() -> Result<()> {
    let mut images = Images::default();
    let source = images.create(1, [3, 2])?;
    for (pixel, value) in images
        .get_mut(source)?
        .pixels_mut()
        .chunks_exact_mut(4)
        .zip([4, 5, 6, 1, 2, 3])
    {
        pixel.copy_from_slice(&[value, 0, 0, 255]);
    }
    let mut list = ImageList::new(2, [16, 12], &mut images)?;
    let points = |images: &Images, list: &ImageList| -> Result<Vec<_>> {
        let frame = images.get(list.image())?.surface();
        Ok((0..12)
            .flat_map(|y| (0..16).map(move |x| (x, y, frame.pixel(x, 11 - y).unwrap() as u8)))
            .filter(|&(_, _, blue)| blue != 0)
            .collect())
    };
    for (base_point, rotation, scale, expected) in [
        ([0., 0.], 0., [1., 1.], &[(7, 6, 2), (8, 6, 3)][..]),
        ([0., 0.], 0., [2., 0.5], &[]),
        ([0., 0.], 0., [-1., 1.], &[(4, 6, 3), (5, 6, 4), (6, 6, 3)]),
        (
            [0., 0.],
            std::f32::consts::FRAC_PI_2,
            [1., 1.],
            &[(7, 3, 3), (7, 4, 3), (7, 5, 2)],
        ),
        (
            [0.5, 1.75],
            0.45,
            [1.3, 0.7],
            &[(7, 5, 2), (8, 5, 4), (9, 5, 5)],
        ),
        ([0., 0.], 0., [0., 1.], &[(6, 6, 2), (7, 6, 2)]),
    ] {
        list.set(
            0,
            source,
            ImageListProperties {
                position: [6.75, 5.5],
                base_point,
                transform: Some(ImageListTransform { rotation, scale }),
                ..Default::default()
            },
            &mut images,
        )?;
        list.update(&mut images)?;
        assert_eq!(points(&images, &list)?, expected, "{rotation} {scale:?}");
    }
    list.remove(0, &mut images)?;
    list.update(&mut images)?;
    assert!(points(&images, &list)?.is_empty());
    Ok(())
}

#[test]
fn affine_list_skips_channel_masks_and_rgb24_sources() -> Result<()> {
    let mut images = Images::default();
    let background = images.create(1, [16, 12])?;
    let background_color = 0xc113_0b07u32.to_le_bytes().repeat(16 * 12);
    images
        .get_mut(background)?
        .pixels_mut()
        .copy_from_slice(&background_color);
    let pixels = [
        0xff4c_2183u32,
        0x3cc9_50ad,
        0xffb5_0c21,
        0xe9b3_7953,
        0x8080_ff14,
        0xfe7c_ef1b,
    ];
    let bgra = images.create(1, [3, 2])?;
    images
        .get_mut(bgra)?
        .pixels_mut()
        .copy_from_slice(&pixels.map(u32::to_le_bytes).concat());
    let rgb = images.create(1, [3, 2])?;
    *images.get_mut(rgb)? = Image::from_frame(DecodedFrame {
        id: 0,
        info: StandardInfo {
            size: [3, 2],
            bit_depth: 24,
            offset: [0; 2],
            canvas_size: [3, 2],
            reserved: 0,
            base: [0; 2],
        },
        pixels: pixels
            .map(|pixel| (pixel | 0xff00_0000).to_le_bytes())
            .concat(),
        attributes: Default::default(),
    })?;
    images.get_mut(rgb)?.set_alpha_mask(3)?;
    let mut list = ImageList::new(2, [16, 12], &mut images)?;
    list.set(0, background, ImageListProperties::default(), &mut images)?;
    let affine = ImageListProperties {
        position: [6.75, 5.5],
        base_point: [0.5, 1.75],
        opacity: 128,
        transform: Some(ImageListTransform {
            rotation: 0.45,
            scale: [1.3, 0.7],
        }),
        ..Default::default()
    };
    let key = 1 << 16;
    for (source, mask) in [(bgra, 0), (bgra, 1), (bgra, 4), (rgb, 0), (rgb, 3)] {
        images.get_mut(source)?.set_alpha_mask(mask)?;
        list.set(key, source, affine, &mut images)?;
        list.update(&mut images)?;
        assert_eq!(
            images.get(list.image())?.surface().pixels() == background_color,
            source == rgb || mask != 0
        );
    }
    Ok(())
}

#[test]
fn explicit_priorities_sort_signed_then_by_unsigned_key_and_repeat_marks_dirty() {
    let mut images = Images::default();
    let first = images.create(1, [1, 1]).unwrap();
    let second = images.create(1, [1, 1]).unwrap();
    let fill = |images: &mut Images, image, color: u32| {
        images
            .get_mut(image)
            .unwrap()
            .pixels_mut()
            .copy_from_slice(&color.to_le_bytes());
    };
    fill(&mut images, first, 0xff11_2233);
    fill(&mut images, second, 0xff44_5566);
    let mut list = ImageList::new(2, [1, 1], &mut images).unwrap();
    let p = ImageListProperties::default();
    list.set(0xffff_ffff, first, p, &mut images).unwrap();
    list.set(0, second, p, &mut images).unwrap();
    assert_eq!(list.priority(u32::MAX), Some(65535));
    assert_eq!(list.priority(99), None);
    assert!(!list.set_priority(99, 0, &images).unwrap());
    list.update(&mut images).unwrap();
    let pixel = |images: &Images| images.get(list.image()).unwrap().surface().pixel(0, 0);
    assert_eq!(pixel(&images), Some(0xff11_2233));
    list.set_priority(u32::MAX, i32::MIN, &images).unwrap();
    list.update(&mut images).unwrap();
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(0, 0),
        Some(0xff44_5566)
    );
    list.set_priority(0, i32::MIN, &images).unwrap();
    list.set(u32::MAX, first, p, &mut images).unwrap();
    assert_eq!(list.priority(u32::MAX), Some(i32::MIN));
    list.update(&mut images).unwrap();
    fill(&mut images, first, 0xff77_8899);
    assert!(list.update(&mut images).unwrap().is_empty());
    list.set_priority(u32::MAX, i32::MIN, &images).unwrap();
    assert!(!list.update(&mut images).unwrap().is_empty());
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(0, 0),
        Some(0xff77_8899)
    );
    list.set_visible(u32::MAX, false, &images).unwrap();
    list.update(&mut images).unwrap();
    list.set_priority(u32::MAX, 10, &images).unwrap();
    assert!(list.update(&mut images).unwrap().is_empty());
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
                Step::Event(fes::Event::Exit(0)) => break,
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

#[test]
fn clipping_keeps_history_shrinks_and_null_reset_waits_for_new_invalidation() {
    use katze_system2::graphics::Rect;
    let mut images = Images::default();
    let source = images.create(1, [3, 2]).unwrap();
    images
        .get_mut(source)
        .unwrap()
        .fill_rect_from_bottom(Rect::from_xywh(0, 0, 3, 2), 0xff12_3456);
    let mut list = ImageList::new(2, [6, 4], &mut images).unwrap();
    let p = ImageListProperties {
        position: [1., 1.],
        ..Default::default()
    };
    list.set(7, source, p, &mut images).unwrap();
    list.update(&mut images).unwrap();
    let clip = Rect::from_xywh(2, 1, 2, 2);
    assert!(list.set_clip(7, Some(clip), &images).unwrap());
    assert_eq!(
        list.update(&mut images).unwrap(),
        [Rect::from_xywh(1, 1, 3, 2)]
    );
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(1, 1),
        Some(0)
    );
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(2, 1),
        Some(0xff12_3456)
    );
    list.set_clip(7, Some(clip), &images).unwrap();
    assert!(list.dirty_rectangles().next().is_none());
    list.set_clip(7, None, &images).unwrap();
    assert!(list.update(&mut images).unwrap().is_empty());
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(1, 1),
        Some(0)
    );
    list.set(7, source, p, &mut images).unwrap();
    list.update(&mut images).unwrap();
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(1, 1),
        Some(0xff12_3456)
    );
    list.set_clip(7, Some(clip), &images).unwrap();
    list.update(&mut images).unwrap();
    list.set_clip(7, Some(Rect::from_xywh(0, 0, 0, 0)), &images)
        .unwrap();
    assert_eq!(list.update(&mut images).unwrap(), [clip]);
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(1, 1),
        Some(0)
    );
    assert!(!list.set_clip(99, Some(clip), &images).unwrap());
}

#[test]
fn list_opacity_keeps_full_words_until_the_pixel_alpha_lookup() {
    let mut images = Images::default();
    let source = images.create(1, [1, 1]).unwrap();
    images
        .get_mut(source)
        .unwrap()
        .pixels_mut()
        .copy_from_slice(&0x8012_3456u32.to_le_bytes());
    for (opacity, expected) in [
        (0, 0),
        (255, 0x8012_3456),
        (256, 0x8112_3456),
        (511, 0x0112_3456),
        (65536, 0x0012_3456),
        (0x8000_0000, 0x0012_3456),
        (0xffff_ff00, 0x7f12_3456),
        (u32::MAX, 0xff12_3456),
    ] {
        let mut list = ImageList::new(2, [3, 1], &mut images).unwrap();
        list.set(
            7,
            source,
            ImageListProperties {
                position: [1., 0.],
                opacity,
                ..Default::default()
            },
            &mut images,
        )
        .unwrap();
        list.update(&mut images).unwrap();
        let surface = images.get(list.image()).unwrap().surface();
        assert_eq!(surface.pixel(1, 0), Some(expected), "opacity={opacity}");
        assert_eq!(
            surface.pixel(0, 0),
            Some(if opacity == 0 { 0 } else { 0x0012_3456 })
        );
        list.release(&mut images).unwrap();
    }
}
