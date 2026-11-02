use crate::{Error, Result, read};

/// 下层滑块的数值与像素状态，独立于 FES 保存的原始位置请求。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SliderControl {
    pub range: [i32; 2],
    pub travel: [i32; 2],
    pub position: i32,
    pub pixel_position: i32,
    pub dragging: bool,
}

impl SliderControl {
    pub fn set_range(&mut self, range: [i32; 2]) {
        self.range = range;
    }

    /// 先按有符号端点限制数值；拖动中保留当前像素位置。
    pub fn set_position(&mut self, position: i32) -> Result<()> {
        self.position = position.max(self.range[0]).min(self.range[1]);
        if !self.dragging {
            self.update_pixel_position()?;
        }
        Ok(())
    }

    /// 换算使用范围跨度，不减起点；最终像素也不加 travel 起点。
    pub fn update_pixel_position(&mut self) -> Result<()> {
        let span = self.range[1].wrapping_sub(self.range[0]).wrapping_abs();
        if self.position <= 0 {
            self.position = 0;
            self.pixel_position = 0;
        } else if self.position >= span {
            self.position = span;
            self.pixel_position = self.travel[1];
        } else {
            let numerator = self.travel[1]
                .wrapping_sub(self.travel[0])
                .wrapping_abs()
                .wrapping_mul(self.position)
                .wrapping_mul(100);
            self.pixel_position = divide(numerator, span)? / 100;
        }
        Ok(())
    }

    /// 用半步阈值读回数值；保留百倍整数精度及 DWORD 回绕。
    pub fn position_at_pixel(&self) -> Result<i32> {
        let span = self.range[1].wrapping_sub(self.range[0]).wrapping_abs();
        let step = divide(
            self.travel[1]
                .wrapping_sub(self.travel[0])
                .wrapping_abs()
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
                return Err(Error::new(0, "滑块像素换算超过查询上限"));
            }
            position += 1;
            threshold = threshold.wrapping_add(step);
        }
        Ok(position)
    }
}

fn divide(numerator: i32, denominator: i32) -> Result<i32> {
    numerator
        .checked_div(denominator)
        .ok_or_else(|| Error::new(0, "滑块范围换算除数为零或商溢出"))
}
