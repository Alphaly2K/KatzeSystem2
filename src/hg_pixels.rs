use crate::{Error, Result, read};

pub(crate) fn decode(literals: &[u8], commands: &[u8], dimensions: [u32; 2]) -> Result<Vec<u8>> {
    let size = byte_size(dimensions)?;
    let planes = expand_runs(literals, commands, size)?;
    let mut pixels = vec![0; size];
    // 位平面按完整游程缓冲分组；小图允许在可见像素之后保留填充。
    let plane_size = planes.len() / 4;
    for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
        for (channel, value) in pixel.iter_mut().enumerate() {
            let mut packed = 0;
            for plane in 0..4 {
                let pair = (planes[plane * plane_size + index] >> (channel * 2)) & 3;
                packed |= pair << (6 - plane * 2);
            }
            *value = (packed >> 1) ^ 0u8.wrapping_sub(packed & 1);
        }
    }
    let stride = dimensions[0] as usize * 4;
    for index in 4..stride {
        pixels[index] = pixels[index].wrapping_add(pixels[index - 4]);
    }
    for index in stride..size {
        pixels[index] = pixels[index].wrapping_add(pixels[index - stride]);
    }
    Ok(pixels)
}

pub(crate) fn byte_size(dimensions: [u32; 2]) -> Result<usize> {
    if dimensions.contains(&0) {
        return Err(Error::new(0, "HG 像素尺寸为零"));
    }
    let size = dimensions[0]
        .checked_mul(dimensions[1])
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| Error::new(0, "HG 像素尺寸溢出"))?;
    read::sized(size, 0)
}

struct Bits<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Bits<'_> {
    fn bit(&mut self) -> Result<bool> {
        let byte = self
            .bytes
            .get(self.position / 8)
            .ok_or_else(|| Error::new(self.position / 8, "HG 游程命令截断"))?;
        let value = (byte >> (self.position % 8)) & 1;
        self.position += 1;
        Ok(value != 0)
    }

    fn gamma(&mut self) -> Result<usize> {
        let mut zeros = 0;
        while !self.bit()? {
            zeros += 1;
            if zeros >= 32 {
                return Err(Error::new(self.position / 8, "HG 游程长度溢出"));
            }
        }
        let mut value = 1usize;
        for _ in 0..zeros {
            value = (value << 1) | usize::from(self.bit()?);
        }
        Ok(value)
    }
}

fn expand_runs(literals: &[u8], commands: &[u8], minimum_size: usize) -> Result<Vec<u8>> {
    let mut bits = Bits {
        bytes: commands,
        position: 0,
    };
    let mut copy = bits.bit()?;
    let size = bits.gamma()?;
    if size < minimum_size {
        return Err(Error::new(0, "HG 游程输出短于像素长度"));
    }
    if size > read::MAX_BYTES {
        return Err(Error::new(0, "HG 游程输出超过存储上限"));
    }
    if size % 4 != 0 {
        return Err(Error::new(0, "HG 游程输出没有四字节对齐"));
    }
    let mut output = vec![0; size];
    let mut position = 0;
    let mut source = 0;
    while position < size {
        let count = bits.gamma()?;
        let end = position
            .checked_add(count)
            .filter(|&end| end <= size)
            .ok_or_else(|| Error::new(bits.position / 8, "HG 游程超出输出范围"))?;
        if copy {
            output[position..end].copy_from_slice(read::range(literals, source, count)?);
            source += count;
        }
        position = end;
        copy = !copy;
    }
    Ok(output)
}

pub(crate) fn encode(pixels: &[u8], dimensions: [u32; 2]) -> Result<(Vec<u8>, Vec<u8>)> {
    let size = byte_size(dimensions)?;
    if pixels.len() != size {
        return Err(Error::new(0, "HG 像素长度与尺寸不符"));
    }
    let mut deltas = pixels.to_vec();
    let stride = dimensions[0] as usize * 4;
    for i in (stride..size).rev() {
        deltas[i] = deltas[i].wrapping_sub(deltas[i - stride]);
    }
    for i in (4..stride).rev() {
        deltas[i] = deltas[i].wrapping_sub(deltas[i - 4]);
    }
    let padded = size.max(1024);
    let quarter = padded / 4;
    let mut planes = vec![0; padded];
    for (i, pixel) in deltas.chunks_exact(4).enumerate() {
        for (channel, &delta) in pixel.iter().enumerate() {
            let packed = if delta < 128 {
                delta << 1
            } else {
                (!delta << 1) | 1
            };
            for plane in 0..4 {
                planes[plane * quarter + i] |= ((packed >> (6 - plane * 2)) & 3) << (channel * 2);
            }
        }
    }
    let mut bits = BitWriter::default();
    let mut copy = planes[0] != 0;
    bits.bit(copy);
    bits.gamma(padded);
    let mut literals = Vec::new();
    let mut start = 0;
    while start < padded {
        let count = planes[start..]
            .iter()
            .take_while(|&&value| (value != 0) == copy)
            .count();
        bits.gamma(count);
        if copy {
            literals.extend_from_slice(&planes[start..start + count]);
        }
        start += count;
        copy = !copy;
    }
    Ok((literals, bits.bytes))
}

#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    position: usize,
}

impl BitWriter {
    fn bit(&mut self, value: bool) {
        if self.position.is_multiple_of(8) {
            self.bytes.push(0);
        }
        if value {
            *self.bytes.last_mut().unwrap() |= 1 << (self.position % 8);
        }
        self.position += 1;
    }

    fn gamma(&mut self, value: usize) {
        let highest = usize::BITS - value.leading_zeros() - 1;
        for _ in 0..highest {
            self.bit(false);
        }
        self.bit(true);
        for bit in (0..highest).rev() {
            self.bit(value & (1 << bit) != 0);
        }
    }
}
