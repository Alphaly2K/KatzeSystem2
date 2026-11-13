use katze_system2::{
    Result,
    graphics::{Image, Images, Rect, Surface, WipeAlphaObject},
    kcs::{Host, Interpreter, NativeReturn},
    runtime::{TaskObject, kind},
    task::{Command, CommandBuffers},
};

#[derive(Default)]
struct Media {
    images: Images,
    submissions: Vec<(Surface, Rect, [i32; 2])>,
}

impl Host for Media {
    fn call(&mut self, _: u32, _: &[u8], _: &mut Interpreter) -> Result<NativeReturn> {
        unreachable!()
    }
    fn image(&self, handle: u32) -> Result<&Image> {
        self.images.get(handle)
    }
    fn submit_message(&mut self, frame: &Surface, rect: Rect, at: [i32; 2]) -> Result<bool> {
        self.submissions.push((frame.clone(), rect, at));
        Ok(true)
    }
}

fn image(media: &mut Media, size: [u32; 2], pixels: &[u32]) -> u32 {
    let handle = media.images.create(0, size).unwrap();
    for (out, word) in media
        .images
        .get_mut(handle)
        .unwrap()
        .pixels_mut()
        .chunks_exact_mut(4)
        .zip(pixels)
    {
        out.copy_from_slice(&word.to_le_bytes());
    }
    handle
}

fn command(code: u32) -> Command {
    Command {
        target: 1,
        sender: 0,
        code,
        data: [0; 3],
    }
}

#[test]
fn alpha_wipe_keeps_source_rgb_and_distinct_natural_and_forced_completion() {
    for reveal in [false, true] {
        for natural in [false, true] {
            let mut media = Media::default();
            let source = image(
                &mut media,
                [3, 2],
                &[
                    0x20010203, 0x40040506, 0x80070809, 0x400a0b0c, 0x800d0e0f, 0xff101112,
                ],
            );
            let mask = image(
                &mut media,
                [2, 2],
                &[0x00000000, 0x000000ff, 0x00ffff40, 0x00000080],
            );
            let mut parameters: Vec<_> =
                [4, source, 0, 0, mask, 2, 3, 3, 2, u32::from(reveal), 1, 0]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect();
            let mut wipe = WipeAlphaObject::new();
            let mut output = 0x12345678;
            wipe.dispatch_command_with_buffers(
                kind::WIPE_ALPHA,
                &command(280),
                &mut CommandBuffers {
                    data1: Some(&mut parameters),
                    ..Default::default()
                },
                &mut output,
                &mut media,
                1,
            )
            .unwrap();
            wipe.state().1.update(10);
            wipe.dispatch_command(kind::WIPE_ALPHA, &command(263), &mut output, &mut media, 1)
                .unwrap();
            wipe.state().1.update(12);
            wipe.update_body(&mut media, 12, 1).unwrap();
            let (frame, rect, at) = media.submissions.last().unwrap();
            assert_eq!(*rect, Rect::from_xywh(1, 0, 2, 2));
            assert_eq!(*at, [3, 3]);
            let alpha = |value| if reveal { value } else { 255 - value };
            assert_eq!(frame.pixel(1, 0), Some((alpha(32) << 24) | 0x000d0e0f));
            assert_eq!(frame.pixel(2, 0), Some((alpha(127) << 24) | 0x00101112));
            assert_eq!(frame.pixel(1, 1), Some((alpha(0) << 24) | 0x00040506));
            assert_eq!(frame.pixel(2, 1), Some((alpha(127) << 24) | 0x00070809));
            media.images.get_mut(source).unwrap().pixels_mut()[16..20]
                .copy_from_slice(&0x10abcdefu32.to_le_bytes());
            if natural {
                wipe.state().1.update(14);
                wipe.update_body(&mut media, 14, 1).unwrap();
            } else {
                wipe.dispatch_command(kind::WIPE_ALPHA, &command(264), &mut output, &mut media, 1)
                    .unwrap();
            }
            assert!(!wipe.is_active());
            assert_eq!(output, 0x12345678);
            let (frame, rect, at) = media.submissions.last().unwrap();
            assert_eq!(*rect, Rect::from_xywh(0, 0, 3, 2));
            assert_eq!(*at, if natural { [2, 3] } else { [3, 3] });
            assert_eq!(
                frame.pixel(1, 0),
                Some((alpha(if natural { 32 } else { 16 }) << 24) | 0x000d0e0f)
            );
            assert_eq!(
                media.images.get(source).unwrap().surface().pixel(1, 1),
                Some(0x10abcdef)
            );
        }
    }
}
