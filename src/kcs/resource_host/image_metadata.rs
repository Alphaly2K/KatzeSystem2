use crate::{Result, hg2, hg3, kcs::Interpreter, kcs::NativeReturn};

pub(super) fn query(bytes: &[u8], frame: u32) -> Result<Option<[u32; 9]>> {
    let info = if bytes.starts_with(b"HG-3") {
        hg3::frame_info(bytes, frame)?
    } else if bytes.starts_with(b"HG-2") {
        hg2::frame_info(bytes, frame)?
    } else {
        return Ok(jpeg_size(bytes).map(|[w, h]| [0, 0, w, h, w, h, 3, 0, 0]));
    };
    Ok(info.map(|info| {
        [
            info.offset[0] as u32,
            info.offset[1] as u32,
            info.size[0],
            info.size[1],
            info.canvas_size[0],
            info.canvas_size[1],
            (i32::from(info.bit_depth as i16) / 8) as u32,
            info.base[0] as u32,
            info.base[1] as u32,
        ]
    }))
}

pub(super) fn write(
    info: Option<[u32; 9]>,
    vm: &mut Interpreter,
    address: u32,
) -> Result<NativeReturn> {
    let Some(info) = info else {
        return Ok(NativeReturn::Value(0));
    };
    let mut bytes = [0; 36];
    for (out, field) in bytes.chunks_exact_mut(4).zip(info) {
        out.copy_from_slice(&field.to_le_bytes());
    }
    vm.write(address, &bytes)?;
    Ok(NativeReturn::Value(1))
}

fn jpeg_size(bytes: &[u8]) -> Option<[u32; 2]> {
    if bytes.len() < 11 {
        return None;
    }
    let mut bytes = bytes.strip_prefix(&[0xff, 0xd8])?;
    let mut frame = None;
    loop {
        let Some(marker) = next_jpeg_marker(&mut bytes) else {
            // The original source supplies EOI at EOF and accepts tables-only headers.
            return frame.is_none().then_some([0, 0]);
        };
        match marker {
            0x01 | 0xd0..=0xd7 => continue,
            0xd9 => return frame.is_none().then_some([0, 0]),
            _ => {}
        }
        let length = usize::from(u16::from_be_bytes(bytes.get(..2)?.try_into().ok()?));
        let payload_len = length.checked_sub(2)?;
        let payload = &bytes[2..];
        match marker {
            0xc0..=0xc2 | 0xc9 | 0xca => {
                let payload = payload.get(..payload_len)?;
                let &[8, h0, h1, w0, w1, count, ref components @ ..] = payload else {
                    return None;
                };
                let w = u32::from(u16::from_be_bytes([w0, w1]));
                let h = u32::from(u16::from_be_bytes([h0, h1]));
                if w == 0 || h == 0 || count == 0 || components.len() != usize::from(count) * 3 {
                    return None;
                }
                if frame.replace(([w, h], components)).is_some() {
                    return None;
                }
            }
            0xda => {
                let (size, components) = frame?;
                let count = usize::from(*payload.first()?);
                if !(1..=4).contains(&count) || payload_len != 4 + count * 2 {
                    return None;
                }
                // Only the component IDs are needed before the decoder reaches its first scan.
                for index in 0..count {
                    let id = *payload.get(1 + index * 2)?;
                    if !components
                        .chunks_exact(3)
                        .any(|component| component[0] == id)
                    {
                        return None;
                    }
                }
                return Some(size);
            }
            0xe0..=0xef | 0xfe => {}
            0xc4 | 0xcc | 0xdb | 0xdd => {
                payload.get(..payload_len)?;
            }
            _ => return None,
        }
        bytes = bytes.get(length..).unwrap_or_default();
    }
}

fn next_jpeg_marker(bytes: &mut &[u8]) -> Option<u8> {
    while let Some(start) = bytes.iter().position(|&byte| byte == 0xff) {
        let rest = &bytes[start + 1..];
        let marker = rest.iter().position(|&byte| byte != 0xff)?;
        *bytes = &rest[marker + 1..];
        if rest[marker] != 0 {
            return Some(rest[marker]);
        }
    }
    None
}
