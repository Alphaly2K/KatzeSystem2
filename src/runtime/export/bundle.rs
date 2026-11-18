use super::ExportDestination;
use crate::{Error, Result, read};
use std::sync::atomic::{AtomicBool, Ordering};

const NODE_HEADER: usize = 12;
const NAME_END: usize = 260;
const FILE_HEADER: usize = 272;
const MAX_DEPTH: usize = 64;

pub(super) fn unpack(
    bytes: &[u8],
    destination: &mut dyn ExportDestination,
    cancel: &AtomicBool,
) -> Result<()> {
    let mut bundle = Bundle {
        bytes,
        destination,
        cancel,
        remaining: read::MAX_ITEMS,
    };
    bundle.extract_level(0, &mut Vec::new())
}

struct Bundle<'a> {
    bytes: &'a [u8],
    destination: &'a mut dyn ExportDestination,
    cancel: &'a AtomicBool,
    remaining: usize,
}

impl Bundle<'_> {
    fn extract_level(&mut self, mut offset: usize, path: &mut Vec<String>) -> Result<()> {
        if path.len() == MAX_DEPTH {
            return Err(Error::new(offset, "导出目录层级超过上限"));
        }
        let mut nodes = Vec::new();
        while !self.cancel.load(Ordering::Relaxed) {
            self.remaining = self
                .remaining
                .checked_sub(1)
                .ok_or_else(|| Error::new(offset, "导出包节点数量超过上限"))?;
            let header = read::range(self.bytes, offset, NODE_HEADER)?;
            let next = read::u32_at(header, 0)? as usize;
            let child = read::u32_at(header, 4)? as usize;
            let length = read::u32_at(header, 8)? as usize;
            let data = read::range(self.bytes, offset + NODE_HEADER, length)?;
            let child = link(offset, length, child)?;
            nodes.push((offset, child, data));
            let Some(next) = link(offset, length, next)? else {
                break;
            };
            offset = next;
        }
        // 装入时逐个从头插入同级节点，原导出遍历使用插入后的顺序。
        for (offset, child, data) in nodes.into_iter().rev() {
            if self.cancel.load(Ordering::Relaxed) {
                break;
            }
            let kind = read::u32_at(data, 0)?;
            let name = read::zstr(read::range(data, 4, NAME_END - 4)?, 0)?;
            let name = encoding_rs::SHIFT_JIS
                .decode_without_bom_handling_and_without_replacement(name)
                .ok_or_else(|| Error::new(offset, "导出包名称不是有效 CP932"))?;
            if name.is_empty()
                || name == "."
                || name == ".."
                || name.contains(['/', '\\', ':', '<', '>', '"', '|', '?', '*'])
                || name.ends_with(['.', ' '])
                || name.chars().any(char::is_control)
            {
                return Err(Error::new(offset, "导出包名称不是有效路径组件"));
            }
            path.push(name.into_owned());
            match kind {
                0 if child.is_none() => {
                    let packed = read::u32_at(data, 264)? as usize;
                    let plain = read::sized(read::u32_at(data, 268)?, offset)?;
                    let packed = read::range(data, FILE_HEADER, packed)?;
                    let bytes = if packed.is_empty() && plain == 0 {
                        Vec::new()
                    } else {
                        read::inflate(packed, plain)?
                    };
                    if !self.cancel.load(Ordering::Relaxed) {
                        self.destination.write_file(path, &bytes)?;
                    }
                }
                1 => {
                    self.destination.create_directory(path)?;
                    if let Some(child) = child {
                        self.extract_level(child, path)?;
                    }
                }
                _ => return Err(Error::new(offset, "导出包节点类型无效")),
            }
            path.pop();
        }
        Ok(())
    }
}

fn link(offset: usize, length: usize, relative: usize) -> Result<Option<usize>> {
    if relative == 0 {
        return Ok(None);
    }
    if relative < NODE_HEADER + length {
        return Err(Error::new(offset, "导出包节点链接与当前记录重叠"));
    }
    offset
        .checked_add(relative)
        .map(Some)
        .ok_or_else(|| Error::new(offset, "导出包节点偏移溢出"))
}
