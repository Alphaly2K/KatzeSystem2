use katze_system2::{
    Result,
    expression::State,
    graphics::{GraphicsFactory, ImageList, ImageListProperties, Images, RenderTarget},
    kcs::{Program, ResourceServices},
    resource::ResourceMode,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
    task::{Command, CommandBuffers},
};
use std::{cell::Cell, io::Write, rc::Rc};

fn hg3(frames: &[(u32, u32, [i32; 2], [i32; 2])]) -> Vec<u8> {
    let compress = |bytes: &[u8]| {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), Default::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    };
    let mut bytes = b"HG-3".to_vec();
    bytes.extend(12u32.to_le_bytes());
    bytes.extend(0x300u32.to_le_bytes());
    for (index, &(id, color, offset, base)) in frames.iter().enumerate() {
        let delta = color
            .to_le_bytes()
            .map(|v| (v << 1) ^ 0u8.wrapping_sub(v >> 7));
        let planes: Vec<_> = [6, 4, 2, 0]
            .into_iter()
            .map(|shift| {
                delta
                    .iter()
                    .enumerate()
                    .fold(0, |p, (i, v)| p | (((v >> shift) & 3) << (2 * i)))
            })
            .collect();
        let data = compress(&planes);
        let control = compress(&[9, 1]);
        let start = bytes.len();
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(id.to_le_bytes());
        bytes.extend(b"stdinfo\0");
        for value in [
            56u32,
            40,
            1,
            1,
            32,
            offset[0] as u32,
            offset[1] as u32,
            10,
            20,
            0,
            base[0] as u32,
            base[1] as u32,
        ] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(b"img0000\0");
        for value in [
            0,
            (24 + data.len() + control.len()) as u32,
            0,
            1,
            data.len() as u32,
            4,
            control.len() as u32,
            2,
        ] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(data);
        bytes.extend(control);
        if index + 1 < frames.len() {
            let next = (bytes.len() - start) as u32;
            bytes[start..start + 4].copy_from_slice(&next.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn raw_sources_keep_all_frames_and_pixel_rebinding_changes_the_current_cached_wrapper() {
    let mut data = hg3(&[
        (9, 0xff11_2233, [0, 0], [0, 0]),
        (2, 0xff22_3344, [1, 0], [1, 0]),
        (7, 0xff33_4455, [0, 0], [0, 0]),
        (u32::MAX, 0xff44_5566, [0, 0], [0, 0]),
        (2, 0xff55_6677, [0, 0], [0, 0]),
    ]);
    let mut images = Images::default();
    let mut list = ImageList::new(1, [8, 3], &mut images).unwrap();
    let properties = ImageListProperties {
        position: [4., 1.],
        ..Default::default()
    };
    list.set_hg3(7, &data, properties, &mut images).unwrap();
    assert!(list.set_source_identity(7, 123));
    assert!(!list.set_source_identity(8, 123));
    data.fill(0);
    list.update(&mut images).unwrap();
    let pixel = |images: &Images, x| images.get(list.image()).unwrap().surface().pixel(x, 1);
    assert_eq!(pixel(&images, 5), Some(0xff22_3344));
    assert!(list.select_frame(7, 7, &mut images).unwrap());
    assert!(list.dirty_rectangles().next().is_none());
    list.update(&mut images).unwrap();
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(5, 1),
        Some(0xff22_3344)
    );
    list.redraw(&mut images).unwrap();
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(4, 1),
        Some(0xff33_4455)
    );
    assert!(!list.select_frame(7, 999, &mut images).unwrap());
    assert!(!list.select_frame(8, 7, &mut images).unwrap());
    assert!(
        list.set_hg3(7, b"invalid", properties, &mut images)
            .is_err()
    );
    assert!(list.dirty_rectangles().next().is_none());
    list.redraw(&mut images).unwrap();
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(4, 1),
        Some(0xff33_4455)
    );

    assert!(list.select_frame(7, u32::MAX, &mut images).unwrap());
    let source = images.create(2, [1, 1]).unwrap();
    let pixels = images.pixel_handle(source).unwrap();
    images
        .get_mut(source)
        .unwrap()
        .pixels_mut()
        .copy_from_slice(&0xff77_8899u32.to_le_bytes());
    list.set(7, source, properties, &mut images).unwrap();
    assert_eq!(list.find_source_identity(123), Some(7));
    images.release(source).unwrap();
    assert!(list.select_frame(7, 7, &mut images).unwrap());
    list.redraw(&mut images).unwrap();
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(4, 1),
        Some(0xff33_4455)
    );
    assert!(list.select_frame(7, u32::MAX, &mut images).unwrap());
    list.redraw(&mut images).unwrap();
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(4, 1),
        Some(0xff77_8899)
    );
    list.remove(7, &mut images).unwrap();
    assert_eq!(list.find_source_identity(123), None);
    assert!(list.set_pixels(8, pixels, properties, &mut images).is_err());
    list.release(&mut images).unwrap();

    let mut list = ImageList::new(1, [8, 3], &mut images).unwrap();
    list.set_hg3(7, &hg3(&[(2, 0, [0; 2], [0; 2])]), properties, &mut images)
        .unwrap();
    let source = images.create(2, [1, 1]).unwrap();
    let pixels = images.pixel_handle(source).unwrap();
    list.set(7, source, properties, &mut images).unwrap();
    list.set_hg3(
        u32::MAX,
        &hg3(&[(2, 0, [0; 2], [0; 2])]),
        properties,
        &mut images,
    )
    .unwrap();
    assert_eq!(list.find_source_identity(0), Some(u32::MAX));
    list.set_source_identity(7, 123);
    list.set_source_identity(u32::MAX, 123);
    assert_eq!(list.find_source_identity(123), Some(u32::MAX));
    list.set_source_identity(u32::MAX, 0);
    assert_eq!(list.find_source_identity(123), Some(7));
    images.release(source).unwrap();
    images.release_owner(1);
    let mut observer = ImageList::new(3, [1, 1], &mut images).unwrap();
    assert!(
        observer
            .set_pixels(0, pixels, properties, &mut images)
            .is_err()
    );
    observer.release(&mut images).unwrap();
}

struct Programs;

impl ProgramLoader for Programs {
    fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
        let mut code = Vec::new();
        let mut ins = |op, args: &[(u8, u32)]| {
            code.push(op);
            for &(flag, value) in args {
                code.push(flag);
                code.extend(value.to_le_bytes());
            }
        };
        if name == b"load" {
            ins(0x25, &[(3, 32)]);
            ins(0x2b, &[(3, 365), (3, 4)]);
            ins(0x26, &[(3, 0)]);
        } else {
            ins(0x25, &[(3, 0x1234_5678)]);
            for (index, slot, x, source) in [
                (622, 5, 0.0f32, (0x13, 0)),
                (623, 5, 1.0, (0x13, 0)),
                (623, 6, 2.0, (0x13, 0)),
                (623, 5, 3.0, (3, 0)),
            ] {
                ins(0x25, &[(0x13, 4)]);
                ins(0x25, &[(3, slot)]);
                ins(0x25, &[source]);
                for arg in [x.to_bits(), 1f32.to_bits(), 0, 0, 0, 0, 0, u32::MAX, 1, 0] {
                    ins(0x25, &[(3, arg)]);
                }
                ins(0x2b, &[(3, index), (3, 52)]);
            }
            ins(0x25, &[(0x13, 0)]);
            ins(0x2b, &[(3, 366), (3, 4)]);
            ins(0x26, &[(3, 8)]);
        }
        code.push(0x3b);
        let mut data = vec![0; 64];
        data[32..39].copy_from_slice(b"frames\0");
        Ok(Some(Program::from_code(&code, data, 256)?))
    }
}

struct Files {
    bytes: Vec<u8>,
    reads: Rc<Cell<usize>>,
}

impl ResourceServices for Files {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(
            (category, name),
            (b"image".as_slice(), b"frames".as_slice())
        );
        Ok(b"frames.hg3".to_vec())
    }

    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(name, b"frames.hg3");
        assert_eq!(mode, ResourceMode::LooseFirst);
        self.reads.set(self.reads.get() + 1);
        Ok(Some(self.bytes.clone()))
    }
}

#[test]
fn runtime_second_source_decodes_raw_handles_and_keeps_pixels_after_native_close() {
    for budget in [1, 4096] {
        let reads = Rc::new(Cell::new(0));
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            GraphicsFactory {
                fallback: KcsFactory::new(Programs),
            },
        );
        runtime.set_budget(budget, 10000);
        runtime.host_mut().surface_size = [8, 3];
        runtime.drawing_mut().target = Some(RenderTarget::new([8, 3]).unwrap());
        runtime.set_resources(Files {
            bytes: hg3(&[
                (7, 0xff11_2233, [0, 0], [0, 0]),
                (2, 0xff44_5566, [0, 0], [0, 0]),
            ]),
            reads: reads.clone(),
        });
        let producer = runtime.create(0, kind::ORDINARY, b"load", b"").unwrap();
        runtime.update(0, 0).unwrap();
        let raw = u32::from_le_bytes(
            runtime
                .object(producer)
                .unwrap()
                .interpreter()
                .unwrap()
                .read(0, 4)
                .unwrap()
                .try_into()
                .unwrap(),
        );
        let list = runtime.create(0, kind::IMAGE_LIST, b"list", b"").unwrap();
        let command = |code, data| Command {
            target: list,
            sender: producer,
            code,
            data,
        };
        runtime
            .dispatch_command(0, command(285, [8, 3, 0]))
            .unwrap();
        let mut record = [
            0,
            raw,
            u32::MAX,
            u32::MAX,
            4f32.to_bits(),
            1f32.to_bits(),
            0,
            0,
            0,
            1f32.to_bits(),
            1f32.to_bits(),
            255,
            1,
            0,
            123,
        ]
        .map(u32::to_le_bytes)
        .concat();
        runtime
            .dispatch_command_with_buffers(
                0,
                command(286, [5, 0, 0]),
                &mut CommandBuffers {
                    data0: None,
                    data1: Some(&mut record),
                    external: None,
                },
            )
            .unwrap();
        let mut slot = [0xa5; 4];
        let found = runtime
            .dispatch_command_with_buffers(
                0,
                command(308, [123, 0, 0]),
                &mut CommandBuffers {
                    data0: None,
                    data1: Some(&mut slot),
                    external: None,
                },
            )
            .unwrap();
        assert_eq!(found, 1);
        assert_eq!(u32::from_le_bytes(slot), 5);
        assert_eq!(
            runtime
                .dispatch_command(0, command(308, [999, 0, 0]))
                .unwrap(),
            0
        );
        let mut output = [0; 4];
        runtime
            .dispatch_command_with_buffers(
                0,
                command(291, [0; 3]),
                &mut CommandBuffers {
                    data0: None,
                    data1: Some(&mut output),
                    external: None,
                },
            )
            .unwrap();
        let output = u32::from_le_bytes(output);
        let consumer = runtime.create(0, kind::ORDINARY, b"close", b"").unwrap();
        runtime
            .object_mut(consumer)
            .unwrap()
            .interpreter_mut()
            .unwrap()
            .write(0, &[raw, list].map(u32::to_le_bytes).concat())
            .unwrap();
        runtime.update(1, 1).unwrap();
        assert!(runtime.image_data(raw).is_err());
        let vm = runtime.object(consumer).unwrap().interpreter().unwrap();
        assert_eq!(vm.read(8, 4).unwrap(), 0x1234_5678u32.to_le_bytes());
        runtime.dispatch_command(0, command(298, [0; 3])).unwrap();
        let surface = runtime.images().get(output).unwrap().surface();
        for (x, pixel) in [0, 0xff44_5566, 0xff44_5566, 0, 0xff44_5566]
            .into_iter()
            .enumerate()
        {
            assert_eq!(surface.pixel(x as u32, 1), Some(pixel));
        }
        assert_eq!(reads.get(), 1);
        assert!(
            runtime
                .dispatch_command_with_buffers(
                    0,
                    command(286, [5, 0, 0]),
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut record),
                        external: None,
                    },
                )
                .is_err()
        );
        runtime.dispatch_command(0, command(298, [0; 3])).unwrap();
        assert_eq!(
            runtime.images().get(output).unwrap().surface().pixel(4, 1),
            Some(0xff44_5566)
        );
        runtime.destroy(list).unwrap();
        assert!(runtime.images().get(output).is_err());
    }
}

fn animation(ops: &[(u32, [i32; 3])]) -> Vec<u8> {
    let mut bytes = b"ANM\0".to_vec();
    bytes.resize(32, 0);
    bytes[8..12].copy_from_slice(&(ops.len() as u32).to_le_bytes());
    for &(opcode, values) in ops {
        bytes.extend(opcode.to_le_bytes());
        for value in values.into_iter().chain([0; 5]) {
            bytes.extend(0u32.to_le_bytes());
            bytes.extend(value.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn animated_lists_freeze_budgeted_time_and_keep_hidden_pixels_until_visible_invalidation() {
    let mut images = Images::default();
    let mut list = ImageList::new_tiled(1, [8, 6], [3, 2], &mut images).unwrap();
    list.set_hg3(
        7,
        &hg3(&[
            (9, 0xffff_0000, [0, 0], [0; 2]),
            (2, 0xff00_00ff, [0, 0], [0; 2]),
            (7, 0xff00_ff00, [1, 1], [0; 2]),
        ]),
        ImageListProperties {
            position: [2., 1.],
            ..Default::default()
        },
        &mut images,
    )
    .unwrap();
    let code = animation(&[
        (12, [128, 0, 0]),
        (14, [999, -999, 0]),
        (0, [7, 3, 3]),
        (13, [0, 0, 0]),
        (0, [9, 2, 2]),
        (13, [1, 0, 0]),
        (0, [2, 4, 4]),
        (0, [999, 3, 3]),
    ]);
    list.load_animation(7, &code).unwrap();
    assert!(list.update_animations(100, 1, &mut images).unwrap());
    list.update(&mut images).unwrap();
    list.set_animation_playing(7, 1).unwrap();
    assert!(!list.update_animations(100, 1, &mut images).unwrap());
    assert!(list.update(&mut images).is_err());
    assert!(list.set_animation_playing(7, 0).is_err());
    let mut completed = false;
    for _ in 0..16 {
        if list.update_animations(999, 1, &mut images).unwrap() {
            completed = true;
            break;
        }
    }
    assert!(completed);
    let state = list.animation(7).unwrap();
    assert_eq!(state.time, 100);
    assert_eq!(state.timeline.as_ref().unwrap().origin, 100);
    assert_eq!(state.display().image, 7);
    drop(state);
    list.update(&mut images).unwrap();
    let painted = images
        .get(list.image())
        .unwrap()
        .surface()
        .pixels()
        .to_vec();
    assert_ne!(
        images.get(list.image()).unwrap().surface().pixel(3, 3),
        Some(0)
    );
    assert!(list.update_animations(103, 32, &mut images).unwrap());
    assert_eq!(list.animation(7).unwrap().display().visible, 0);
    assert!(list.dirty_rectangles().next().is_none());
    list.update(&mut images).unwrap();
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixels(),
        painted
    );
    assert!(list.update_animations(104, 32, &mut images).unwrap());
    assert!(list.update_animations(105, 32, &mut images).unwrap());
    assert!(list.dirty_rectangles().next().is_some());
    list.update(&mut images).unwrap();
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(2, 4),
        Some(0x8000_00ff)
    );
    assert!(list.load_animation(7, b"invalid").is_err());
    assert!(list.animation(7).unwrap().timeline.is_none());
    list.redraw(&mut images).unwrap();
    assert_eq!(
        images.get(list.image()).unwrap().surface().pixel(2, 4),
        Some(0xff00_00ff)
    );
    list.release(&mut images).unwrap();
}

#[test]
fn list_restart_runs_once_across_budget_yields_and_invalidates_hidden_or_unanimated_entries() {
    for budget in [1, 4096] {
        for scenario in 0..4 {
            let reset_frame = scenario & 1 != 0;
            let hidden = scenario & 2 != 0;
            let playing = if hidden { -3 } else { 0 };
            let mut images = Images::default();
            let mut list = ImageList::new(1, [4, 3], &mut images).unwrap();
            let mut frames = vec![
                (2, 0xff00_00ff, [0; 2], [0; 2]),
                (7, 0xff00_ff00, [0; 2], [0; 2]),
            ];
            if reset_frame {
                frames.push((10000, 0xffff_0000, [0; 2], [0; 2]));
            }
            list.set_hg3(
                7,
                &hg3(&frames),
                ImageListProperties {
                    position: [1., 1.],
                    ..Default::default()
                },
                &mut images,
            )
            .unwrap();
            let code = animation(&[
                (16, [0, 1, 0]),
                (13, [i32::from(!hidden), 0, 0]),
                (0, [7, 3, 3]),
                (0, [2, 4, 4]),
            ]);
            list.load_animation(7, &code).unwrap();
            list.set_animation_playing(7, playing).unwrap();
            list.update(&mut images).unwrap();
            assert!(list.dirty_rectangles().next().is_none());
            let mut completed = false;
            for _ in 0..16 {
                if list.restart_animation(7, budget, &mut images).unwrap() {
                    completed = true;
                    break;
                }
                assert!(list.restart_animation(8, budget, &mut images).is_err());
                assert!(list.update(&mut images).is_err());
            }
            assert!(completed);
            let state = list.animation(7).unwrap();
            assert_eq!(state.playing, playing);
            assert!(!state.reset_time_on_update);
            assert_eq!(state.timeline.as_ref().unwrap().interpreter.variables[0], 1);
            assert_eq!(state.display().image, if reset_frame { 10000 } else { 7 });
            assert_eq!(state.display().visible, i32::from(!hidden));
            drop(state);
            assert!(list.dirty_rectangles().next().is_some());
            list.update(&mut images).unwrap();
            let pixel = images
                .get(list.image())
                .unwrap()
                .surface()
                .pixel(1, 1)
                .unwrap();
            assert_eq!(pixel >> 24, if hidden { 0 } else { 255 });
            if !hidden {
                assert_eq!(
                    pixel,
                    if reset_frame {
                        0xffff_0000
                    } else {
                        0xff00_ff00
                    }
                );
            }
            assert!(list.restart_animation(8, budget, &mut images).unwrap());
            assert!(list.dirty_rectangles().next().is_none());
            assert!(list.load_animation(7, b"invalid").is_err());
            assert!(list.restart_animation(7, budget, &mut images).unwrap());
            assert!(list.dirty_rectangles().next().is_some());
            list.release(&mut images).unwrap();
        }
    }
}

#[test]
fn native_image_pack_keeps_shared_frames_until_the_last_list_releases_them() -> Result<()> {
    struct Pack(bool);
    impl ProgramLoader for Pack {
        fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
            let mut code = Vec::new();
            let mut ins = |op, args: &[(u8, u32)]| {
                code.push(op);
                for (flag, value) in args {
                    code.push(*flag);
                    code.extend(value.to_le_bytes());
                }
            };
            for (name, address, count, output) in [(32, 128, 2, 0), (48, 512, 1, 4)] {
                for value in [name, address, count] {
                    ins(0x25, &[(3, value)]);
                }
                ins(0x2b, &[(3, 703), (3, 12)]);
                ins(0x26, &[(3, output)]);
            }
            ins(0x25, &[(0x13, 132)]);
            ins(0x26, &[(3, 8)]);
            ins(0x25, &[(0x13, 516)]);
            ins(0x26, &[(3, 12)]);
            for (native, list, slot, x) in [(707, 16, 3, 1f32), (708, 20, 5, 2f32)] {
                ins(0x25, &[(0x13, list)]);
                ins(0x25, &[(3, slot)]);
                ins(0x25, &[(0x13, 8)]);
                for value in [x.to_bits(), 0, 0, 0, 0, 0, 0, 255, 1, 0] {
                    ins(0x25, &[(3, value)]);
                }
                ins(0x2b, &[(3, native), (3, 52)]);
            }
            for (address, output) in [(704, 112), (728, 116), (752, 120)] {
                ins(0x25, &[(3, address)]);
                ins(0x2b, &[(3, 430), (3, 4)]);
                ins(0x26, &[(3, output)]);
            }
            for name in [704, 728, 752] {
                ins(0x25, &[(0x13, 16)]);
                ins(0x25, &[(3, 3)]);
                ins(0x25, &[(3, name)]);
                ins(0x2b, &[(3, 624), (3, 12)]);
            }
            for (address, count) in [(128, 2), (512, 1)] {
                ins(0x25, &[(3, address)]);
                ins(0x25, &[(3, count)]);
                ins(0x2b, &[(3, 704), (3, 8)]);
            }
            ins(0x25, &[(0x13, 8)]);
            ins(0x25, &[(3, 64)]);
            ins(0x2b, &[(3, 706), (3, 8)]);
            ins(0x26, &[(3, 24)]);
            for (ordinal, output) in [(u32::MAX, 100), (1, 104), (2, 108)] {
                ins(0x25, &[(0x13, 8)]);
                ins(0x25, &[(3, ordinal)]);
                ins(0x2b, &[(3, 709), (3, 8)]);
                ins(0x26, &[(3, output)]);
            }
            code.push(0x3b);
            let mut data = vec![0xa5; 768];
            let name: &[u8] = if self.0 { b"parts,a,b\0" } else { b"shared\0" };
            data[32..32 + name.len()].copy_from_slice(name);
            data[48..54].copy_from_slice(b"still\0");
            let companion: &[u8] = if self.0 {
                b"dir/\x83\x5cA.hg3\0"
            } else {
                b"shared.hg3\0"
            };
            data[704..704 + companion.len()].copy_from_slice(companion);
            data[728..736].copy_from_slice(b"missing\0");
            data[752] = 0;
            Ok(Some(Program::from_code(&code, data, 256)?))
        }
    }
    struct PackFiles(Vec<u8>);
    impl ResourceServices for PackFiles {
        fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            assert_eq!(category, b"image");
            katze_system2::resource::Category {
                prefix: b"image".to_vec(),
                extension: b"hg3".to_vec(),
            }
            .resolve(name)
        }
        fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
            assert_eq!(mode, ResourceMode::LooseFirst);
            Ok(match name {
                b"image/shared.hg3" | b"image/parts_a.hg3.hg3" => Some(self.0.clone()),
                b"image/still.hg3" | b"image/parts.hg3" => {
                    Some(hg3(&[(0, 0xff00_ff00, [0; 2], [0; 2])]))
                }
                b"image/shared.anm" => Some(animation(&[(0, [2, 100, 100])])),
                b"image/panes,1.anm" | b"shared.anm" => {
                    Some(animation(&[(0, [9, 100, 100])]))
                }
                b"image/anmtable.csv" => Some(
                    b"PARTS_A\tignored\r\n\"parts_a\"\t\"panes,1.hg3\"\r\nparts_a\tignored\n\x83\x5ca\tpanes,1.hg3\n".to_vec(),
                ),
                b"" => panic!("an empty animation command must not read a file"),
                _ => None,
            })
        }
    }
    for (budget, grouped) in [(1, false), (4096, true)] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            GraphicsFactory {
                fallback: KcsFactory::new(Pack(grouped)),
            },
        );
        runtime.set_budget(budget, 10000);
        runtime.host_mut().surface_size = [4, 3];
        runtime.drawing_mut().target = Some(RenderTarget::new([4, 3])?);
        runtime.set_resources(PackFiles(hg3(&[
            (9, 0xff12_3456, [0; 2], [0; 2]),
            (2, 0xff98_7654, [0; 2], [0; 2]),
        ])));
        let owner = runtime.create(0, kind::ORDINARY, b"pack", b"")?;
        let first = runtime.create(0, kind::IMAGE_LIST, b"first", b"")?;
        let second = runtime.create(0, kind::IMAGE_LIST, b"second", b"")?;
        let command = |target, code, data| Command {
            target,
            code,
            sender: owner,
            data,
        };
        for list in [first, second] {
            runtime.dispatch_command(0, command(list, 285, [4, 3, 0]))?;
        }
        runtime
            .object_mut(owner)
            .unwrap()
            .interpreter_mut()
            .unwrap()
            .write(16, [first, second].map(u32::to_le_bytes).as_flattened())?;
        runtime.update(0, 0)?;
        let vm = runtime.object(owner).unwrap().interpreter().unwrap();
        let word = |address| -> Result<u32> {
            Ok(u32::from_le_bytes(vm.read(address, 4)?.try_into().unwrap()))
        };
        assert_eq!(
            [word(0)?, word(4)?, word(24)?, word(108)?],
            [1 + u32::from(grouped), 1, 1, 0]
        );
        assert_eq!([word(112)?, word(116)?, word(120)?], [1, 0, 0]);
        assert!(vm.stack_bytes().is_empty());
        assert_eq!(
            vm.read(64, 36)?,
            [0u32, 0, 1, 1, 10, 20, 32, 0, 0]
                .map(u32::to_le_bytes)
                .as_flattened()
        );
        for address in [128, 264, 512] {
            assert_eq!(vm.read(address, 8)?, [0; 8]);
            let label: Option<&str> = match (grouped, address) {
                (true, 128) => Some("image/parts_a.hg3.hg3"),
                (true, 264) => Some("image/parts_0b.hg3"),
                _ => None,
            };
            if let Some(label) = label {
                let bytes: Vec<_> = label
                    .encode_utf16()
                    .chain([0])
                    .flat_map(u16::to_le_bytes)
                    .collect();
                assert_eq!(vm.read(address + 8, bytes.len())?, bytes);
            } else {
                assert_eq!(vm.read(address + 8, 4)?, [0xa5; 4]);
            }
        }
        let (still, early_frame, late_frame) = (word(12)?, word(100)?, word(104)?);
        assert!(runtime.images().get(still).is_err());
        assert_eq!(runtime.images().get(early_frame)?.id(), 2);
        assert_eq!(runtime.images().get(late_frame)?.id(), 9);
        runtime.destroy(owner)?;
        runtime.dispatch_command(0, command(first, 301, [3, 1, 0]))?;
        for list in [first, second] {
            let mut output = [0; 4];
            runtime.dispatch_command_with_buffers(
                0,
                command(list, 291, [0; 3]),
                &mut CommandBuffers {
                    data1: Some(&mut output),
                    ..Default::default()
                },
            )?;
            runtime.dispatch_command(0, command(list, 298, [0; 3]))?;
            let surface = runtime.images().get(u32::from_le_bytes(output))?.surface();
            assert_eq!(surface.pixel(2, 2), Some(0xff12_3456));
            assert_eq!(surface.pixel(1, 2), Some(0));
            runtime.destroy(list)?;
            if list == first {
                assert!(runtime.images().get(early_frame).is_ok());
            }
        }
        assert!(runtime.images().get(early_frame).is_err());
        assert!(runtime.images().get(late_frame).is_err());
    }
    Ok(())
}
