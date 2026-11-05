/// 捕获图像的原地颜色处理；透明度保持不变。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageFilter {
    #[default]
    None,
    Mono,
    Sepia,
    Negative,
}

impl ImageFilter {
    pub(crate) fn parse(name: &[u8]) -> Self {
        if name.eq_ignore_ascii_case(b"mono") {
            Self::Mono
        } else if name.eq_ignore_ascii_case(b"sepia") {
            Self::Sepia
        } else if name.eq_ignore_ascii_case(b"np") {
            Self::Negative
        } else {
            Self::None
        }
    }

    pub fn apply(self, image: &mut super::Image) {
        if self == Self::None {
            return;
        }
        image.filter_revision = image.filter_revision.wrapping_add(1);
        for pixel in image.pixels_mut().chunks_exact_mut(4) {
            match self {
                Self::None => return,
                Self::Negative => {
                    for value in &mut pixel[..3] {
                        *value = !*value;
                    }
                }
                Self::Mono | Self::Sepia => {
                    // 原入口分别截断三个查表分量，再按字节相加。
                    let gray = (f32::from(pixel[2]) * 0.298_912) as u8
                        + (f32::from(pixel[0]) * 0.114_478) as u8
                        + (f32::from(pixel[1]) * 0.586_611) as u8;
                    if self == Self::Mono {
                        pixel[..3].fill(gray);
                    } else {
                        pixel[0] = gray.saturating_sub(35);
                        pixel[1] = gray.saturating_add(5);
                        pixel[2] = gray.saturating_add(30);
                    }
                }
            }
        }
    }
}
