use super::{ObjectState, Scene};
use crate::{
    Error, Result, fes,
    runtime::{ObjectFactory, Store},
};

impl<F: ObjectFactory> Store<'_, F> {
    fn panel_values<const N: usize>(
        &mut self,
        scene: &mut Scene,
        args: &[Vec<u8>],
        offset: usize,
    ) -> Result<Option<[i32; N]>> {
        let mut values = [0; N];
        for (index, value) in values.iter_mut().enumerate() {
            let Some(source) = args.get(index + 2) else {
                return Ok(None);
            };
            if !source.is_empty() {
                *value = scene
                    .interpreter
                    .eval(source, self.numbers)
                    .map_err(|error| Error::new(offset + error.offset, error.message))?;
            }
        }
        Ok(Some(values))
    }

    fn panel_number(
        &mut self,
        scene: &mut Scene,
        target: &fes::ObjectReference,
        args: &[Vec<u8>],
        offset: usize,
    ) -> Result<()> {
        let Some([id, value]) = self.panel_values(scene, args, offset)? else {
            return Ok(());
        };
        let banks = if target.bank == -1 {
            0..i64::from(scene.objects.count(&target.name))
        } else {
            i64::from(target.bank)..i64::from(target.bank) + 1
        };
        for bank in banks {
            let Some(handle) = scene.objects.lookup(&target.name, bank as i32) else {
                break;
            };
            let ObjectState::SavePanel(panel) = &mut scene.objects.get_mut(handle).unwrap().state
            else {
                continue;
            };
            let Some(cell) = panel.cells.get_mut(&id) else {
                continue;
            };
            cell.data[0x60..0x64].copy_from_slice(&value.to_le_bytes());
            if let Some(content) = panel.contents.get_mut(&id) {
                content.set_number(value);
            }
            scene.objects.invalidate(handle)?;
        }
        Ok(())
    }

    fn panel_flag(
        &mut self,
        scene: &mut Scene,
        target: &fes::ObjectReference,
        args: &[Vec<u8>],
        offset: usize,
    ) -> Result<()> {
        if target.bank == -1 {
            return Ok(());
        }
        let Some([id, destination]) = self.panel_values(scene, args, offset)? else {
            return Ok(());
        };
        let Some(handle) = scene.objects.lookup(&target.name, target.bank) else {
            return Ok(());
        };
        if let ObjectState::SavePanel(panel) = &scene.objects.get(handle).unwrap().state {
            let value = panel.flags.get(id as usize).copied().unwrap_or(0);
            if let Some(slot) = self.numbers.shared.get_mut(destination as usize) {
                *slot = value;
            }
        }
        Ok(())
    }

    fn panel_save(
        &mut self,
        scene: &mut Scene,
        target: &fes::ObjectReference,
        args: &[Vec<u8>],
        offset: usize,
    ) -> Result<()> {
        if target.bank == -1 {
            return Ok(());
        }
        let Some([slot, record_id, start]) = self.panel_values(scene, args, offset)? else {
            return Ok(());
        };
        let Some(handle) = scene.objects.lookup(&target.name, target.bank) else {
            return Ok(());
        };
        let ObjectState::SavePanel(panel) = &mut scene.objects.get_mut(handle).unwrap().state
        else {
            return Ok(());
        };
        let name = format!("save{slot:04}.dat").into_bytes();
        panel.save_name.clone_from(&name);
        panel.record_id = record_id as u32;
        let result = (|| {
            let services = self
                .host
                .save_services
                .as_mut()
                .ok_or_else(|| Error::new(0, "运行时未配置存档读取服务"))?;
            let data = services
                .read(scene.owner.unwrap(), &name)?
                .map(|file| crate::save::DataContainer::from_css(&file))
                .transpose()?
                .flatten();
            panel.date = [0; 6];
            if let Some(data) = data {
                panel.data = Some(data);
                panel.date = services
                    .modified_date(scene.owner.unwrap(), &name)?
                    .unwrap_or([0; 6]);
                if let (Some(source), Some(target)) = (
                    panel.data.as_ref().unwrap().record(record_id as u32)?,
                    panel.flags.get_mut(start as usize..),
                ) {
                    for (target, bytes) in target.iter_mut().zip(source.chunks_exact(4)) {
                        *target = i32::from_le_bytes(bytes.try_into().unwrap());
                    }
                }
            }
            panel.apply_data()?;
            Ok(())
        })();
        result.map_err(|error: Error| Error::new(offset + error.offset, error.message))?;
        scene.objects.invalidate(handle)?;
        Ok(())
    }

    fn panel_cell(
        &mut self,
        scene: &mut Scene,
        target: &fes::ObjectReference,
        args: &[Vec<u8>],
        offset: usize,
    ) -> Result<()> {
        let banks = if target.bank == -1 {
            0..i64::from(scene.objects.count(&target.name))
        } else {
            i64::from(target.bank)..i64::from(target.bank) + 1
        };
        let Some((id, cell)) = CellArguments::parse(args, &mut scene.interpreter, self.numbers)
            .map_err(|error| Error::new(offset + error.offset, error.message))?
        else {
            return Ok(());
        };
        for bank in banks {
            let Some(handle) = scene.objects.lookup(&target.name, bank as i32) else {
                break;
            };
            if let ObjectState::SavePanel(panel) = &mut scene.objects.get_mut(handle).unwrap().state
            {
                panel.cells.insert(id, cell.clone());
            }
        }
        Ok(())
    }

    pub(super) fn save_panel_command(
        &mut self,
        scene: &mut Scene,
        target: &fes::ObjectReference,
        args: &[Vec<u8>],
        line: &fes::Line,
    ) -> Result<bool> {
        let Some(name) = args.get(1).map(Vec::as_slice) else {
            return Ok(false);
        };
        if !matches!(
            name,
            b"ptype" | b"dispid" | b"setnum" | b"setsave" | b"getflag"
        ) {
            return Ok(false);
        }
        if name == b"ptype" {
            self.panel_cell(scene, target, args, line.offset)?;
            return Ok(true);
        }
        if name == b"setnum" {
            self.panel_number(scene, target, args, line.offset)?;
            return Ok(true);
        }
        if name == b"dispid" {
            self.panel_selection(scene, target, args, line.offset)?;
            return Ok(true);
        }
        if name == b"getflag" {
            self.panel_flag(scene, target, args, line.offset)?;
        } else if name == b"setsave" {
            self.panel_save(scene, target, args, line.offset)?;
        }
        Ok(true)
    }

    fn panel_selection(
        &mut self,
        scene: &mut Scene,
        target: &fes::ObjectReference,
        args: &[Vec<u8>],
        offset: usize,
    ) -> Result<()> {
        let enabled = match args.get(2) {
            Some(value) => {
                scene
                    .interpreter
                    .eval(value, self.numbers)
                    .map_err(|error| Error::new(offset + error.offset, error.message))?
                    != 0
            }
            None => true,
        };
        let banks = if target.bank == -1 {
            0..i64::from(scene.objects.count(&target.name))
        } else {
            i64::from(target.bank)..i64::from(target.bank) + 1
        };
        for bank in banks {
            let Some(handle) = scene.objects.lookup(&target.name, bank as i32) else {
                break;
            };
            for source in args.iter().skip(3) {
                let mut eval = |text: &[u8]| {
                    scene
                        .interpreter
                        .eval(text, self.numbers)
                        .map_err(|error| Error::new(offset + error.offset, error.message))
                };
                let (first, last) = if source.first() == Some(&b'[') {
                    let body = &source[1..];
                    let split = body.iter().position(|&byte| byte == b'-');
                    let first = split.map_or(body, |index| &body[..index]);
                    if first.len() >= 128 {
                        return Err(Error::new(offset, "面板编号表达式超过容量"));
                    }
                    let first = eval(first)?;
                    let last = match split {
                        Some(index) => {
                            let tail = &body[index + 1..];
                            let end = tail
                                .iter()
                                .position(|&byte| byte == b']')
                                .unwrap_or(tail.len());
                            if end >= 128 {
                                return Err(Error::new(offset, "面板编号表达式超过容量"));
                            }
                            eval(&tail[..end])?
                        }
                        None => first,
                    };
                    (first, last)
                } else {
                    let value = eval(source)?;
                    (value, value)
                };
                if first > last {
                    continue;
                }
                if last == i32::MAX || i64::from(last) - i64::from(first) >= 1_000_000 {
                    return Err(Error::new(offset, "面板编号区间超出处理范围"));
                }
                if let ObjectState::SavePanel(panel) =
                    &mut scene.objects.get_mut(handle).unwrap().state
                {
                    for id in first..=last {
                        if enabled {
                            panel.enabled_ids.insert(id);
                        } else {
                            panel.enabled_ids.remove(&id);
                        }
                    }
                    scene.objects.invalidate(handle)?;
                }
            }
        }
        Ok(())
    }
}

struct CellArguments<'a> {
    args: &'a [Vec<u8>],
    interpreter: &'a mut fes::Interpreter,
    numbers: &'a mut crate::expression::State,
    cell: fes::PanelCell,
}

impl CellArguments<'_> {
    fn parse(
        args: &[Vec<u8>],
        interpreter: &mut fes::Interpreter,
        numbers: &mut crate::expression::State,
    ) -> Result<Option<(i32, fes::PanelCell)>> {
        let Some(name) = args.get(2).map(Vec::as_slice) else {
            return Ok(None);
        };
        let kind = match name {
            b"NORM" => 0,
            b"IMAGE" | b"SIMAGE" => 1,
            b"IMAGEOBJ" => 2,
            b"STR" | b"SSTR" => 3,
            b"NUM_S" => 4,
            b"NUM_O" | b"NUM_O2" => 5,
            b"NUM_I" => 6,
            _ => return Ok(None),
        };
        let mut parser = CellArguments {
            args,
            interpreter,
            numbers,
            cell: fes::PanelCell::default(),
        };
        let Some(id) = parser.value(3)? else {
            return Ok(None);
        };
        parser.word(0, kind);
        if name == b"NUM_O2" {
            parser.word(0x54, 1);
        }
        match kind {
            1 if name == b"SIMAGE" => {
                if !parser.number(4, 0x10)? {
                    return Ok(None);
                }
                if parser.number(5, 0xa4)? {
                    for (index, offset) in [(6, 0xa8), (7, 0xac), (8, 0xb0), (9, 0xb4), (10, 0xb8)]
                    {
                        parser.number(index, offset)?;
                    }
                    if parser.read(0xb4) == 0 || parser.read(0xb8) == 0 {
                        parser.word(0xac, 0);
                        parser.word(0xb0, 0);
                        parser.word(0xb4, parser.read(0xa4).wrapping_sub(1));
                        parser.word(0xb8, parser.read(0xa8).wrapping_sub(1));
                    }
                    parser.word(0, 7);
                    parser.word(4, 10);
                } else {
                    parser.word(4, 1);
                }
            }
            2 if !parser.reference(4)? => return Ok(None),
            3 => {
                parser.word(0x58, 1);
                if !parser.selector(4)? {
                    return Ok(None);
                }
                if parser.read(4) == 9 {
                    parser.word(0x10, parser.read(0x60));
                    parser.word(4, 2);
                } else {
                    parser.word(0x10, u32::MAX);
                }
                for (index, offset) in [(5, 0x14), (6, 0x1c), (7, 0x18)] {
                    if !parser.number(index, offset)? {
                        return Ok(None);
                    }
                }
                if let Some(value) = parser.value(8)? {
                    parser.word(0x48, value & 1);
                    parser.word(0x4c, (value >> 1) & 1);
                }
                for (index, offset) in [(9, 0x50), (10, 0x58), (11, 0x5c)] {
                    parser.number(index, offset)?;
                }
                if let Some(font) = args.get(12) {
                    let font = font.split(|&byte| byte == 0).next().unwrap();
                    if font.len() >= 88 {
                        return Err(Error::new(0, "面板字体名称超过容量"));
                    }
                    parser.cell.data[0x64..0x64 + font.len()].copy_from_slice(font);
                }
            }
            5 if !parser.reference(4)? || !parser.selector(5)? => return Ok(None),
            _ => {}
        }
        Ok(Some((id as i32, parser.cell)))
    }

    fn evaluate(&mut self, source: &[u8]) -> Result<u32> {
        if source.is_empty() {
            return Ok(0);
        }
        self.interpreter
            .eval(source, self.numbers)
            .map(|value| value as u32)
    }

    fn value(&mut self, index: usize) -> Result<Option<u32>> {
        self.args
            .get(index)
            .map(|source| self.evaluate(source))
            .transpose()
    }

    fn number(&mut self, index: usize, offset: usize) -> Result<bool> {
        let Some(value) = self.value(index)? else {
            return Ok(false);
        };
        self.word(offset, value);
        Ok(true)
    }

    fn reference(&mut self, index: usize) -> Result<bool> {
        let Some(source) = self.args.get(index) else {
            return Ok(false);
        };
        if source.is_empty() {
            return Ok(false);
        }
        let reference = self.interpreter.object_reference(source, self.numbers)?;
        let name = reference.name.as_bytes();
        if name.len() >= 32 {
            return Err(Error::new(0, "面板来源对象名超过容量"));
        }
        self.cell.data[0x20..0x20 + name.len()].copy_from_slice(name);
        self.word(0x40, reference.bank as u32);
        Ok(true)
    }

    fn selector(&mut self, index: usize) -> Result<bool> {
        let Some(source) = self.args.get(index) else {
            return Ok(false);
        };
        let source = source.split(|&byte| byte == 0).next().unwrap();
        let end = source
            .iter()
            .position(|&byte| matches!(byte, b'.' | b':'))
            .unwrap_or(source.len());
        let name = &source[..end];
        if name.len() >= 32 {
            return Err(Error::new(0, "面板数值来源超过容量"));
        }
        let mut cursor = end;
        let mut first = 0;
        if source.get(cursor) == Some(&b'.') {
            let start = cursor + 1;
            cursor = source[start..]
                .iter()
                .position(|&byte| byte == b':')
                .map_or(source.len(), |i| start + i);
            if cursor - start >= 32 {
                return Err(Error::new(0, "面板数值来源表达式超过容量"));
            }
            first = self.evaluate(&source[start..cursor])?;
        }
        let second = if source.get(cursor) == Some(&b':') {
            let value = &source[cursor + 1..];
            if value.len() >= 32 {
                return Err(Error::new(0, "面板数值来源表达式超过容量"));
            }
            Some(self.evaluate(value)?)
        } else {
            None
        };
        let kind = match name {
            b"FLAG" => 0,
            b"DATE_Y" => 3,
            b"DATE_M" => 4,
            b"DATE_D" => 5,
            b"DATE_H" => 6,
            b"DATE_N" => 7,
            b"DATE_S" => 8,
            _ => 9,
        };
        self.word(4, kind);
        if kind == 0 {
            self.word(8, first);
            self.word(0xc, second.unwrap_or(0));
        } else if kind == 9 {
            let value = self.evaluate(name)?;
            self.word(0x60, value);
        } else {
            self.word(0xc, second.unwrap_or(u32::MAX));
        }
        Ok(true)
    }

    fn word(&mut self, offset: usize, value: u32) {
        self.cell.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn read(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.cell.data[offset..offset + 4].try_into().unwrap())
    }
}
