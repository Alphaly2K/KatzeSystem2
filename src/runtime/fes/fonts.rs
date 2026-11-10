use super::Scene;
use crate::runtime::{ObjectFactory, Store};
use crate::{Error, Result, fes};

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn fes_font_command(
        &mut self,
        scene: &mut Scene,
        args: &[Vec<u8>],
        line: &fes::Line,
    ) -> Result<bool> {
        if !args.first().is_some_and(|name| name == b"fontcom") {
            return Ok(false);
        }
        let offset = line.offset;
        match args.get(1).map(Vec::as_slice) {
            Some(b"init") => {
                scene.font_names = None;
                let character_set = args
                    .get(2)
                    .map_or(Ok(128), |value| scene.interpreter.eval(value, self.numbers))?
                    as u32;
                let mut names = self
                    .resources
                    .fonts
                    .as_mut()
                    .ok_or_else(|| Error::new(offset, "运行时未配置字体服务"))?
                    .enumerate(character_set, self.host.message_measurement_parameter)?;
                if names.len() > crate::read::MAX_ITEMS {
                    return Err(Error::new(offset, "字体枚举数量超过上限"));
                }
                for name in &mut names {
                    if let Some(end) = name.iter().position(|&unit| unit == 0) {
                        name.truncate(end);
                    }
                    if name.len() >= 64 || String::from_utf16(name).is_err() {
                        return Err(Error::new(offset, "枚举字体名称过长或不是有效 UTF-16"));
                    }
                }
                scene.font_names = Some(names);
            }
            Some(b"add") => {
                if let Some(name) = args.get(2).filter(|name| !name.is_empty()) {
                    self.resources
                        .fonts
                        .as_mut()
                        .ok_or_else(|| Error::new(offset, "运行时未配置字体服务"))?
                        .add_resource(name)?;
                }
            }
            Some(command @ (b"count" | b"get" | b"search" | b"default")) => {
                let Some(names) = scene.font_names.as_ref() else {
                    return Ok(true);
                };
                let Some(index) = args.get(2) else {
                    return Ok(true);
                };
                let index = scene.interpreter.eval(index, self.numbers)? as u32;
                match command {
                    b"count" => {
                        if let Some(slot) = self.numbers.shared.get_mut(index as usize) {
                            *slot = names.len() as i32;
                        }
                    }
                    b"get" => {
                        let Some(output) = args.get(3) else {
                            return Ok(true);
                        };
                        let output = scene.interpreter.eval(output, self.numbers)? as u32;
                        let value = names.get(index as usize).map_or_else(Vec::new, |name| {
                            String::from_utf16(name).unwrap().into_bytes()
                        });
                        self.numbers.strings.insert(output, value);
                    }
                    b"search" => {
                        let raw = super::string::remainder(&line.bytes, 3);
                        if raw.is_empty() {
                            return Ok(true);
                        }
                        let text = std::str::from_utf8(raw)
                            .map_err(|_| Error::new(offset, "FES 字体查询名称需要有效 UTF-8"))?;
                        let name = text.encode_utf16().collect::<Vec<_>>();
                        let found = names
                            .iter()
                            .position(|value| value == &name)
                            .map_or(-1, |index| index as i32);
                        if let Some(slot) = self.numbers.shared.get_mut(index as usize) {
                            *slot = found;
                        }
                    }
                    b"default" => {
                        let defaults = self.host.message_defaults.clone().unwrap_or_default();
                        let name = defaults.font_name.split(|&unit| unit == 0).next().unwrap();
                        self.numbers.strings.insert(index, font_name_bytes(name));
                    }
                    _ => unreachable!(),
                }
            }
            _ => (),
        }
        Ok(true)
    }
}

pub(super) fn font_name(bytes: &[u8], offset: usize) -> Result<Vec<u16>> {
    let bytes = bytes.split(|&byte| byte == 0).next().unwrap();
    let mut name = Vec::new();
    let mut cursor = 0;
    while bytes.get(cursor).copied().unwrap_or(0) != 0 {
        let byte = bytes[cursor];
        let (unit, length) = if bytes.get(cursor..cursor + 2) == Some(&[255, 255]) {
            (u16::MAX, 2)
        } else {
            // 原读取器只核对中间续字节，逐项转换后保留首个 UTF-16 单元。
            let leading = byte.leading_ones();
            if leading == 1 {
                break;
            }
            let length = if leading >= 6 { 1 } else { leading.max(1) } as usize;
            if (1..length.saturating_sub(1))
                .any(|i| bytes.get(cursor + i).copied().unwrap_or(0) & 0xc0 != 0x80)
            {
                break;
            }
            if cursor + length > bytes.len() {
                return Err(Error::new(offset, "FES 字体名 UTF-8 字符被截断"));
            }
            let mut chunk = [0; 5];
            for (i, value) in chunk[..length].iter_mut().enumerate() {
                *value = bytes.get(cursor + i).copied().unwrap_or(0);
            }
            let text = String::from_utf8_lossy(&chunk[..length]);
            let unit = text.encode_utf16().next().unwrap_or(0);
            (unit, length)
        };
        if name.len() == 63 {
            return Err(Error::new(offset, "FES 字体名超过 63 个 UTF-16 单元"));
        }
        name.push(unit);
        cursor += length;
    }
    Ok(name)
}

pub(super) fn font_name_bytes(name: &[u16]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for &unit in name.iter().take_while(|&&unit| unit != 0) {
        if unit == u16::MAX {
            bytes.extend([255, 255]);
        } else {
            let character = char::from_u32(u32::from(unit)).unwrap_or(char::REPLACEMENT_CHARACTER);
            let mut buffer = [0; 4];
            bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
        }
    }
    bytes
}
