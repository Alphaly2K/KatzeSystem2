use std::cell::Cell;
use std::collections::BTreeMap;

use katze_system2::{
    Error, Result,
    graphics::Image,
    hg3::{DecodedFrame, StandardInfo},
    input::{HitMask, InputObject},
    kcs::{Host, Interpreter, NativeReturn},
    runtime::TaskObject,
    task::{Command, CommandBuffers},
};

fn image(size: [u32; 2], canvas_size: [u32; 2], offset: [i32; 2], alpha: &[u8]) -> Image {
    Image::from_frame(DecodedFrame {
        id: 0,
        attributes: Default::default(),
        info: StandardInfo {
            size,
            bit_depth: 32,
            offset,
            canvas_size,
            reserved: 0,
            base: [91, -73],
        },
        pixels: alpha.iter().flat_map(|&a| [0x56, 0x34, 0x12, a]).collect(),
    })
    .unwrap()
}

#[test]
fn image_masks_keep_canvas_fallback_offsets_alpha_and_row_padding() {
    let alpha = [0, 1, 0, 1, 0, 0, 0, 1, 255, 1, 0, 0, 0, 128, 0, 0, 0, 0];
    let mask = HitMask::from_image(&image([9, 2], [10, 0], [1, 0], &alpha)).unwrap();
    assert_eq!(mask.size(), [10, 2]);
    assert_eq!(mask.bytes(), [0x14, 0x03, 0x22, 0]);
    assert!(mask.contains([5, 6], [7, 6]));
    assert!(!mask.contains([5, 6], [6, 6]));
    assert!(mask.contains([5, 6], [14, 6]));
    let clipped = HitMask::from_image(&image([9, 2], [0, 3], [-1, 1], &alpha)).unwrap();
    assert_eq!(clipped.size(), [9, 3]);
    assert_eq!(clipped.bytes(), [0, 0, 0xc5, 0, 0x08, 0]);
    let outside =
        HitMask::from_image(&image([9, 2], [0, 0], [i32::MAX, i32::MIN], &alpha)).unwrap();
    assert_eq!(outside.bytes(), [0; 4]);
    for size in [[0, 3], [9, 0]] {
        let empty = HitMask::from_image(&image(size, [0; 2], [0; 2], &[])).unwrap();
        assert_eq!(empty.size(), size);
        assert!(empty.bytes().is_empty());
        assert!(!empty.contains([0; 2], [0; 2]));
    }
}

struct ImagesHost {
    images: BTreeMap<u32, Image>,
    queries: Cell<usize>,
}

impl Host for ImagesHost {
    fn call(&mut self, _: u32, _: &[u8], _: &mut Interpreter) -> Result<NativeReturn> {
        unreachable!("图像掩码不执行 native")
    }
    fn image(&self, handle: u32) -> Result<&Image> {
        self.queries.set(self.queries.get() + 1);
        self.images
            .get(&handle)
            .ok_or_else(|| Error::new(0, "合成图像句柄无效"))
    }
}

fn send(
    object: &mut InputObject,
    host: &mut ImagesHost,
    code: u32,
    a: u32,
    b: u32,
    buffer: Option<&mut [u8]>,
) -> Result<u32> {
    let mut output = 0x13579bdf;
    let outcome = object.dispatch_command_with_buffers(
        11,
        &Command {
            target: 8,
            sender: 99,
            code,
            data: [a, b, 1],
        },
        &mut CommandBuffers {
            data0: None,
            data1: buffer,
            external: None,
        },
        &mut output,
        host,
        1,
    )?;
    assert!(outcome.done && !outcome.propagate);
    Ok(output)
}

fn words(object: &InputObject) -> [u32; 15] {
    *object
        .input_state()
        .unwrap()
        .regions()
        .get(7)
        .unwrap()
        .words()
}

#[test]
fn image_mask_commands_replace_only_the_first_region_and_keep_failure_side_effects() {
    let mut host = ImagesHost {
        images: BTreeMap::from([
            (1, image([2, 1], [9, 2], [7, 1], &[0, 1])),
            (2, image([1, 1], [0; 2], [0; 2], &[128])),
            (3, image([1, 1], [1_000_000; 2], [0; 2], &[255])),
        ]),
        queries: Cell::new(0),
    };
    let mut object = InputObject::new(8);
    for priority in [9u32, 5] {
        let mut data: Vec<_> = [7, 0, 2, 4, 6, 100, 100, 0, 0, 0, 0x12345678, 0, 0, 0, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        send(&mut object, &mut host, 127, priority, 0, Some(&mut data)).unwrap();
    }
    send(&mut object, &mut host, 154, 7, 17, None).unwrap();
    assert_eq!(
        send(&mut object, &mut host, 147, 999, u32::MAX, None).unwrap(),
        0x13579bdf
    );
    assert_eq!(host.queries.get(), 0);
    assert_eq!(
        send(&mut object, &mut host, 147, 7, 1, None).unwrap(),
        0x13579bdf
    );
    let region = object.input_state().unwrap().regions().get(7).unwrap();
    assert_eq!(&region.words()[11..15], &[1, 9, 2, 17]);
    assert_eq!(region.hit_mask().unwrap().bytes(), [0, 0, 0, 1]);
    assert!(!object.input_state().unwrap().regions().geometry_dirty());
    assert!(
        object.input_state().unwrap().regions().records()[1]
            .hit_mask()
            .is_none()
    );
    host.images.get_mut(&1).unwrap().pixels_mut().fill(0);
    host.images.remove(&1);
    assert_eq!(
        object
            .input_state()
            .unwrap()
            .regions()
            .get(7)
            .unwrap()
            .hit_mask()
            .unwrap()
            .bytes(),
        [0, 0, 0, 1]
    );
    assert!(send(&mut object, &mut host, 147, 7, 99, None).is_err());
    assert_eq!(&words(&object)[11..15], &[0, 9, 2, 17]);
    send(&mut object, &mut host, 147, 7, 0, None).unwrap();
    assert_eq!(&words(&object)[11..15], &[0, 9, 2, 17]);
    assert!(send(&mut object, &mut host, 147, 7, 3, None).is_err());
    assert_eq!(&words(&object)[11..15], &[0, 1_000_000, 1_000_000, 17]);
    send(&mut object, &mut host, 147, 7, 0, None).unwrap();
    assert_eq!(&words(&object)[12..14], &[1_000_000; 2]);
    send(&mut object, &mut host, 147, 7, 2, None).unwrap();
    assert_eq!(&words(&object)[11..15], &[1, 1, 1, 17]);
    send(&mut object, &mut host, 147, 7, 0, None).unwrap();
    assert_eq!(&words(&object)[11..15], &[0, 0, 0, 17]);
    send(&mut object, &mut host, 128, 0, 7, None).unwrap();
    send(&mut object, &mut host, 147, 7, 2, None).unwrap();
    assert_eq!(&words(&object)[11..15], &[1, 1, 1, 1]);
    send(&mut object, &mut host, 3, 0, 0, None).unwrap();
    let queries = host.queries.get();
    assert_eq!(
        send(&mut object, &mut host, 147, 7, u32::MAX, None).unwrap(),
        0x13579bdf
    );
    assert_eq!(host.queries.get(), queries);
    assert_eq!(&words(&object)[11..15], &[1, 1, 1, 1]);
}
