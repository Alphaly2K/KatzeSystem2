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
fn rectangle_image_union_uses_wrappers_physical_size_and_wrapping_endpoints() {
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
            let empty = rt.images_mut().create_empty(0).unwrap();
            let cases = [
                ([0i32; 4], wrapper, [-2, -3], [-2i32, -3, 1, -1]),
                ([-4, -4, 10, 10], wrapper, [-2, -3], [-4, -4, 10, 10]),
                ([7, 8, 1, 2], wrapper, [0, 0], [0, 0, 3, 2]),
                ([1, 2, 3, 4], wrapper, [i32::MAX, 0], [1, 2, 3, 4]),
                ([8, 7, 2, 1], 0, [10, 20], [0; 4]),
                ([1, 2, 3, 4], zero, [10, 20], [1, 2, 3, 4]),
                ([8, 7, 2, 1], empty, [10, 20], [0; 4]),
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
            call(&mut code, 119, &[wrapper, base + 112, base + 116], None);
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
            assert_eq!(vm.read(base + 112, 8).unwrap(), [3, 0, 0, 0, 2, 0, 0, 0]);
            assert_eq!(vm.read(base + 120, 8).unwrap(), [0xa5; 8]);
            assert!(vm.stack_bytes().is_empty());

            let mut code = Vec::new();
            call(&mut code, 216, &[0, pixels, 0, 0], None);
            code.push(0x3b);
            rt.factory_mut().programs.0 = Program::from_code(&code, vec![0xa5; 16], 64).unwrap();
            let id = rt.create(0, kind::ORDINARY, b"invalid", b"").unwrap();
            assert!(rt.update(20, 1).unwrap_err().message.contains("对象句柄"));
            let vm = rt.object(id).unwrap().interpreter().unwrap();
            assert_eq!(vm.read(0, 16).unwrap(), [0xa5; 16]);
        }
    }
}

#[test]
fn image_adjustment_natives_keep_shared_storage_alpha_and_null_gates() {
    for budget in [1, 4096] {
        for bit_depth in [24, 32] {
            let mut rt = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                State::new(0),
                KcsFactory::new(Loader(Program::from_code(&[], vec![], 0).unwrap())),
            );
            rt.set_budget(budget, 1 << 20);
            let handle = rt.images_mut().create(0, [2, 1]).unwrap();
            *rt.images_mut().get_mut(handle).unwrap() = Image::from_frame(DecodedFrame {
                id: 7,
                info: StandardInfo {
                    size: [2, 1],
                    bit_depth,
                    offset: [0; 2],
                    canvas_size: [2, 1],
                    reserved: 1,
                    base: [1, -1],
                },
                pixels: vec![50, 100, 200, 64, 2, 3, 4, 127],
                attributes: [(1, [3, 4, 5, 6, 9])].into(),
            })
            .unwrap();
            let shared = rt.images_mut().share(handle).unwrap();
            let empty = rt.images_mut().create_empty(0).unwrap();
            let mut code = Vec::new();
            call(&mut code, 376, &[0], None);
            call(&mut code, 705, &[0, u32::MAX, u32::MAX], None);
            call(&mut code, 376, &[u32::MAX, 0], None);
            call(&mut code, 376, &[u32::MAX, u32::MAX], None);
            call(&mut code, 376, &[empty, 4], None);
            call(&mut code, 121, &[0, 1, 2, 3], None);
            call(&mut code, 122, &[0, u32::MAX, u32::MAX], None);
            call(&mut code, 121, &[u32::MAX, 100, 100, 100], None);
            call(&mut code, 121, &[empty, 100, 100, 100], None);
            call(&mut code, 121, &[handle, 50, 125, 200], None);
            call(&mut code, 65, &[], None);
            call(&mut code, 122, &[shared, 4, 1], None);
            call(&mut code, 65, &[], None);
            for mode in 1..=3 {
                call(&mut code, 376, &[shared, mode], None);
                call(&mut code, 65, &[], None);
            }
            call(&mut code, 705, &[shared, 8, 2], None);
            code.push(0x3b);
            rt.factory_mut().programs.0 = Program::from_code(&code, vec![], 128).unwrap();
            let id = rt.create(0, kind::ORDINARY, b"adjust", b"").unwrap();
            rt.update(10, 1).unwrap();
            let image = rt.images().get(shared).unwrap();
            let colors = if bit_depth == 24 {
                [50, 100, 200, 255, 2, 3, 4, 255]
            } else {
                [100, 125, 100, 64, 4, 3, 2, 127]
            };
            assert_eq!(image.surface().pixels(), colors);
            rt.update(20, 1).unwrap();
            let image = rt.images().get(handle).unwrap();
            assert_eq!(image, rt.images().get(shared).unwrap());
            assert_eq!(image.info().size, [4, 1]);
            assert_eq!(image.info().canvas_size, [4, 1]);
            assert_eq!(image.info().base, [2, 0]);
            assert_eq!(image.info().reserved, 1);
            assert_eq!(image.id(), 7);
            assert_eq!(image.attribute(1), Some([6, 4, 10, 6, 9]));
            assert_eq!(&image.surface().pixels()[..4], &colors[..4]);
            assert_eq!(&image.surface().pixels()[12..], &colors[4..]);
            for (step, filtered) in [
                [113, 113, 113, 64, 1, 1, 1, 127],
                [78, 118, 143, 64, 0, 6, 31, 127],
                [155, 130, 155, 64, 251, 252, 253, 127],
            ]
            .into_iter()
            .enumerate()
            {
                rt.images_mut()
                    .get_mut(handle)
                    .unwrap()
                    .pixels_mut()
                    .copy_from_slice(&colors.repeat(2));
                rt.update(30 + step as u32, 1).unwrap();
                let image = rt.images().get(handle).unwrap();
                assert_eq!(image, rt.images().get(shared).unwrap());
                assert_eq!(
                    image.surface().pixels(),
                    if bit_depth == 24 { colors } else { filtered }.repeat(2)
                );
            }
            rt.update(40, 1).unwrap();
            let image = rt.images().get(handle).unwrap();
            assert_eq!(image, rt.images().get(shared).unwrap());
            assert_eq!(image.info().size, [8, 2]);
            assert_eq!(image.info().canvas_size, [8, 2]);
            assert_eq!(image.info().base, [4, 0]);
            assert_eq!(image.attribute(1), Some([12, 8, 20, 12, 9]));
            assert!(
                rt.object(id)
                    .unwrap()
                    .interpreter()
                    .unwrap()
                    .stack_bytes()
                    .is_empty()
            );
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

#[test]
fn ordinary_image_properties_keep_shared_geometry_and_explicit_alpha_cache() {
    for budget in [1, 4096] {
        for bit_depth in [24, 32] {
            let mut factory = KcsFactory::new(Loader(Program::from_code(&[], vec![], 0).unwrap()));
            factory.external = katze_system2::memory::SharedMemory::new(vec![0x5a; 16]).unwrap();
            let mut rt = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                State::new(0),
                factory,
            );
            rt.set_budget(budget, 1 << 20);
            let handle = rt.images_mut().create(0, [2, 2]).unwrap();
            *rt.images_mut().get_mut(handle).unwrap() = Image::from_frame(DecodedFrame {
                id: 7,
                info: StandardInfo {
                    size: [2, 2],
                    bit_depth,
                    offset: [-2, 5],
                    canvas_size: [50, 60],
                    reserved: 0,
                    base: [11, -9],
                },
                pixels: [0x0011_2233u32, 0x0144_5566, 0x7f77_8899, 0xffaa_bbcc]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect(),
                attributes: Default::default(),
            })
            .unwrap();
            let shared = rt.images_mut().share(handle).unwrap();
            let mut code = Vec::new();
            code.extend([0x25, 3]);
            code.extend(0xdead_beefu32.to_le_bytes());
            for (x, y, output) in [((-2i32) as u32, 5, 0), (u32::MAX, 6, 4), (100, 100, 8)] {
                call(&mut code, 370, &[handle, x, y], Some(output));
            }
            call(
                &mut code,
                371,
                &[handle, 0x8000_0001, 0x8000_0003],
                Some(12),
            );
            call(
                &mut code,
                374,
                &[shared, i32::MIN as u32, i32::MAX as u32],
                Some(16),
            );
            call(&mut code, 371, &[handle, 20, 24], Some(28));
            call(&mut code, 377, &[handle], Some(32));
            call(&mut code, 117, &[handle, 0, 0, 2, 2, 0xff12_3456], None);
            call(&mut code, 377, &[shared], Some(36));
            call(&mut code, 378, &[shared, 0], None);
            call(&mut code, 377, &[handle], Some(40));
            call(&mut code, 378, &[handle, u32::MAX], None);
            call(&mut code, 377, &[shared], Some(44));
            call(&mut code, 370, &[0], Some(48));
            call(&mut code, 371, &[0, u32::MAX, u32::MAX], Some(52));
            call(&mut code, 374, &[0], Some(56));
            call(&mut code, 378, &[0], None);
            code.extend([0x26, 3]);
            code.extend(60u32.to_le_bytes());
            call(&mut code, 65, &[], None);
            rt.factory_mut().programs.0 = Program::from_code(&code, vec![0xa5; 80], 128).unwrap();
            let task = rt.create(0, kind::ORDINARY, b"properties", b"").unwrap();
            rt.update(0, 1).unwrap();
            let vm = rt.object(task).unwrap().interpreter().unwrap();
            let word =
                |at| u32::from_le_bytes(vm.read(at, 4).unwrap().as_ref().try_into().unwrap());
            assert_eq!(
                [word(0), word(4), word(8)],
                if bit_depth == 24 {
                    [255; 3]
                } else {
                    [127, 1, 0]
                }
            );
            assert_eq!(
                vm.read(0x8000_0000, 8).unwrap(),
                [0x5a, 11, 0, 0xf7, 0xff, 0xff, 0xff, 0x5a]
            );
            assert_eq!([word(20), word(24)], [i32::MIN as u32, i32::MAX as u32]);
            for at in [12, 16, 28] {
                assert_eq!(word(at), 1);
            }
            assert_eq!(
                [word(32), word(36), word(40), word(44)],
                [
                    u32::from(bit_depth == 32),
                    u32::from(bit_depth == 32),
                    0,
                    u32::from(bit_depth == 32)
                ]
            );
            assert_eq!(
                [word(48), word(52), word(56), word(60)],
                [0, 0, 0, 0xdead_beef]
            );
            assert!(vm.stack_bytes().is_empty());
            let image = rt.images().get(handle).unwrap();
            assert_eq!(image, rt.images().get(shared).unwrap());
            assert_eq!(image.info().base, [i32::MIN, i32::MAX]);
            assert_eq!(image.info().offset, [-2, 5]);
            assert_eq!(image.info().canvas_size, [50, 60]);
            assert!(
                image
                    .surface()
                    .pixels()
                    .chunks_exact(4)
                    .all(|pixel| pixel[3] == 255)
            );
        }
    }
}

#[test]
fn image_mutation_natives_share_pixels_geometry_mask_and_alpha_classification() {
    for budget in [1, 4096] {
        for bit_depth in [24, 32] {
            let mut rt = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                State::new(0),
                KcsFactory::new(Loader(Program::from_code(&[], vec![], 0).unwrap())),
            );
            rt.set_budget(budget, 1 << 20);
            let handle = rt.images_mut().create(0, [2, 2]).unwrap();
            *rt.images_mut().get_mut(handle).unwrap() = Image::from_frame(DecodedFrame {
                id: 7,
                info: StandardInfo {
                    size: [2, 2],
                    bit_depth,
                    offset: [-2, 5],
                    canvas_size: [11, 13],
                    reserved: 1,
                    base: [3, -4],
                },
                pixels: vec![1, 0, 0, 64, 2, 0, 0, 127, 3, 0, 0, 255, 4, 0, 0, 0],
                attributes: [(1, [7, 8, 9, 10, 11])].into(),
            })
            .unwrap();
            let shared = rt.images_mut().share(handle).unwrap();
            let mut code = Vec::new();
            code.extend([0x25, 3]);
            code.extend(0xdead_beefu32.to_le_bytes());
            for native in [558, 564, 641] {
                call(&mut code, native, &[0], None);
            }
            call(&mut code, 378, &[handle, 0], None);
            call(&mut code, 641, &[handle, 100], None);
            call(&mut code, 641, &[shared, 50], None);
            call(&mut code, 558, &[shared, 0], None);
            call(&mut code, 558, &[shared, u32::MAX], None);
            call(&mut code, 564, &[shared, u32::MAX], None);
            call(&mut code, 377, &[handle], Some(4));
            code.extend([0x26, 3]);
            code.extend(0u32.to_le_bytes());
            code.push(0x3b);
            rt.factory_mut().programs.0 = Program::from_code(&code, vec![0; 8], 128).unwrap();
            let task = rt.create(0, kind::ORDINARY, b"mutations", b"").unwrap();
            rt.update(0, 1).unwrap();
            let vm = rt.object(task).unwrap().interpreter().unwrap();
            assert_eq!(
                vm.read(0, 4).unwrap().as_ref(),
                0xdead_beefu32.to_le_bytes()
            );
            assert_eq!(vm.read(4, 4).unwrap(), [0; 4]);
            assert!(vm.stack_bytes().is_empty());
            let image = rt.images().get(handle).unwrap();
            assert_eq!(image, rt.images().get(shared).unwrap());
            assert_eq!(image.info().offset, [11, 6]);
            assert_eq!(image.info().base, [8, 17]);
            assert_eq!(image.info().size, [2, 2]);
            assert_eq!(image.info().canvas_size, [11, 13]);
            assert_eq!(image.info().bit_depth, bit_depth);
            assert_eq!(image.info().reserved, 1);
            assert_eq!(image.id(), 7);
            assert_eq!(image.attribute(1), Some([4, 5, 9, 10, 11]));
            assert_eq!(image.alpha_mask(), u32::MAX);
            let alphas = if bit_depth == 24 {
                [255; 4]
            } else {
                [0, 127, 63, 32]
            };
            assert_eq!(
                image.surface().pixels(),
                [4, 3, 2, 1]
                    .into_iter()
                    .zip(alphas)
                    .flat_map(|(color, alpha)| [color, 0, 0, alpha])
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn ordinary_image_property_errors_keep_written_prefix_and_diagnose_undefined_result() {
    for (native, args, message, first) in [
        (377, vec![0], "未初始化", 0xa5a5_a5a5u32),
        (371, vec![1, 16, 0x7fff_ffff], "越界", 11),
        (370, vec![2, 0, 0], "句柄", 0xa5a5_a5a5),
        (641, vec![2, 100], "句柄", 0xa5a5_a5a5),
    ] {
        let mut code = Vec::new();
        call(&mut code, 374, &[1, 11, (-9i32) as u32], Some(0));
        code.extend([0x25, 3]);
        code.extend(0xdead_beefu32.to_le_bytes());
        call(&mut code, native, &args, None);
        let program = Program::from_code(&code, vec![0xa5; 32], 64).unwrap();
        let mut rt = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Loader(program)),
        );
        assert_eq!(rt.images_mut().create(0, [1, 1]).unwrap(), 1);
        let task = rt.create(0, kind::ORDINARY, b"invalid", b"").unwrap();
        assert!(rt.update(0, 1).unwrap_err().message.contains(message));
        let vm = rt.object(task).unwrap().interpreter().unwrap();
        assert_eq!(vm.read(16, 4).unwrap(), first.to_le_bytes());
        assert_eq!(vm.stack_bytes(), 0xdead_beefu32.to_le_bytes());
    }
}

#[test]
fn region_composition_uses_physical_pixels_record_order_and_shared_feedback() {
    for budget in [1, 4096] {
        for bit_depth in [24, 32] {
            for (source_kind, edges) in [(24, false), (32, false), (0, false), (32, true)] {
                let mut factory =
                    KcsFactory::new(Loader(Program::from_code(&[], vec![], 0).unwrap()));
                factory.external =
                    katze_system2::memory::SharedMemory::new(vec![0xa5; 32]).unwrap();
                let mut rt = Runtime::new(
                    0,
                    kind::ROOT,
                    Box::new(BaseObject::new()),
                    State::new(0),
                    factory,
                );
                rt.set_budget(budget, 1 << 20);
                let target = rt.images_mut().create(0, [4, 3]).unwrap();
                let original: Vec<u32> = (0..12)
                    .map(|i| {
                        if edges {
                            0
                        } else {
                            0x801e_140a + i * 0x0001_0101
                        }
                    })
                    .collect();
                let target_image = Image::from_frame(DecodedFrame {
                    id: 7,
                    info: StandardInfo {
                        size: [4, 3],
                        bit_depth,
                        offset: [17, 19],
                        canvas_size: [50, 60],
                        reserved: 1,
                        base: [11, -9],
                    },
                    pixels: original
                        .iter()
                        .flat_map(|value| value.to_le_bytes())
                        .collect(),
                    attributes: [(1, [2, 3, 4, 5, 6])].into(),
                })
                .unwrap();
                *rt.images_mut().get_mut(target).unwrap() = target_image.clone();
                let shared = rt.images_mut().share(target).unwrap();
                let source_pixels = [0x40aa_bbccu32, 0x80dd_eeff, 0xc011_2233, 0xff44_5566];
                let source = if source_kind == 0 {
                    shared
                } else {
                    let source = rt.images_mut().create(0, [2, 2]).unwrap();
                    *rt.images_mut().get_mut(source).unwrap() = Image::from_frame(DecodedFrame {
                        id: 8,
                        info: StandardInfo {
                            size: [2, 2],
                            bit_depth: source_kind,
                            offset: [-17, 50],
                            canvas_size: [90, 80],
                            reserved: 1,
                            base: [30, 40],
                        },
                        pixels: source_pixels
                            .into_iter()
                            .flat_map(u32::to_le_bytes)
                            .collect(),
                        attributes: Default::default(),
                    })
                    .unwrap();
                    rt.images_mut()
                        .get_mut(source)
                        .unwrap()
                        .set_alpha_mask(2)
                        .unwrap();
                    source
                };
                let physical = rt.images().pixel_handle(target).unwrap();
                let alias = source_kind == 0;
                let mut code = Vec::new();
                code.extend([0x25, 3]);
                code.extend(0xdead_beefu32.to_le_bytes());
                call(&mut code, 120, &[0], None);
                call(&mut code, 120, &[target, 0x8000_0001, 2, 128], Some(160));
                call(&mut code, 65, &[], None);
                let mut data = vec![0xa5; 176];
                let clip = if alias || edges {
                    [1u32, 1, 3, 2]
                } else {
                    [0u32, 0, 4, 3]
                };
                data[128..144].copy_from_slice(
                    &clip
                        .into_iter()
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>(),
                );
                rt.factory_mut().programs.0 = Program::from_code(&code, data.clone(), 128).unwrap();
                let task = rt.create(0, kind::ORDINARY, b"region", b"").unwrap();
                let position = if alias {
                    u32::MAX
                } else if edges {
                    0
                } else {
                    1
                };
                let records = [0, u32::MAX, u32::MAX, source, position, position];
                let record_bytes: Vec<_> = records.into_iter().flat_map(u32::to_le_bytes).collect();
                rt.object_mut(task)
                    .unwrap()
                    .interpreter_mut()
                    .unwrap()
                    .write(0x8000_0001, &record_bytes)
                    .unwrap();
                rt.update(0, 1).unwrap();
                let vm = rt.object(task).unwrap().interpreter().unwrap();
                assert_eq!(vm.read(160, 4).unwrap(), 0xdead_beefu32.to_le_bytes());
                assert_eq!(vm.read(128, 16).unwrap(), &data[128..144]);
                assert_eq!(vm.read(0x8000_0001, 24).unwrap(), record_bytes);
                assert!(vm.stack_bytes().is_empty());
                let mut expected = if alias {
                    original.clone()
                } else if edges {
                    vec![0; 12]
                } else {
                    vec![0xff00_0000; 12]
                };
                if bit_depth == 24 {
                    expected.iter_mut().for_each(|value| *value |= 0xff00_0000);
                }
                if edges {
                    expected[5] = 0xff00_0000;
                    expected[6] = 0xff00_0000;
                    if bit_depth == 32 {
                        expected[4] = 0x00dd_eeff;
                    }
                } else if alias {
                    expected[5] = if bit_depth == 24 {
                        expected[2]
                    } else {
                        0xff00_0000
                    };
                    expected[6] = if bit_depth == 24 {
                        expected[3]
                    } else {
                        0xff00_0000
                    };
                } else if source_kind == 24 {
                    for (index, pixel) in [1, 2, 5, 6].into_iter().zip(source_pixels) {
                        expected[index] = pixel | 0xff00_0000;
                    }
                }
                let image = rt.images().get(target).unwrap();
                assert_eq!(
                    image.surface().pixels(),
                    expected
                        .into_iter()
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>()
                );
                assert_eq!(image.info(), target_image.info());
                assert_eq!(image.attribute(1), target_image.attribute(1));
                assert_eq!(image, rt.images().get(shared).unwrap());
                assert_eq!(rt.images().pixel_handle(target).unwrap(), physical);
            }
        }
    }
}

#[test]
fn region_composition_keeps_completed_clear_and_skips_null_record_coordinates() {
    for count in [1, 2] {
        let mut code = vec![0x25, 3];
        code.extend(0xdead_beefu32.to_le_bytes());
        call(&mut code, 120, &[1, 0x8000_0000, count, 0], None);
        call(&mut code, 65, &[], None);
        let mut factory = KcsFactory::new(Loader(
            Program::from_code(
                &code,
                [0u32, 0, 1, 1]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect(),
                64,
            )
            .unwrap(),
        ));
        factory.external = katze_system2::memory::SharedMemory::new(vec![0; 4]).unwrap();
        let mut rt = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            factory,
        );
        assert_eq!(rt.images_mut().create(0, [1, 1]).unwrap(), 1);
        let task = rt.create(0, kind::ORDINARY, b"prefix", b"").unwrap();
        let result = rt.update(0, 1);
        if count == 1 {
            result.unwrap();
        } else {
            assert!(result.unwrap_err().message.contains("越界"));
        }
        assert_eq!(
            rt.images().get(1).unwrap().surface().pixel(0, 0),
            Some(0xff00_0000)
        );
        let vm = rt.object(task).unwrap().interpreter().unwrap();
        assert_eq!(vm.stack_bytes(), 0xdead_beefu32.to_le_bytes());
    }
}
