use katze_system2::graphics::{Images, Rect};

#[test]
fn shared_images_keep_live_pixels_until_the_last_holder_releases() {
    let mut images = Images::default();
    let image = images.create(3, [2, 2]).unwrap();
    let other = images.create(4, [1, 1]).unwrap();
    images.retain(image).unwrap();
    images.retain(image).unwrap();
    images
        .get_mut(image)
        .unwrap()
        .fill_rect_from_bottom(Rect::from_xywh(0, 0, 2, 1), 0xff12_3456);
    images.release_owner(3);
    images.release_owner(3);
    assert_eq!(
        images.get(image).unwrap().surface().pixel(1, 1),
        Some(0xff12_3456)
    );
    images
        .get_mut(image)
        .unwrap()
        .fill_rect_from_bottom(Rect::from_xywh(1, 1, 1, 1), 0x8077_8899);
    images.release_reference(image).unwrap();
    assert_eq!(
        images.get(image).unwrap().surface().pixel(1, 0),
        Some(0x8077_8899)
    );
    images.release_reference(image).unwrap();
    assert!(images.get(image).is_err());
    assert!(images.get(other).is_ok());
    images.release(other).unwrap();
    assert!(images.get(other).is_err());
    images.release_reference(0).unwrap();
    assert!(images.retain(image).is_err());
    assert!(images.release_reference(image).is_err());
}

#[test]
fn explicit_release_detaches_only_the_calling_tasks_ownership() {
    for foreign in [false, true] {
        let mut images = Images::default();
        let image = images.create(7, [1, 1]).unwrap();
        images.retain(image).unwrap();
        images.retain(image).unwrap();
        images
            .release_for(if foreign { 8 } else { 7 }, image)
            .unwrap();
        images.release_owner(8);
        images.release_owner(7);
        images.release_reference(image).unwrap();
        assert_eq!(images.get(image).is_err(), foreign);
        if !foreign {
            images.release_for(7, image).unwrap();
            assert!(images.get(image).is_err());
        }
        images.release_for(7, 0).unwrap();
        assert!(images.release_for(7, image).is_err());
    }
    let mut images = Images::default();
    let image = images.create(7, [1, 1]).unwrap();
    images.retain(image).unwrap();
    images.release(image).unwrap();
    images.release_owner(7);
    assert!(images.get(image).is_ok());
    images.release_reference(image).unwrap();
    assert!(images.get(image).is_err());
}

#[test]
fn separate_wrappers_share_pixels_and_preserve_alias_blending_after_owner_release() {
    let mut images = Images::default();
    let source = images.create(9, [4, 2]).unwrap();
    for (pixel, value) in images
        .get_mut(source)
        .unwrap()
        .pixels_mut()
        .chunks_exact_mut(4)
        .zip(1..=8u32)
    {
        pixel.copy_from_slice(&(0xff00_0000 | value).to_le_bytes());
    }
    let shared = images.share(source).unwrap();
    assert_ne!(source, shared);
    images
        .blend(shared, [1, 0], source, Rect::from_xywh(0, 0, 3, 2))
        .unwrap();
    assert_eq!(
        images.get(source).unwrap().surface().pixels(),
        [1, 1, 1, 1, 5, 5, 5, 5]
            .map(|value| (0xff00_0000u32 | value).to_le_bytes())
            .concat()
    );
    images.release_for(9, source).unwrap();
    assert!(images.get(source).is_err());
    assert!(images.share(source).is_err());
    images.release_owner(9);
    let next = images.share(shared).unwrap();
    images.release(shared).unwrap();
    assert!(images.get(shared).is_err());
    assert_eq!(
        images.get(next).unwrap().surface().pixel(3, 1),
        Some(0xff00_0005)
    );
    images.retain(next).unwrap();
    images.release_reference(next).unwrap();
    images
        .get_mut(next)
        .unwrap()
        .fill_rect_from_bottom(Rect::from_xywh(3, 0, 1, 1), 0x8012_3456);
    assert_eq!(
        images.get(next).unwrap().surface().pixel(3, 1),
        Some(0x8012_3456)
    );
    images.release_reference(next).unwrap();
    assert!(images.get(next).is_err());
}

#[test]
fn empty_wrappers_allocate_on_first_recreation_and_share_only_the_current_binding() {
    let mut images = Images::default();
    let empty = images.create_empty(7).unwrap();
    let shared = images.share(empty).unwrap();
    assert_ne!(empty, 0);
    assert_eq!(images.pixel_handle(empty).unwrap(), 0);
    assert_eq!(images.pixel_handle(shared).unwrap(), 0);
    assert!(images.get(empty).is_err());
    assert!(images.get_mut(empty).is_err());
    assert!(images.recreate(empty, [u32::MAX; 2]).is_err());
    assert_eq!(images.pixel_handle(empty).unwrap(), 0);
    images.recreate(empty, [0, 0]).unwrap();
    assert_ne!(images.pixel_handle(empty).unwrap(), 0);
    assert_eq!(images.get(empty).unwrap().info().size, [0, 0]);
    assert_eq!(images.pixel_handle(shared).unwrap(), 0);
    images.recreate(shared, [1, 2]).unwrap();
    assert!(
        images
            .blend(empty, [0; 2], shared, Rect::from_xywh(0, 0, 1, 2))
            .is_ok()
    );
    images.release_owner(7);
    assert!(images.pixel_handle(empty).is_err());
    assert_eq!(images.get(shared).unwrap().info().size, [1, 2]);
    images.release(shared).unwrap();
    let empty = images.create_empty(8).unwrap();
    images.retain(empty).unwrap();
    images.release_owner(8);
    images.release_reference(empty).unwrap();
    assert!(images.pixel_handle(empty).is_err());
}

#[test]
fn recreation_reuses_either_matching_axis_and_keeps_shared_old_pixels_alive() {
    let mut images = Images::default();
    let image = images.create(7, [3, 2]).unwrap();
    images
        .get_mut(image)
        .unwrap()
        .fill_rect_from_bottom(Rect::from_xywh(0, 0, 3, 2), 0x8012_3456);
    let alias = images.share(image).unwrap();
    let pixels = images.pixel_handle(image).unwrap();
    for size in [[3, 5], [4, 2]] {
        images.recreate(image, size).unwrap();
        assert_eq!(images.get(image).unwrap().info().size, [3, 2]);
        assert_eq!(images.pixel_handle(image).unwrap(), pixels);
        assert_eq!(
            images.get(image).unwrap().surface().pixel(0, 0),
            Some(0x8012_3456)
        );
    }
    images.recreate(image, [4, 5]).unwrap();
    assert_ne!(images.pixel_handle(image).unwrap(), pixels);
    assert_eq!(images.get(image).unwrap().info().size, [4, 5]);
    assert!(
        images
            .get(image)
            .unwrap()
            .surface()
            .pixels()
            .iter()
            .all(|&value| value == 0)
    );
    assert_eq!(images.pixel_handle(alias).unwrap(), pixels);
    assert_eq!(
        images.get(alias).unwrap().surface().pixel(0, 0),
        Some(0x8012_3456)
    );
    let next = images.pixel_handle(image).unwrap();
    assert!(images.recreate(image, [u32::MAX, 2]).is_err());
    assert_eq!(images.pixel_handle(image).unwrap(), next);
    images.recreate(image, [0, 5]).unwrap();
    assert_eq!(images.get(image).unwrap().info().size, [4, 5]);
    images.recreate(image, [0, 0]).unwrap();
    assert_eq!(images.get(image).unwrap().info().size, [0, 0]);
    images.recreate(image, [0, 7]).unwrap();
    assert_eq!(images.get(image).unwrap().info().size, [0, 0]);
    images.release_owner(7);
    assert!(images.get(image).is_err());
    assert!(images.get(alias).is_ok());
    images.release(alias).unwrap();
}
