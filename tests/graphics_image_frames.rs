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
        let mut ins = |op, args: &[u32]| {
            code.push(op);
            for value in args {
                code.push(3);
                code.extend(value.to_le_bytes());
            }
        };
        if name == b"load" {
            ins(0x25, &[32]);
            ins(0x2b, &[365, 4]);
            ins(0x26, &[0]);
        } else {
            code.extend([0x25, 0x13, 0, 0, 0, 0, 0x2b, 3]);
            code.extend(366u32.to_le_bytes());
            code.push(3);
            code.extend(4u32.to_le_bytes());
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
            .write(0, &raw.to_le_bytes())
            .unwrap();
        runtime.update(1, 1).unwrap();
        assert!(runtime.image_data(raw).is_err());
        runtime.dispatch_command(0, command(298, [0; 3])).unwrap();
        assert_eq!(
            runtime.images().get(output).unwrap().surface().pixel(4, 1),
            Some(0xff44_5566)
        );
        assert_eq!(reads.get(), 1);
        assert!(
            runtime
                .dispatch_command_with_buffers(
                    0,
                    command(286, [5, 0, 0]),
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut record)
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
