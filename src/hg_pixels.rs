use crate::{Error, Result, read};

const PARALLEL_INFLATE_MIN: usize = 32 * 1024;

pub(crate) fn inflate_streams(
    input: &[u8],
    offset: usize,
    literal_length: usize,
    literal_size: usize,
    command_length: usize,
    command_size: usize,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let literal_bytes = read::range(input, offset, literal_length)?;
    // 即使命令范围无效，也先返回字面量流的错误。
    let command_bytes = read::range(input, offset + literal_length, command_length);
    let parallel_command = command_bytes.as_ref().ok().filter(|bytes| {
        literal_bytes.len() >= PARALLEL_INFLATE_MIN
            && bytes.len() >= PARALLEL_INFLATE_MIN
            && std::thread::available_parallelism().is_ok_and(|count| count.get() > 1)
    });
    if let Some(command_bytes) = parallel_command {
        return std::thread::scope(|scope| {
            let commands = std::thread::Builder::new()
                .spawn_scoped(scope, || read::inflate(command_bytes, command_size));
            let literals = read::inflate(literal_bytes, literal_size)?;
            let commands = match commands {
                Ok(worker) => worker.join().unwrap(),
                Err(_) => read::inflate(command_bytes, command_size),
            }?;
            Ok((literals, commands))
        });
    }
    let literals = read::inflate(literal_bytes, literal_size)?;
    let commands = read::inflate(command_bytes?, command_size)?;
    Ok((literals, commands))
}

pub(crate) fn decode(literals: &[u8], commands: &[u8], dimensions: [u32; 2]) -> Result<Vec<u8>> {
    decode_channels(literals, commands, dimensions, 4)
}

pub(crate) fn decode_channels(
    literals: &[u8],
    commands: &[u8],
    dimensions: [u32; 2],
    channels: usize,
) -> Result<Vec<u8>> {
    let size = channel_byte_size(dimensions, channels)?;
    let planes = expand_runs(literals, commands, size)?;
    let mut pixels = vec![0; size];
    // 位平面按完整游程缓冲分组；小图允许在可见像素之后保留填充。
    let plane_size = planes.len() / 4;
    let (a, rest) = planes.split_at(plane_size);
    let (b, rest) = rest.split_at(plane_size);
    let (c, d) = rest.split_at(plane_size);
    let mut words = pixels.chunks_exact_mut(4);
    for (pixel, ((&a, &b), (&c, &d))) in words.by_ref().zip(a.iter().zip(b).zip(c.iter().zip(d))) {
        pixel.copy_from_slice(&unpack_planes(a, b, c, d));
    }
    let tail = words.into_remainder();
    if !tail.is_empty() {
        let index = size / 4;
        tail.copy_from_slice(&unpack_planes(a[index], b[index], c[index], d[index])[..tail.len()]);
    }
    let stride = dimensions[0] as usize * channels;
    let mut rows = pixels.chunks_exact_mut(stride);
    let mut previous = rows.next().unwrap();
    for index in channels..stride {
        previous[index] = previous[index].wrapping_add(previous[index - channels]);
    }
    for row in rows {
        for (pixel, &above) in row.iter_mut().zip(previous.iter()) {
            *pixel = pixel.wrapping_add(above);
        }
        previous = row;
    }
    Ok(pixels)
}

fn unpack_planes(a: u8, b: u8, c: u8, d: u8) -> [u8; 4] {
    fn spread(byte: u8) -> u32 {
        let byte = u32::from(byte);
        // 将一个位平面的四对位各放入一个独立字节。
        let nibbles = (byte | (byte << 12)) & 0x000f_000f;
        (nibbles | (nibbles << 6)) & 0x0303_0303
    }

    let packed = (spread(a) << 6) | (spread(b) << 4) | (spread(c) << 2) | spread(d);
    let sign = (packed & 0x0101_0101) * 255;
    (((packed & 0xfefe_fefe) >> 1) ^ sign).to_le_bytes()
}

pub(crate) fn byte_size(dimensions: [u32; 2]) -> Result<usize> {
    channel_byte_size(dimensions, 4)
}

fn channel_byte_size(dimensions: [u32; 2], channels: usize) -> Result<usize> {
    if dimensions.contains(&0) {
        return Err(Error::new(0, "HG 像素尺寸为零"));
    }
    let size = dimensions[0]
        .checked_mul(dimensions[1])
        .and_then(|n| n.checked_mul(channels as u32))
        .ok_or_else(|| Error::new(0, "HG 像素尺寸溢出"))?;
    read::sized(size, 0)
}

struct Bits<'a> {
    bytes: &'a [u8],
    position: usize,
    word: u64,
    available: u32,
}

impl<'a> Bits<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            position: 0,
            word: 0,
            available: 0,
        }
    }

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
        if let Some(value) = self.buffered_gamma() {
            return Ok(value);
        }
        let byte = self.position / 8;
        let shift = self.position % 8;
        if let Some(bytes) = self.bytes.get(byte..byte + 8) {
            // 命令按低位开始，翻转后可从最高位连续取出完整码字。
            self.word = u64::from_le_bytes(bytes.try_into().unwrap()).reverse_bits() << shift;
            self.available = 64 - shift as u32;
            if let Some(value) = self.buffered_gamma() {
                return Ok(value);
            }
        }
        self.available = 0;
        self.gamma_tail()
    }

    fn buffered_gamma(&mut self) -> Option<usize> {
        let length = self.word.leading_zeros() * 2 + 1;
        if length > self.available {
            return None;
        }
        let value = self.word >> (64 - length);
        self.word <<= length;
        self.available -= length;
        self.position += length as usize;
        Some(value as usize)
    }

    #[cold]
    fn gamma_tail(&mut self) -> Result<usize> {
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
    let mut bits = Bits::new(commands);
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
    encode_channels(pixels, dimensions, 4)
}

pub(crate) fn encode_channels(
    pixels: &[u8],
    dimensions: [u32; 2],
    channels: usize,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let size = channel_byte_size(dimensions, channels)?;
    if pixels.len() != size {
        return Err(Error::new(0, "HG 像素长度与尺寸不符"));
    }
    let mut deltas = pixels.to_vec();
    let stride = dimensions[0] as usize * channels;
    for i in (stride..size).rev() {
        deltas[i] = deltas[i].wrapping_sub(deltas[i - stride]);
    }
    for i in (channels..stride).rev() {
        deltas[i] = deltas[i].wrapping_sub(deltas[i - channels]);
    }
    let padded = (size.max(1024) + 3) & !3;
    let quarter = padded / 4;
    let mut planes = vec![0; padded];
    for (i, pixel) in deltas.chunks(4).enumerate() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_inflate_preserves_exact_stream_lengths_and_literal_error_order() {
        use std::io::Write;

        let stored = |bytes: &[u8]| {
            let mut writer =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::none());
            writer.write_all(bytes).unwrap();
            writer.finish().unwrap()
        };
        let plain_literals = vec![11; PARALLEL_INFLATE_MIN];
        let plain_commands = vec![37; PARALLEL_INFLATE_MIN + 7];
        let literals = stored(&plain_literals);
        let commands = stored(&plain_commands);
        let mut payload = [&[42][..], &literals, &commands].concat();
        let decode = |bytes: &[u8], l_size: usize, c_length: usize| {
            inflate_streams(
                bytes,
                1,
                literals.len(),
                l_size,
                c_length,
                plain_commands.len(),
            )
        };
        let literal_size = plain_literals.len();
        assert_eq!(
            decode(&payload, literal_size, commands.len()).unwrap(),
            (plain_literals, plain_commands.clone())
        );
        let length_error = Error::new(0, "Zlib 长度或流末尾不匹配");
        assert_eq!(
            decode(&payload, literal_size + 1, commands.len() + 1).unwrap_err(),
            length_error
        );
        assert_eq!(
            decode(&payload, literal_size, commands.len() + 1).unwrap_err(),
            Error::new(1 + literals.len(), "输入范围越界")
        );
        payload.push(0);
        assert_eq!(
            decode(&payload, literal_size, commands.len() + 1).unwrap_err(),
            length_error
        );
    }

    #[test]
    fn gamma_keeps_unaligned_suffix_order_long_codes_and_tail_errors() {
        let bytes = [
            45, 83, 136, 128, 127, 0, 192, 255, 63, 0, 0, 0, 32, 0, 0, 0, 0, 0, 0, 0, 240, 255,
            255, 255, 15, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let mut bits = Bits {
            position: 3,
            ..Bits::new(&bytes)
        };
        for (value, position) in [
            (1, 4),
            (2, 7),
            (3, 10),
            (5, 15),
            (17, 24),
            (255, 39),
            (65535, 70),
            (0x8000_0000, 133),
            (0xffff_ffff, 196),
        ] {
            assert_eq!(bits.gamma().unwrap(), value);
            assert_eq!(bits.position, position);
        }
        for (bytes, value) in [
            ([0, 0, 0, 128, 0, 0, 0, 0], 0x8000_0000),
            ([0, 0, 0, 128, 255, 255, 255, 127], 0xffff_ffff),
        ] {
            let mut bits = Bits::new(&bytes);
            assert_eq!(bits.gamma().unwrap(), value);
            assert_eq!(bits.position, 63);
        }
        for (bytes, position, offset) in [
            ([0u8; 8].as_slice(), 32, 4),
            ([0u8; 2].as_slice(), 16, 2),
            ([128u8].as_slice(), 8, 1),
        ] {
            let mut bits = Bits::new(bytes);
            assert_eq!(bits.gamma().unwrap_err().offset, offset);
            assert_eq!(bits.position, position);
        }
    }

    #[test]
    fn gamma_refills_unaligned_windows_without_consuming_padding() {
        let runs = [1, 3, 4, 7, 63, 8, 1, 257, 65537, 1, 4, 2, 13, 1];
        for alignment in 0..8 {
            let mut writer = BitWriter::default();
            for _ in 0..alignment {
                writer.bit(false);
            }
            for _ in 0..4 {
                for run in runs {
                    writer.gamma(run);
                }
            }
            let mut bits = Bits {
                position: alignment,
                ..Bits::new(&writer.bytes)
            };
            for _ in 0..4 {
                for run in runs {
                    assert_eq!(bits.gamma().unwrap(), run);
                }
            }
            assert_eq!(bits.position, writer.position);
            assert_eq!(bits.gamma().unwrap_err().offset, writer.bytes.len());
        }
    }
}
