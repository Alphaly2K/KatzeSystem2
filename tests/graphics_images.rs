use katze_system2::{
    Result,
    graphics::{DrawingContext, Image, ImageReference, Images, Rect, RenderTarget, Surface},
    hg3::{DecodedFrame, StandardInfo},
    kcs::{Program, ResourceServices},
    resource::{Category, ResourceMode},
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};
use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;

fn frame(id: u32, size: [u32; 2], offset: [i32; 2], pixels: &[u32]) -> DecodedFrame {
    DecodedFrame {
        id,
        info: StandardInfo {
            size,
            bit_depth: 32,
            offset,
            canvas_size: [100, 90],
            reserved: 3,
            base: [-7, 8],
        },
        pixels: pixels.iter().flat_map(|p| p.to_le_bytes()).collect(),
    }
}

fn blob(id: u32, argb: u32) -> Vec<u8> {
    fn compress(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), Default::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }
    let delta = argb
        .to_le_bytes()
        .map(|v| (v << 1) ^ 0u8.wrapping_sub(v >> 7));
    let planes: Vec<u8> = [6, 4, 2, 0]
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
    let mut bytes = b"HG-3".to_vec();
    for value in [12, 0x300, 0, id] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(b"stdinfo\0");
    for value in [56u32, 40, 1, 1, 32, 0, 0, 10, 20, 0, 3, 4] {
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
    bytes
}

#[test]
fn image_names_recompose_extensions_around_the_first_colon() {
    for (input, name, frame) in [
        (b"plain".as_slice(), b"plain".as_slice(), 0),
        (b"a:3.hg3", b"a.hg3", 3),
        (b"a:7.gz.hg3", b"a.hg3", 0),
        (b"a:-0x10.gz.hg3", b"a.hg3", -16),
        (b"a:1.5", b"a.5", 1),
        (b"a.hg3:4", b"a.hg3:4", 4),
        (b"a.b.c:3", b"a.c:3", 3),
        (b"foo.bar:5.hg3", b"foo.hg3", 5),
        (b"dir.x/a:2", b"dir.x/a.", 2),
        (b"d:x/a:3.ext", b"d.ext", 0),
        (b".x:2", b".x:2", 2),
        (b"a:", b"a.", 0),
        (b"a:1:2.hg3", b"a.hg3", 1),
        (b"a:-0x10.hg3", b"a.hg3", -16),
        (b"a:0xffffffff.hg3", b"a.hg3", -1),
        (b"a:+012tail.hg3", b"a.hg3", 12),
        (b"a:\t2.hg3", b"a.hg3", 0),
        (b"a\x81\x5c\\b:2", b"a\x81\x5c\\b.", 2),
        (b"a\0ignored:9", b"a", 0),
    ] {
        assert_eq!(
            ImageReference::parse(input).unwrap(),
            ImageReference {
                name: name.to_vec(),
                frame
            }
        );
    }
    assert!(ImageReference::parse(&[b'a'; 260]).is_err());
}

#[test]
fn image_composition_keeps_first_geometry_and_bottom_origin_zero_alpha_colors() {
    let colors = [
        0xff00_0011,
        0x8000_0022,
        0x0000_0033,
        0x0100_0044,
        0xfe00_0055,
        0xff00_0066,
    ];
    for (offset, expected) in [
        (
            [0, 0],
            [
                [0, 0, 0, 0, 0],
                [0, 0, 0, 0, 0],
                [0x11, 0x22, 0x33, 0x33, 0],
                [0x44, 0x55, 0x66, 0x66, 0],
            ],
        ),
        (
            [-2, -1],
            [
                [0, 0, 0, 0, 0],
                [0, 0, 0, 0, 0],
                [0x11, 0x22, 0x33, 0x33, 0],
                [0x44, 0x55, 0x66, 0x66, 0],
            ],
        ),
        (
            [1, 1],
            [
                [0, 0, 0, 0, 0],
                [0x11, 0x11, 0x22, 0x33, 0x33],
                [0x44, 0x44, 0x55, 0x66, 0x66],
                [0, 0, 0, 0, 0],
            ],
        ),
        (
            [4, 3],
            [
                [0, 0, 0, 0x44, 0x44],
                [0, 0, 0, 0, 0],
                [0, 0, 0, 0, 0],
                [0, 0, 0, 0, 0],
            ],
        ),
        ([-5, 0], [[0; 5]; 4]),
    ] {
        let mut target = Image::from_frame(frame(7, [5, 4], [0, 0], &[0; 20])).unwrap();
        let source = Image::from_frame(frame(9, [3, 2], offset, &colors)).unwrap();
        assert_eq!(target.composite(&source), offset != [-5, 0]);
        assert_eq!(target.id(), 7);
        assert_eq!(target.info(), &frame(7, [5, 4], [0, 0], &[0; 20]).info);
        let expected: Vec<u8> = expected
            .into_iter()
            .flatten()
            .flat_map(u32::to_le_bytes)
            .collect();
        assert_eq!(target.surface().pixels(), expected);
    }
}

type Requests = Rc<RefCell<Vec<Vec<u8>>>>;
struct Files {
    requests: Requests,
}
impl ResourceServices for Files {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(category, b"image");
        Category {
            prefix: b"graphics".to_vec(),
            extension: b"hg3".to_vec(),
        }
        .resolve(name)
    }
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        self.requests.borrow_mut().push(name.to_vec());
        Ok(match name {
            b"graphics/first.hg3" => Some(blob(7, 0)),
            b"graphics/second.hg3" => Some(blob(0, 0xff12_3456)),
            b"graphics/empty.hg3" => Some(Vec::new()),
            b"graphics/bad.hg3" => Some(b"HG-3".to_vec()),
            _ => None,
        })
    }
}

#[test]
fn list_loading_skips_missing_entries_composes_and_releases_only_the_owner() {
    let requests = Requests::default();
    let mut files = Files {
        requests: requests.clone(),
    };
    let mut images = Images::default();
    assert_eq!(images.load(1, b"  ", &mut files).unwrap(), 0);
    let first = images
        .load(
            1,
            b"missing first:999.hg3 empty first:7.hg3 second",
            &mut files,
        )
        .unwrap();
    assert_eq!(images.get(first).unwrap().id(), 7);
    assert_eq!(
        images.get(first).unwrap().surface().pixel(0, 0),
        Some(0x0012_3456)
    );
    let second = images.load(2, b"second second", &mut files).unwrap();
    assert_ne!(first, second);
    assert_eq!(
        images.get(second).unwrap().surface().pixel(0, 0),
        Some(0xff12_3456)
    );
    assert_eq!(images.load(1, b"first\tsecond", &mut files).unwrap(), 0);
    assert_eq!(images.load(1, b"\"first second\"", &mut files).unwrap(), 0);
    assert!(images.load(1, b"bad", &mut files).is_err());
    assert!(
        images
            .blend(u32::MAX, [0, 0], second, Rect::from_xywh(0, 0, 1, 1))
            .is_err()
    );
    assert!(images.get(second).is_ok());
    assert!(
        images
            .blend(second, [0, 0], second, Rect::from_xywh(0, 0, 1, 1))
            .unwrap()
    );
    assert!(!images.blend(u32::MAX, [0, 0], 0, Rect::default()).unwrap());
    images.release_owner(1);
    assert!(images.get(first).is_err());
    assert!(images.get(second).is_ok());
    images.release(0).unwrap();
    assert!(images.release(u32::MAX).is_err());
    images.release(second).unwrap();
    assert!(images.get(second).is_err());
    let third = images.load(1, b"first:7.hg3", &mut files).unwrap();
    assert_ne!(first, third);
    assert_eq!(images.get(third).unwrap().surface().pixel(0, 0), Some(0));
    assert_eq!(
        requests.borrow()[..5],
        [
            b"graphics/missing.hg3".to_vec(),
            b"graphics/first.hg3".to_vec(),
            b"graphics/empty.hg3".to_vec(),
            b"graphics/first.hg3".to_vec(),
            b"graphics/second.hg3".to_vec(),
        ]
    );
}

struct ScratchPrograms;
impl ProgramLoader for ScratchPrograms {
    fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
        let mut code = Vec::new();
        let width = if name == b"empty" { 0 } else { 3 };
        for output in [0, 4] {
            ins(&mut code, 0x25, &[width]);
            ins(&mut code, 0x25, &[2]);
            ins(&mut code, 0x2b, &[112, 8]);
            ins(&mut code, 0x26, &[output]);
        }
        code.extend([0x25, 0x13, 0, 0, 0, 0]);
        for n in [0, 0, 3, 1, 0xff12_3456] {
            ins(&mut code, 0x25, &[n]);
        }
        ins(&mut code, 0x2b, &[117, 24]);
        code.extend([0x25, 0x13, 4, 0, 0, 0]);
        for n in [0, 0, 3, 2] {
            ins(&mut code, 0x25, &[n]);
        }
        code.extend([0x25, 0x13, 0, 0, 0, 0]);
        for n in [0, 0, 3, 2] {
            ins(&mut code, 0x25, &[n]);
        }
        ins(&mut code, 0x2b, &[115, 40]);
        ins(&mut code, 0x25, &[0xabcd_ef01]);
        code.extend([0x25, 0x13, 0, 0, 0, 0]);
        ins(&mut code, 0x2b, &[114, 4]);
        ins(&mut code, 0x25, &[0]);
        ins(&mut code, 0x2b, &[114, 4]);
        ins(&mut code, 0x26, &[8]);
        ins(&mut code, 0x3b, &[]);
        Ok(Some(Program::from_code(&code, vec![0; 64], 256)?))
    }
}

#[test]
fn blank_images_need_no_resources_and_explicit_release_preserves_other_images_and_stack() {
    for budget in [1, 4096] {
        for name in [b"empty".as_slice(), b"painted"] {
            let mut runtime = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                katze_system2::expression::State::new(0),
                KcsFactory::new(ScratchPrograms),
            );
            runtime.set_budget(budget, 1 << 20);
            let id = runtime.create(0, kind::ORDINARY, name, b"").unwrap();
            runtime.update(0, 0).unwrap();
            let vm = runtime.object(id).unwrap().interpreter().unwrap();
            let first = u32::from_le_bytes(vm.read(0, 4).unwrap().try_into().unwrap());
            let second = u32::from_le_bytes(vm.read(4, 4).unwrap().try_into().unwrap());
            assert_ne!(first, 0);
            assert_ne!(first, second);
            assert!(runtime.images().get(first).is_err());
            assert_eq!(vm.read(8, 4).unwrap(), 0xabcd_ef01u32.to_le_bytes());
            assert!(vm.stack_bytes().is_empty());
            let image = runtime.images().get(second).unwrap();
            assert_eq!(image.id(), 0);
            assert_eq!(image.info().bit_depth, 32);
            assert_eq!(image.info().offset, [0; 2]);
            assert_eq!(image.info().canvas_size, [0; 2]);
            assert_eq!(image.info().base, [0; 2]);
            if name == b"empty" {
                assert_eq!(image.info().size, [0, 2]);
                assert!(image.surface().pixels().is_empty());
            } else {
                assert_eq!(image.info().size, [3, 2]);
                assert_eq!(
                    image.surface().pixels(),
                    [0u32, 0, 0, 0xff12_3456, 0xff12_3456, 0xff12_3456]
                        .into_iter()
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>()
                );
            }
            runtime.destroy(id).unwrap();
            assert!(runtime.images().get(second).is_err());
        }
    }
}

fn ins(code: &mut Vec<u8>, op: u8, args: &[u32]) {
    code.push(op);
    for n in args {
        code.push(3);
        code.extend(n.to_le_bytes());
    }
}

struct Programs;
impl ProgramLoader for Programs {
    fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
        let mut code = Vec::new();
        ins(&mut code, 0x25, &[64]);
        ins(&mut code, 0x2b, &[111, 4]);
        ins(&mut code, 0x26, &[0]);
        if name == b"fill" {
            ins(&mut code, 0x25, &[0xdead_beef]);
            code.extend([0x25, 0x13, 0, 0, 0, 0]);
            for n in [0, 0, 1, 1, 0x8044_5566] {
                ins(&mut code, 0x25, &[n]);
            }
            ins(&mut code, 0x2b, &[117, 24]);
            ins(&mut code, 0x26, &[4]);
        }
        ins(&mut code, 0x3b, &[]);
        let mut data = vec![0; 128];
        data[64..75].copy_from_slice(b"first:7.hg3");
        Ok(Some(Program::from_code(&code, data, 128)?))
    }
}

#[test]
fn runtime_load_and_fill_preserve_stack_and_release_images_with_the_task() {
    for budget in [1, 4096] {
        let requests = Requests::default();
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            katze_system2::expression::State::new(0),
            KcsFactory::new(Programs),
        );
        runtime.set_resources(Files {
            requests: requests.clone(),
        });
        runtime.set_budget(budget, 1 << 20);
        let first = runtime.create(0, kind::ORDINARY, b"fill", b"").unwrap();
        let second = runtime.create(0, kind::ORDINARY, b"load", b"").unwrap();
        runtime.update(0, 0).unwrap();
        let value = |id, offset| {
            let vm = runtime.object(id).unwrap().interpreter().unwrap();
            u32::from_le_bytes(vm.read(offset, 4).unwrap().try_into().unwrap())
        };
        let h1 = value(first, 0);
        let h2 = value(second, 0);
        assert_eq!(value(first, 4), 0xdead_beef);
        assert_ne!(h1, h2);
        assert_eq!(
            runtime.images().get(h1).unwrap().surface().pixel(0, 0),
            Some(0x8044_5566)
        );
        assert_eq!(
            runtime.images().get(h2).unwrap().surface().pixel(0, 0),
            Some(0)
        );
        runtime.destroy(first).unwrap();
        assert!(runtime.images().get(h1).is_err());
        assert!(runtime.images().get(h2).is_ok());
        runtime.update(1, 1).unwrap();
        assert_eq!(
            runtime.create(0, kind::ORDINARY, b"load", b"").unwrap(),
            first
        );
        runtime.update(2, 1).unwrap();
        assert!(runtime.images().get(h1).is_err());
        assert_eq!(requests.borrow().len(), 3);
        runtime.destroy(0).unwrap();
        assert!(runtime.images().get(h2).is_err());
    }
}

#[test]
fn image_blending_adds_source_offset_and_clips_both_rectangles_independently() {
    let source = Image::from_frame(frame(
        7,
        [2, 2],
        [-1, 1],
        &[0xff00_0011, 0x8000_0022, 0x0000_0033, 0x0100_0044],
    ))
    .unwrap();
    let mut target = Image::from_frame(frame(9, [5, 4], [9, -8], &[0; 20])).unwrap();
    assert!(target.blend_from(&source, Rect::from_xywh(-1, -1, 3, 3), [2, -1]));
    let expected = [
        [0; 5],
        [0; 5],
        [0x11, 0xff00_0011, 0x8000_0022, 0x22, 0],
        [0x33, 0x33, 0x0100_0044, 0x44, 0],
    ];
    assert_eq!(
        target.surface().pixels(),
        expected
            .into_iter()
            .flatten()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(target.info().offset, [9, -8]);
    assert_eq!(target.id(), 9);
}

#[test]
fn self_blending_reads_previous_writes_in_bottom_origin_row_and_column_order() {
    let mut horizontal = Image::from_frame(frame(
        7,
        [5, 1],
        [0, 0],
        &[
            0xff00_0011,
            0xff00_0022,
            0xff00_0033,
            0xff00_0044,
            0xff00_0055,
        ],
    ))
    .unwrap();
    assert!(horizontal.blend_within(Rect::from_xywh(0, 0, 4, 1), [1, 0]));
    assert_eq!(
        horizontal.surface().pixels(),
        0xff00_0011u32.to_le_bytes().repeat(5)
    );
    let mut vertical = Image::from_frame(frame(
        7,
        [2, 3],
        [0, 0],
        &[
            0xff00_0001,
            0xff00_0002,
            0xff00_0003,
            0xff00_0004,
            0xff00_0005,
            0xff00_0006,
        ],
    ))
    .unwrap();
    assert!(vertical.blend_within(Rect::from_xywh(0, 0, 2, 2), [0, 1]));
    assert_eq!(
        vertical.surface().pixels(),
        [0xff00_0005u32, 0xff00_0006]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>()
            .repeat(3)
    );
}

struct BlitPrograms;
impl ProgramLoader for BlitPrograms {
    fn load(&mut self, _name: &[u8]) -> Result<Option<Program>> {
        let mut code = Vec::new();
        for (address, output) in [(64, 0), (80, 4)] {
            ins(&mut code, 0x25, &[address]);
            ins(&mut code, 0x2b, &[111, 4]);
            ins(&mut code, 0x26, &[output]);
        }
        ins(&mut code, 0x25, &[0xabcd_ef01]);
        code.extend([0x25, 0x13, 0, 0, 0, 0]);
        for n in [0, 0, i32::MAX as u32, 0] {
            ins(&mut code, 0x25, &[n]);
        }
        code.extend([0x25, 0x13, 4, 0, 0, 0]);
        for n in [0, 0, 1, 1] {
            ins(&mut code, 0x25, &[n]);
        }
        ins(&mut code, 0x2b, &[115, 40]);
        ins(&mut code, 0x26, &[8]);
        ins(&mut code, 0x25, &[0x1234_5678]);
        for n in [0, 1, 0, 0, 0] {
            ins(&mut code, 0x25, &[n]);
        }
        code.extend([0x25, 0x13, 0, 0, 0, 0]);
        for n in [0, 0, 1, 1] {
            ins(&mut code, 0x25, &[n]);
        }
        ins(&mut code, 0x2b, &[115, 40]);
        for n in [0x1234, u32::MAX, u32::MAX, u32::MAX, u32::MAX, 0] {
            ins(&mut code, 0x25, &[n]);
        }
        ins(&mut code, 0x2b, &[115, 24]);
        ins(&mut code, 0x26, &[12]);
        ins(&mut code, 0x3b, &[]);
        let mut data = vec![0; 128];
        data[64..75].copy_from_slice(b"first:7.hg3");
        data[80..86].copy_from_slice(b"second");
        Ok(Some(Program::from_code(&code, data, 256)?))
    }
}

#[test]
fn runtime_blits_between_images_and_to_the_current_target_without_scaling_or_stack_values() {
    for budget in [1, 4096] {
        let requests = Requests::default();
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            katze_system2::expression::State::new(0),
            KcsFactory::new(BlitPrograms),
        );
        runtime.set_resources(Files {
            requests: requests.clone(),
        });
        runtime.set_budget(budget, 1 << 20);
        let mut surface = Surface::new([3, 2]).unwrap();
        surface.clear(0xaabb_ccdd);
        *runtime.drawing_mut() = DrawingContext {
            mode: 0,
            enabled: true,
            active: true,
            target: Some(RenderTarget::from_tiles([3, 2], [1, 1], vec![Some(surface)]).unwrap()),
        };
        let id = runtime.create(0, kind::ORDINARY, b"copy", b"").unwrap();
        runtime.update(0, 0).unwrap();
        let vm = runtime.object(id).unwrap().interpreter().unwrap();
        let handle = u32::from_le_bytes(vm.read(0, 4).unwrap().try_into().unwrap());
        assert_eq!(
            vm.read(8, 8).unwrap(),
            [0xabcd_ef01u32, 0x1234_5678]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            runtime.images().get(handle).unwrap().surface().pixel(0, 0),
            Some(0xff12_3456)
        );
        let target = runtime.drawing().target.as_ref().unwrap();
        assert_eq!(target.pixel(1, 0), Some(0xff12_3456));
        assert_eq!(target.pixel(1, 1), Some(0xaabb_ccdd));
        assert_eq!(target.dirty_tiles(), [true]);
        assert_eq!(requests.borrow().len(), 2);
        runtime.destroy(id).unwrap();
        assert!(runtime.images().get(handle).is_err());
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(1, 0),
            Some(0xff12_3456)
        );
    }
}
