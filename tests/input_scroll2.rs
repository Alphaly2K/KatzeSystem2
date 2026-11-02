use katze_system2::{
    graphics::Image,
    hg3::{DecodedFrame, StandardInfo},
    input::Scroll2Control,
};
use std::collections::BTreeMap;

#[test]
fn scroll_geometry_uses_page_proportions_and_uncapped_minimum_extents() {
    let mut control = Scroll2Control {
        range: [0, 100],
        page: 25,
        track_extent: 80,
        minimum_extent: 9,
        ..Default::default()
    };
    control.rebuild_geometry().unwrap();
    assert_eq!((control.knob_extent, control.travel), (20, [0, 60]));
    control.track_extent = 7;
    control.rebuild_geometry().unwrap();
    assert_eq!((control.knob_extent, control.travel), (9, [0, -2]));
    control.page = 100;
    control.travel[0] = 5;
    control.rebuild_geometry().unwrap();
    assert_eq!((control.knob_extent, control.travel), (7, [5, 5]));
    control.range = [0, 0];
    control.page = -1;
    control.track_extent = 40;
    control.rebuild_geometry().unwrap();
    assert_eq!((control.knob_extent, control.travel), (7, [5, 5]));
    control.range = [i32::MIN, 0];
    control.page = 4;
    control.rebuild_geometry().unwrap();
    assert_eq!((control.knob_extent, control.travel), (40, [5, 5]));
}

#[test]
fn scroll_positions_keep_page_adjusted_precision_and_raw_drag_values() {
    let mut control = Scroll2Control {
        range: [0, 100],
        page: -7,
        track_extent: 100,
        minimum_extent: 7,
        ..Default::default()
    };
    control.rebuild_geometry().unwrap();
    control.set_position(99).unwrap();
    assert_eq!((control.position, control.pixel_position), (99, 85));
    control.range = [5, 13];
    control.page = 4;
    control.dragging = true;
    control.set_position(7).unwrap();
    assert_eq!((control.position, control.pixel_position), (7, 85));
    control.dragging = false;
    control.rebuild_geometry().unwrap();
    control.update_pixel_position().unwrap();
    assert_eq!((control.position, control.pixel_position), (4, 50));
    control.page = 0;
    control.track_extent = 10;
    control.minimum_extent = 0;
    control.range = [5, 8];
    control.rebuild_geometry().unwrap();
    for (pixel, position) in [(1, 0), (2, 1), (5, 2), (8, 2), (9, 3)] {
        control.pixel_position = pixel;
        assert_eq!(control.position_at_pixel().unwrap(), position);
    }
    control.page = 3;
    assert_eq!(control.position_at_pixel().unwrap(), 0);
    control.range = [0, 10_000_000];
    control.pixel_position = 0;
    assert_eq!(control.position_at_pixel().unwrap(), 9_999_997);
}

fn frames() -> BTreeMap<u32, Image> {
    let mut frames = BTreeMap::new();
    for state in 0..4 {
        for (part, [mut width, mut height]) in
            [[3, 4], [2, 3], [4, 2], [3, 3]].into_iter().enumerate()
        {
            width += state % 2;
            height += state / 2;
            let mut pixels = Vec::new();
            for y in 0..height {
                for x in 0..width {
                    pixels.extend([
                        (part * 60 + x * 29 + y * 13 + state * 7) as u8,
                        (x * 35 + y * 41 + state * 23) as u8,
                        (part * 17 + x * 11 + y * 19) as u8,
                        [0, 63, 128, 255][(x + y + state) % 4],
                    ]);
                }
            }
            let id = (state * 100 + part) as u32;
            frames.insert(
                id,
                Image::from_frame(DecodedFrame {
                    id,
                    info: StandardInfo {
                        size: [width as u32, height as u32],
                        bit_depth: 32,
                        offset: [2, 1],
                        canvas_size: [width as u32 + 3, height as u32 + 5],
                        reserved: 1,
                        base: [0; 2],
                    },
                    pixels,
                })
                .unwrap(),
            );
        }
    }
    frames
}

fn pixel_words(image: &Image) -> Vec<u32> {
    image
        .surface()
        .pixels()
        .chunks_exact(4)
        .map(|pixel| u32::from_le_bytes(pixel.try_into().unwrap()))
        .collect()
}

#[test]
fn scroll_images_compose_caps_and_interpolation_with_centered_alpha_edges() {
    let source = frames();
    let control = Scroll2Control {
        knob_extent: 8,
        minimum_extent: 6,
        ..Default::default()
    };
    let images = control.compose_images(&source, false).unwrap();
    assert_eq!(
        images.each_ref().map(|image| image.info().size),
        [[8, 4], [8, 4], [8, 5], [8, 5]]
    );
    assert_eq!(
        pixel_words(&images[0]),
        [
            0x00000000, 0x00000000, 0x003300b4, 0x5f10342b, 0x3f3e23d1, 0x804946ee, 0x004946ee,
            0x00000000, 0x0011003c, 0x3f1c2359, 0x3f13290d, 0xcd2d4d61, 0x80514cde, 0xff5c6ffb,
            0x005c6ffb, 0x00000000, 0x3f242949, 0x802f4c66, 0x8026521a, 0xbf4c629f, 0xff6475eb,
            0x3f2d2395, 0x803846b2, 0xff4369cf, 0x80375256, 0xff427573, 0xff397b27, 0x1f49af52,
            0x3f352985, 0x80404ca2, 0xff4b6fbf, 0x005692dc,
        ]
    );
    assert_eq!(images[0].info().offset, [0; 2]);
    assert_eq!(images[0].info().canvas_size, [0; 2]);
    assert_eq!(source[&3].info().offset, [2, 1]);
}

#[test]
fn vertical_scroll_images_average_large_reductions_and_keep_cross_axis_offsets() {
    let control = Scroll2Control {
        knob_extent: 6,
        minimum_extent: 5,
        ..Default::default()
    };
    let images = control.compose_images(&frames(), true).unwrap();
    assert_eq!(images[2].info().size, [4, 6]);
    assert_eq!(
        pixel_words(&images[2]),
        [
            0x004880a0, 0x3f53a3bd, 0x805ec6da, 0xff69e9f7, 0x72268028, 0x8c31a345, 0xab3547a9,
            0xff3e51df, 0x80112e4a, 0xff1c5167, 0xff4657cf, 0x00517aec, 0xff245757, 0x002f7a74,
            0x005980dc, 0x3f64a3f9, 0x00378064, 0x3f42a381, 0x3f6ca9e9, 0x8077cc06, 0x3f4aa971,
            0x8055cc8e, 0x00000000, 0x00000000,
        ]
    );
}
