use katze_system2::input::Scroll2Control;

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
