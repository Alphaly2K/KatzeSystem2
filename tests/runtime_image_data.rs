use katze_system2::{
    Result,
    expression::State,
    graphics::{Image, Rect},
    hg3::{DecodedFrame, StandardInfo},
    kcs::{Program, ResourceServices},
    resource::ResourceMode,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};
struct Loader(Program);
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

struct RawPrograms;

fn stored_call(code: &mut Vec<u8>, native: u32, address: u32, args: &[u32], output: Option<u32>) {
    code.extend([0x25, 0x13]);
    code.extend(address.to_le_bytes());
    for &value in args {
        code.extend([0x25, 3]);
        code.extend(value.to_le_bytes());
    }
    code.extend([0x2b, 3]);
    code.extend(native.to_le_bytes());
    code.push(3);
    code.extend(((args.len() as u32 + 1) * 4).to_le_bytes());
    if let Some(output) = output {
        code.extend([0x26, 3]);
        code.extend(output.to_le_bytes());
    }
}

impl ProgramLoader for RawPrograms {
    fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
        let mut code = Vec::new();
        let mut data = vec![0; 128];
        if name == b"load" {
            data[96..100].copy_from_slice(b"art\0");
            call(&mut code, 365, &[96], Some(0));
        } else {
            for (frame, output) in [(7, 4), (7, 8), (9, 12), (8, 16)] {
                stored_call(&mut code, 364, 0, &[frame], Some(output));
            }
            call(&mut code, 364, &[0, 7], Some(20));
            call(&mut code, 65, &[], None);
            stored_call(&mut code, 366, 0, &[], None);
            stored_call(&mut code, 114, 4, &[], None);
            call(&mut code, 65, &[], None);
        }
        code.push(0x3b);
        Ok(Some(Program::from_code(&code, data, 128)?))
    }
}

struct RawFile(Vec<u8>);
impl ResourceServices for RawFile {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!((category, name), (b"image".as_slice(), b"art".as_slice()));
        Ok(name.to_vec())
    }

    fn read(&mut self, _: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        Ok(Some(self.0.clone()))
    }
}

fn raw_fixture(hg3: bool) -> (Vec<u8>, Vec<u8>) {
    let pixels = vec![11, 22, 33, 44, 55, 66, 77, 255, 99, 88, 77, 66, 9, 8, 7, 6];
    let mut hg2 = katze_system2::hg2::encode_frame([2, 2], &pixels).unwrap();
    for (at, value) in [(52, 7u32), (64, (-2i32) as u32), (68, 5), (80, 6), (84, 7)] {
        hg2[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    if !hg3 {
        return (hg2, pixels);
    }
    let word = |at| u32::from_le_bytes(hg2[at..at + 4].try_into().unwrap());
    let mut frames = Vec::new();
    for (id, x) in [(7u32, (-2i32) as u32), (9, 3), (7, 99)] {
        let mut frame = vec![0; 4];
        frame.extend(id.to_le_bytes());
        frame.extend(b"stdinfo\0");
        frame.extend(
            [56, 40, 2, 2, 32, x, 5, 20, 30, 1, 6, 7]
                .into_iter()
                .flat_map(u32::to_le_bytes),
        );
        frame.extend(b"img0000\0");
        frame.extend(
            [
                0,
                (24 + hg2.len() - 88) as u32,
                0,
                2,
                word(32),
                word(36),
                word(40),
                word(44),
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes),
        );
        frame.extend_from_slice(&hg2[88..]);
        frames.push(frame);
    }
    let mut bytes = b"HG-3".to_vec();
    bytes.extend([12u32, 0x300].into_iter().flat_map(u32::to_le_bytes));
    for (i, mut frame) in frames.into_iter().enumerate() {
        if i != 2 {
            let len = frame.len() as u32;
            frame[..4].copy_from_slice(&len.to_le_bytes());
        }
        bytes.extend(frame);
    }
    (bytes, pixels)
}

#[test]
fn raw_decode_selects_frame_ids_and_owns_independent_images_until_release() {
    for budget in [1, 4096] {
        for hg3 in [false, true] {
            let (bytes, pixels) = raw_fixture(hg3);
            let mut rt = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                State::new(0),
                KcsFactory::new(RawPrograms),
            );
            rt.set_budget(budget, 1 << 20);
            rt.set_resources(RawFile(bytes.clone()));
            let loader = rt.create(0, kind::ORDINARY, b"load", b"").unwrap();
            rt.update(0, 0).unwrap();
            let raw = u32::from_le_bytes(
                rt.object(loader)
                    .unwrap()
                    .interpreter()
                    .unwrap()
                    .read(0, 4)
                    .unwrap()
                    .as_ref()
                    .try_into()
                    .unwrap(),
            );
            rt.destroy(loader).unwrap();
            assert_eq!(rt.image_data(raw).unwrap(), bytes);
            let decoder = rt.create(0, kind::ORDINARY, b"decode", b"").unwrap();
            rt.object_mut(decoder)
                .unwrap()
                .interpreter_mut()
                .unwrap()
                .write(0, &raw.to_le_bytes())
                .unwrap();
            rt.update(1, 0).unwrap();
            let vm = rt.object(decoder).unwrap().interpreter().unwrap();
            let words: Vec<_> = vm
                .read(4, 20)
                .unwrap()
                .chunks_exact(4)
                .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
                .collect();
            assert!(vm.stack_bytes().is_empty());
            let (first, second) = (words[0], words[1]);
            assert_ne!(first, second);
            assert_eq!(words[2] != 0, hg3);
            assert_eq!(&words[3..], &[0, 0]);
            for image in [first, second] {
                let image = rt.images().get(image).unwrap();
                assert_eq!(image.id(), 7);
                assert_eq!(image.info().offset, [-2, 5]);
                assert_eq!(image.info().base, [6, 7]);
                assert_eq!(
                    image.info().canvas_size,
                    if hg3 { [20, 30] } else { [2, 2] }
                );
                assert_eq!(image.surface().pixels(), pixels);
            }
            rt.images_mut()
                .get_mut(first)
                .unwrap()
                .fill_rect_from_bottom(Rect::from_xywh(0, 0, 2, 2), 0);
            assert_eq!(rt.images().get(second).unwrap().surface().pixels(), pixels);
            rt.images_mut().retain(first).unwrap();
            let shared = rt.images_mut().share(second).unwrap();
            rt.update(2, 0).unwrap();
            assert!(rt.image_data(raw).is_err());
            assert!(rt.images().get(first).is_ok());
            rt.destroy(decoder).unwrap();
            assert!(rt.images().get(first).is_ok());
            assert!(rt.images().get(second).is_err());
            assert_eq!(rt.images().get(shared).unwrap().surface().pixels(), pixels);
            rt.images_mut().release_reference(first).unwrap();
            rt.images_mut().release_reference(shared).unwrap();
            assert!(rt.images().get(first).is_err());
            assert!(rt.images().get(shared).is_err());
            if hg3 {
                assert!(rt.images().get(words[2]).is_err());
            }
        }
    }
}
fn call(code: &mut Vec<u8>, native: u32, args: &[u32], output: Option<u32>) {
    for &value in args {
        code.extend([0x25, 3]);
        code.extend(value.to_le_bytes());
    }
    code.extend([0x2b, 3]);
    code.extend(native.to_le_bytes());
    code.push(3);
    code.extend((args.len() as u32 * 4).to_le_bytes());
    if let Some(output) = output {
        code.extend([0x26, 3]);
        code.extend(output.to_le_bytes());
    }
}

#[test]
fn rectangle_image_union_uses_pixel_handles_physical_size_and_wrapping_endpoints() {
    for budget in [1, 4096] {
        for base in [0, 0x8000_0000u32] {
            let mut rt = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                State::new(0),
                KcsFactory::new(Loader(Program::from_code(&[], vec![], 0).unwrap())),
            );
            rt.set_budget(budget, 1 << 20);
            let wrapper = rt.images_mut().create(0, [3, 2]).unwrap();
            let pixels = rt.images().pixel_handle(wrapper).unwrap();
            *rt.images_mut().get_mut(wrapper).unwrap() = Image::from_frame(DecodedFrame {
                id: 7,
                info: StandardInfo {
                    size: [3, 2],
                    bit_depth: 32,
                    offset: [-20, 30],
                    canvas_size: [40, 50],
                    reserved: 0,
                    base: [60, 70],
                },
                pixels: vec![0; 24],
                attributes: Default::default(),
            })
            .unwrap();
            let zero = rt.images_mut().create(0, [0, 4]).unwrap();
            let zero_pixels = rt.images().pixel_handle(zero).unwrap();
            let cases = [
                ([0i32; 4], pixels, [-2, -3], [-2i32, -3, 1, -1]),
                ([-4, -4, 10, 10], pixels, [-2, -3], [-4, -4, 10, 10]),
                ([7, 8, 1, 2], pixels, [0, 0], [0, 0, 3, 2]),
                ([1, 2, 3, 4], pixels, [i32::MAX, 0], [1, 2, 3, 4]),
                ([8, 7, 2, 1], 0, [10, 20], [0; 4]),
                ([1, 2, 3, 4], zero_pixels, [10, 20], [1, 2, 3, 4]),
            ];
            let mut memory = vec![0xa5; 128];
            let mut code = Vec::new();
            for (i, (source, image, offset, _)) in cases.iter().enumerate() {
                let at = i * 16;
                let bytes: Vec<_> = source.iter().flat_map(|n| n.to_le_bytes()).collect();
                memory[at..at + 16].copy_from_slice(&bytes);
                call(
                    &mut code,
                    216,
                    &[base + at as u32, *image, offset[0] as u32, offset[1] as u32],
                    None,
                );
            }
            code.push(0x3b);
            rt.factory_mut().programs.0 = Program::from_code(&code, memory.clone(), 128).unwrap();
            let id = rt.create(0, kind::ORDINARY, b"union", b"").unwrap();
            if base != 0 {
                rt.object_mut(id)
                    .unwrap()
                    .interpreter_mut()
                    .unwrap()
                    .replace_external(memory)
                    .unwrap();
            }
            rt.update(10, 1).unwrap();
            let vm = rt.object(id).unwrap().interpreter().unwrap();
            for (i, (_, _, _, expected)) in cases.iter().enumerate() {
                let bytes: Vec<_> = expected.iter().flat_map(|n| n.to_le_bytes()).collect();
                assert_eq!(vm.read(base + i as u32 * 16, 16).unwrap(), bytes);
            }
            assert_eq!(vm.read(base + 96, 32).unwrap(), [0xa5; 32]);
            assert!(vm.stack_bytes().is_empty());

            let mut code = Vec::new();
            call(&mut code, 216, &[0, wrapper, 0, 0], None);
            code.push(0x3b);
            rt.factory_mut().programs.0 = Program::from_code(&code, vec![0xa5; 16], 64).unwrap();
            let id = rt.create(0, kind::ORDINARY, b"invalid", b"").unwrap();
            assert!(rt.update(20, 1).unwrap_err().message.contains("像素句柄"));
            let vm = rt.object(id).unwrap().interpreter().unwrap();
            assert_eq!(vm.read(0, 16).unwrap(), [0xa5; 16]);
        }
    }
}

#[test]
fn raw_image_geometry_and_pixels_keep_bottom_coordinates_overlap_and_shared_storage() {
    let mut code = Vec::new();
    call(&mut code, 363, &[1, 80, 84], None);
    call(&mut code, 362, &[1, 88, 92], None);
    call(&mut code, 1026, &[1, (-2i32) as u32, 5], Some(0));
    call(&mut code, 1026, &[1, (-2i32) as u32, 6], Some(4));
    call(&mut code, 1026, &[1, 1, 5], Some(8));
    call(&mut code, 372, &[1, 40, 0], Some(12));
    call(&mut code, 362, &[1, 96, 100], None);
    call(&mut code, 373, &[1, 10, (-3i32) as u32], Some(16));
    call(&mut code, 363, &[1, 104, 105], None);
    call(&mut code, 372, &[0, u32::MAX, u32::MAX], Some(20));
    call(&mut code, 373, &[0, u32::MAX, u32::MAX], Some(24));
    call(&mut code, 369, &[0, 0], None);
    call(&mut code, 1026, &[0, 0, 0], Some(28));
    call(&mut code, 369, &[1, 0x7f808182], None);
    call(&mut code, 1026, &[1, 10, (-3i32) as u32], Some(32));
    call(&mut code, 1026, &[1, 10, (-2i32) as u32], Some(36));
    code.push(0x3b);
    let program = Program::from_code(&code, vec![0xa5; 128], 128).unwrap();
    for budget in [1, 4096] {
        let mut rt = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Loader(program.clone())),
        );
        rt.set_budget(budget, 1 << 20);
        let image = rt.images_mut().create(0, [3, 2]).unwrap();
        assert_eq!(image, 1);
        let frame = DecodedFrame {
            id: 7,
            info: StandardInfo {
                size: [3, 2],
                bit_depth: 32,
                offset: [-2, 5],
                canvas_size: [0, 20],
                reserved: 0,
                base: [8, 9],
            },
            pixels: [
                0xff010203u32,
                0x804080c0,
                0x12345678,
                0xfff0e0d0,
                0x01020304,
                0x00112233,
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect(),
            attributes: Default::default(),
        };
        *rt.images_mut().get_mut(image).unwrap() = Image::from_frame(frame).unwrap();
        let shared = rt.images_mut().share(image).unwrap();
        let task = rt.create(0, kind::ORDINARY, b"main", b"").unwrap();
        rt.update(0, 0).unwrap();
        let vm = rt.object(task).unwrap().interpreter().unwrap();
        let word = |at| u32::from_le_bytes(vm.read(at, 4).unwrap().as_ref().try_into().unwrap());
        let results: Vec<_> = (0..10).map(|i| word(i * 4)).collect();
        assert_eq!(
            results,
            [
                0xfff0e0d0, 0xff010203, 0, 1, 1, 0, 0, 0, 0x7f78716a, 0x7f000101
            ]
        );
        assert_eq!(
            [word(80), word(84), word(88), word(92), word(96), word(100)],
            [(-2i32) as u32, 5, 3, 20, 40, 2]
        );
        assert_eq!(vm.read(104, 5).unwrap(), [10, 0xfd, 0xff, 0xff, 0xff]);
        assert!(vm.stack_bytes().is_empty());
        let other = rt.images().get(shared).unwrap();
        assert_eq!(other.info().offset, [10, -3]);
        assert_eq!(other.info().canvas_size, [40, 0]);
        assert_eq!(other.info().base, [8, 9]);
        assert_eq!(other.surface().pixel(0, 0), Some(0x7f000101));
        assert_eq!(other.surface().pixel(0, 1), Some(0x7f78716a));
    }
}

#[test]
fn clipped_blending_uses_source_extent_and_keeps_feedback_for_shared_pixels() {
    let make_image = |size: [u32; 2], seed: u32| {
        Image::from_frame(DecodedFrame {
            id: 0,
            info: StandardInfo {
                size,
                bit_depth: 32,
                offset: [0; 2],
                canvas_size: [0; 2],
                reserved: 0,
                base: [0; 2],
            },
            pixels: (0..size[0] * size[1])
                .flat_map(|i| {
                    [
                        (i * 17 + seed) as u8,
                        (i * 37 + seed) as u8,
                        (i * 53 + seed) as u8,
                        [0, 1, 127, 128, 254, 255][i as usize % 6],
                    ]
                })
                .collect(),
            attributes: Default::default(),
        })
        .unwrap()
    };
    for budget in [1, 4096] {
        for mode in 0..3 {
            let mut code = vec![0x25, 3];
            code.extend(0xabcd_ef01u32.to_le_bytes());
            let args = if mode == 0 {
                [1, 1, 1, 2, 2, 3, 0, 0, 3, 3]
            } else {
                [1, 0, 0, 2, 2, if mode == 1 { 1 } else { 3 }, 1, 0, 2, 2]
            };
            call(&mut code, 367, &args, None);
            call(&mut code, 367, &[u32::MAX, 0, 0, 0, 0, 0], Some(0));
            code.push(0x3b);
            let mut rt = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                State::new(0),
                KcsFactory::new(Loader(Program::from_code(&code, vec![0; 64], 64).unwrap())),
            );
            rt.set_budget(budget, 1 << 20);
            let target = rt.images_mut().create(0, [4, 3]).unwrap();
            assert_eq!(target, 1);
            *rt.images_mut().get_mut(target).unwrap() = make_image([4, 3], 19);
            if mode == 0 {
                let source = rt.images_mut().create(0, [3, 4]).unwrap();
                assert_eq!(source, 3);
                let mut image = make_image([3, 4], 71);
                image.set_alpha_mask(4).unwrap();
                *rt.images_mut().get_mut(source).unwrap() = image;
            } else if mode == 2 {
                assert_eq!(rt.images_mut().share(target).unwrap(), 3);
            }
            let task = rt.create(0, kind::ORDINARY, b"blend", b"").unwrap();
            rt.update(0, 0).unwrap();
            let vm = rt.object(task).unwrap().interpreter().unwrap();
            assert_eq!(vm.read(0, 4).unwrap(), 0xabcd_ef01u32.to_le_bytes());
            assert!(vm.stack_bytes().is_empty());
            let expected = if mode == 0 {
                [
                    0x00131313u32,
                    0x01483824,
                    0x7f7c5c34,
                    0xbfd9749f,
                    0xfee7a757,
                    0xff20b0a4,
                    0xfe59b9f1,
                    0xff8ede02,
                    0x7fbb3b9b,
                    0x80f060ac,
                    0xfe2585bd,
                    0xff5aaace,
                ]
            } else {
                [
                    0x00131313, 0x01483824, 0x7f7d5d35, 0x80b28246, 0xff1ccc68, 0xff1ccc68,
                    0x0051f179, 0x0186168a, 0xbfdd53a5, 0xff2584bc, 0xfe2585bd, 0xff5aaace,
                ]
            };
            assert_eq!(
                rt.images().get(target).unwrap().surface().pixels(),
                expected
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn copied_images_keep_independent_pixels_and_blur_shared_wrappers_until_release() {
    let pixels: Vec<_> = (0..6)
        .flat_map(|i| {
            [
                (i * 17 + 19) as u8,
                (i * 37 + 19) as u8,
                (i * 53 + 19) as u8,
                [0, 1, 127, 128, 254, 255][i as usize],
            ]
        })
        .collect();
    let mut code = Vec::new();
    call(&mut code, 118, &[1], Some(0));
    call(&mut code, 118, &[0], Some(4));
    call(&mut code, 119, &[1, 16, 17], None);
    stored_call(&mut code, 119, 0, &[24, 28], None);
    stored_call(&mut code, 362, 0, &[32, 36], None);
    call(&mut code, 65, &[], None);
    stored_call(&mut code, 372, 0, &[9, 8], Some(48));
    stored_call(&mut code, 373, 0, &[2, 3], Some(52));
    code.extend([0x25, 3]);
    code.extend(0xabcd_ef01u32.to_le_bytes());
    stored_call(&mut code, 538, 0, &[1, 2], Some(56));
    stored_call(&mut code, 119, 0, &[24, 28], None);
    stored_call(&mut code, 363, 0, &[40, 44], None);
    call(&mut code, 65, &[], None);
    stored_call(&mut code, 114, 0, &[], None);
    call(&mut code, 65, &[], None);
    code.push(0x3b);
    let program = Program::from_code(&code, vec![0xa5; 128], 128).unwrap();
    for budget in [1, 4096] {
        for explicit in [false, true] {
            let mut rt = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                State::new(0),
                KcsFactory::new(Loader(program.clone())),
            );
            rt.set_budget(budget, 1 << 20);
            assert_eq!(rt.images_mut().create(0, [3, 2]).unwrap(), 1);
            let mut image = Image::from_frame(DecodedFrame {
                id: 7,
                info: StandardInfo {
                    size: [3, 2],
                    bit_depth: 32,
                    offset: [0; 2],
                    canvas_size: [0; 2],
                    reserved: 1,
                    base: [-3, 9],
                },
                pixels: pixels.clone(),
                attributes: [(1, [9, 8, 7, 6, 5])].into(),
            })
            .unwrap();
            image.set_alpha_mask(4).unwrap();
            *rt.images_mut().get_mut(1).unwrap() = image;
            let task = rt.create(0, kind::ORDINARY, b"copy", b"").unwrap();
            rt.update(0, 0).unwrap();
            let vm = rt.object(task).unwrap().interpreter().unwrap();
            let copied = u32::from_le_bytes(vm.read(0, 4).unwrap().as_ref().try_into().unwrap());
            assert_eq!(vm.read(4, 4).unwrap(), [0; 4]);
            assert_eq!(vm.read(16, 5).unwrap(), [3, 2, 0, 0, 0]);
            assert_eq!(
                vm.read(24, 16).unwrap(),
                [3u32, 2, 3, 2]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>()
            );
            assert_eq!(rt.images().get(copied).unwrap().alpha_mask(), 4);
            let shared = rt.images_mut().share(copied).unwrap();
            rt.images_mut().get_mut(1).unwrap().pixels_mut().fill(0);
            assert_eq!(rt.images().get(copied).unwrap().surface().pixels(), pixels);
            rt.update(1, 0).unwrap();
            let vm = rt.object(task).unwrap().interpreter().unwrap();
            assert_eq!(vm.read(56, 4).unwrap(), 0xabcd_ef01u32.to_le_bytes());
            assert!(vm.stack_bytes().is_empty());
            let image = rt.images().get(shared).unwrap();
            assert_eq!(image.info().size, [5, 6]);
            assert_eq!(image.info().canvas_size, [9, 8]);
            assert_eq!(image.info().offset, [1, 1]);
            assert_eq!(image.info().base, [-2, 11]);
            assert_eq!(image.attribute(1), Some([9, 8, 7, 6, 5]));
            let expected = [
                0u32, 0, 0x04654c2d, 0x07684e2e, 0x047d5d35, 0x04b28246, 0x0ecb944e, 0x20857e44,
                0x23688748, 0x0e4c944e, 0x07b28246, 0x1ccb944e, 0x3c878246, 0x40678a49, 0x194b944e,
                0x0bb28246, 0x2bcb944e, 0x518b8748, 0x4e67904c, 0x1c49974f, 0x07b28246, 0x1ccb944e,
                0x35888347, 0x32688d4a, 0x124a964f, 0x04b28246, 0x0ecb944e, 0x19a2ab59, 0x1568be61,
                0x071ccc68,
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
            assert_eq!(image.surface().pixels(), expected);
            if explicit {
                rt.update(2, 0).unwrap();
                assert!(rt.images().get(copied).is_err());
            }
            rt.destroy(task).unwrap();
            assert!(rt.images().get(copied).is_err());
            assert_eq!(
                rt.images().get(shared).unwrap().surface().pixels(),
                expected
            );
            rt.images_mut().release_reference(shared).unwrap();
            assert!(rt.images().get(shared).is_err());
        }
    }
}
