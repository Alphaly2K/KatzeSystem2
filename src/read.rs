use crate::{Error, Result};

pub const MAX_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_ITEMS: usize = 1_000_000;

pub fn range(data: &[u8], start: usize, len: usize) -> Result<&[u8]> {
    start
        .checked_add(len)
        .and_then(|end| data.get(start..end))
        .ok_or_else(|| Error::new(start, "输入范围越界"))
}

pub fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        range(data, offset, 4)?.try_into().unwrap(),
    ))
}

pub fn sized(value: u32, offset: usize) -> Result<usize> {
    let n = value as usize;
    if n > MAX_BYTES {
        return Err(Error::new(offset, "声明长度超过读取上限"));
    }
    Ok(n)
}

pub fn inflate(data: &[u8], expected: usize) -> Result<Vec<u8>> {
    if expected > MAX_BYTES {
        return Err(Error::new(0, "解压长度超过读取上限"));
    }
    let mut decoder = flate2::Decompress::new(true);
    let mut out = vec![0; expected + 1];
    let status = decoder
        .decompress(data, &mut out, flate2::FlushDecompress::Finish)
        .map_err(|e| Error::new(0, format!("Zlib 解压失败：{e}")))?;
    if status != flate2::Status::StreamEnd
        || decoder.total_out() as usize != expected
        || decoder.total_in() as usize != data.len()
    {
        return Err(Error::new(0, "Zlib 长度或流末尾不匹配"));
    }
    out.truncate(expected);
    Ok(out)
}

pub fn zstr(data: &[u8], offset: usize) -> Result<&[u8]> {
    let rest = data
        .get(offset..)
        .ok_or_else(|| Error::new(offset, "字符串偏移越界"))?;
    let end = rest
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| Error::new(offset, "字符串缺少结束字节"))?;
    Ok(&rest[..end])
}
