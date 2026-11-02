use crate::{Error, Result, read};

/// 下层滚动条的页长、滑块尺寸与数值／像素位置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scroll2Control {
    pub range: [i32; 2],
    pub page: i32,
    pub travel: [i32; 2],
    pub track_extent: i32,
    pub knob_extent: i32,
    pub minimum_extent: i32,
    pub position: i32,
    pub pixel_position: i32,
    pub dragging: bool,
}

impl Default for Scroll2Control {
    fn default() -> Self {
        Self {
            range: [0, 10],
            page: 4,
            travel: [0; 2],
            track_extent: 0,
            knob_extent: 0,
            minimum_extent: 0,
            position: 0,
            pixel_position: 0,
            dragging: false,
        }
    }
}

impl Scroll2Control {
    /// 以页长比例重建滑块，保留有符号比较及 DWORD 乘法回绕。
    pub fn rebuild_geometry(&mut self) -> Result<()> {
        let span = self.range_span();
        if span <= self.page {
            self.travel[1] = self.travel[0];
            self.knob_extent = self.track_extent;
        } else if span > 0 {
            self.knob_extent =
                divide(self.track_extent.wrapping_mul(self.page), span)?.max(self.minimum_extent);
            self.travel[1] = self.track_extent.wrapping_sub(self.knob_extent);
        }
        Ok(())
    }

    /// 先按原始范围限制位置；拖动期间保留像素位置。
    pub fn set_position(&mut self, position: i32) -> Result<()> {
        self.position = position.max(self.range[0]).min(self.range[1]);
        if !self.dragging {
            self.update_pixel_position()?;
        }
        Ok(())
    }

    /// 换算跨度扣除页长，数值与像素均从零起算。
    pub fn update_pixel_position(&mut self) -> Result<()> {
        let span = self.range_span().wrapping_sub(self.page);
        if self.position <= 0 {
            self.position = 0;
            self.pixel_position = 0;
        } else if self.position >= span {
            self.position = span;
            self.pixel_position = self.travel[1];
        } else {
            let step = divide(
                self.track_extent
                    .wrapping_sub(self.knob_extent)
                    .wrapping_mul(100),
                span,
            )?;
            self.pixel_position = step.wrapping_mul(self.position) / 100;
        }
        Ok(())
    }

    /// 按百倍整数精度的半步阈值读回位置；零跨度直接返回零。
    pub fn position_at_pixel(&self) -> Result<i32> {
        let span = self.range_span().wrapping_sub(self.page);
        if span == 0 {
            return Ok(0);
        }
        let step = divide(
            self.track_extent
                .wrapping_sub(self.knob_extent)
                .wrapping_mul(100),
            span,
        )?;
        let pixel = self.pixel_position.wrapping_mul(100);
        if step == 0 {
            return Ok(if pixel >= 0 { span.max(0) } else { 0 });
        }
        let mut threshold = step / 2;
        let mut position = 0;
        while position < span && pixel >= threshold {
            if position as usize >= read::MAX_ITEMS {
                return Err(Error::new(0, "滚动条像素换算超过查询上限"));
            }
            position += 1;
            threshold = threshold.wrapping_add(step);
        }
        Ok(position)
    }

    fn range_span(&self) -> i32 {
        self.range[1].wrapping_sub(self.range[0]).wrapping_abs()
    }
}

fn divide(numerator: i32, denominator: i32) -> Result<i32> {
    numerator
        .checked_div(denominator)
        .ok_or_else(|| Error::new(0, "滚动条范围换算除数为零或商溢出"))
}
