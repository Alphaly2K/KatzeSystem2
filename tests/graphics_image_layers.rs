use katze_system2::{
    graphics::Image,
    hg3::{DecodedFrame, StandardInfo},
};
use std::collections::BTreeMap;

fn layer(size: [u32; 2], offset: [i32; 2], base: [i32; 2], pixels: Vec<u8>) -> Image {
    Image::from_frame(DecodedFrame {
        id: 7,
        info: StandardInfo {
            size,
            bit_depth: 32,
            offset,
            canvas_size: [0, 4],
            base,
            reserved: 1,
        },
        pixels,
        attributes: BTreeMap::from([(1, [size[0]; 5])]),
    })
    .unwrap()
}

#[test]
fn layered_images_union_bounds_align_bases_and_merge_attributes() {
    let first = layer([2, 1], [-1, 2], [3, 4], [10, 20, 30, 255].repeat(2));
    let mut second = layer(
        [1, 2],
        [1, 1],
        [4, 3],
        vec![80, 90, 100, 128, 40, 50, 60, 255],
    );
    second = Image::from_frame(DecodedFrame {
        id: second.id(),
        info: StandardInfo {
            canvas_size: [7, 0],
            ..*second.info()
        },
        pixels: second.surface().pixels().to_vec(),
        attributes: BTreeMap::from([(1, [8; 5]), (2, [9; 5])]),
    })
    .unwrap();
    let layers = [first, second];
    let image = Image::from_layers(&layers).unwrap().unwrap();
    assert_eq!(image.id(), 0);
    assert_eq!(image.info().size, [3, 2]);
    assert_eq!(image.info().offset, [-1, 1]);
    assert_eq!(image.info().canvas_size, [7, 4]);
    assert_eq!(image.info().base, [3, 4]);
    assert_eq!(image.attribute(1), Some([8; 5]));
    assert_eq!(image.attribute(2), Some([9; 5]));
    assert_eq!(
        image.surface().pixels(),
        [
            10, 20, 30, 255, 40, 50, 60, 255, 10, 20, 30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]
    );
    assert_eq!(layers[0].surface().pixels(), [10, 20, 30, 255].repeat(2));
    assert!(Image::from_layers(&[]).unwrap().is_none());
    assert!(
        Image::from_layers(&[Image::new([0, 3]).unwrap()])
            .unwrap()
            .is_none()
    );
    let image = Image::from_layers(&[
        layer([1, 1], [0; 2], [0; 2], vec![10, 20, 30, 255]),
        layer([1, 1], [0; 2], [0; 2], vec![110, 120, 130, 128]),
    ])
    .unwrap()
    .unwrap();
    assert_eq!(image.surface().pixels(), [60, 70, 80, 255]);
}
