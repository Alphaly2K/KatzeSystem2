use katze_system2::input::SliderControl;

#[test]
fn lower_slider_keeps_zero_based_conversion_after_range_clamping_and_freezes_pixels_while_dragging()
{
    let mut control = SliderControl {
        range: [5, 13],
        travel: [5, 13],
        pixel_position: 19,
        dragging: true,
        ..Default::default()
    };
    control.set_position(12).unwrap();
    assert_eq!((control.position, control.pixel_position), (12, 19));
    control.dragging = false;
    control.update_pixel_position().unwrap();
    assert_eq!((control.position, control.pixel_position), (8, 13));
    control.set_position(7).unwrap();
    assert_eq!((control.position, control.pixel_position), (7, 7));
    control.set_range([-5, 5]);
    control.travel = [-5, 5];
    control.set_position(-4).unwrap();
    assert_eq!((control.position, control.pixel_position), (0, 0));
    control.set_range([9, -3]);
    control.set_position(7).unwrap();
    assert_eq!((control.position, control.pixel_position), (0, 0));
    control.set_range([0, 100]);
    control.travel = [0, 0x40000000];
    control.set_position(1).unwrap();
    assert_eq!((control.position, control.pixel_position), (1, 0));
}

#[test]
fn slider_pixel_queries_keep_half_step_thresholds_and_zero_step_endpoints() {
    let mut control = SliderControl {
        range: [5, 8],
        travel: [0, 10],
        ..Default::default()
    };
    for (pixel, expected) in [(1, 0), (2, 1), (5, 2), (8, 2), (9, 3)] {
        control.pixel_position = pixel;
        assert_eq!(control.position_at_pixel().unwrap(), expected);
    }
    control.range = [0, 100];
    control.travel = [0, 1];
    control.pixel_position = 0;
    assert_eq!(control.position_at_pixel().unwrap(), 1);
    control.range = [0, 10_000_000];
    assert_eq!(control.position_at_pixel().unwrap(), 10_000_000);
    control.pixel_position = -1;
    assert_eq!(control.position_at_pixel().unwrap(), 0);
    control.range = [0, 0];
    assert!(control.position_at_pixel().is_err());
}
