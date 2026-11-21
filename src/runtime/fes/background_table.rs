use super::{ObjectState, Objects};
use crate::{Error, Result, expression::State, fes};
use std::collections::BTreeMap;

/// Background rows and their image state in a FES `BGTABLE2` object.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BackgroundTable {
    pub(super) image: fes::ImageState,
    backgrounds: Vec<Vec<Vec<u8>>>,
    tones: BTreeMap<Vec<u8>, [i32; 3]>,
}

impl BackgroundTable {
    pub(super) fn load(&mut self, bytes: &[u8]) -> Result<()> {
        let mut csv = bytes.split(|&byte| byte == 0).next().unwrap_or_default();
        let mut rows = Vec::new();
        while !csv.is_empty() {
            let mut fields = Vec::new();
            while csv
                .first()
                .is_some_and(|byte| !matches!(byte, b'\r' | b'\n'))
            {
                fields.push(crate::text::delimited::field(&mut csv, b',')?);
            }
            if fields.first().is_some_and(|marker| marker == b"\x81\x9b") {
                rows.push(fields.into_iter().skip(2).collect());
            }
            if !csv.is_empty() {
                csv = &csv[1..];
            }
        }
        self.backgrounds = rows;
        Ok(())
    }

    fn file(&self, row: i32, variation: i32) -> Option<&[u8]> {
        self.backgrounds
            .get(row as usize)?
            .get(variation as usize)
            .map(Vec::as_slice)
            .filter(|name| !name.is_empty())
    }

    fn load_tones(&mut self, bytes: &[u8]) {
        let mut current = [100; 3];
        let text = bytes.split(|&byte| byte == 0).next().unwrap_or_default();
        for line in text.split(|&byte| matches!(byte, b'\r' | b'\n')) {
            let line = line.trim_ascii();
            if line.is_empty() || line.starts_with(b"//") {
                continue;
            }
            if line.starts_with(b"#") {
                current = tone_values(line).unwrap_or([100; 3]);
            } else {
                self.tones.insert(line.to_ascii_lowercase(), current);
            }
        }
    }

    fn tone(&self) -> [i32; 3] {
        self.tones
            .get(&self.image.requested_file.to_ascii_lowercase())
            .copied()
            .unwrap_or([100; 3])
    }
}

fn tone_values(line: &[u8]) -> Option<[i32; 3]> {
    let start = line.iter().position(|&byte| byte == b'(')? + 1;
    let end = start + line[start..].iter().position(|&byte| byte == b')')?;
    let mut fields = line[start..end].split(|&byte| byte == b',');
    let mut tone = [0; 3];
    for channel in &mut tone {
        *channel = std::str::from_utf8(fields.next()?.trim_ascii())
            .ok()?
            .parse()
            .ok()?;
    }
    Some(tone)
}

impl Objects {
    pub(super) fn dispatch_background_table_command(
        &mut self,
        interpreter: &mut fes::Interpreter,
        offset: usize,
        args: &[Vec<u8>],
        target: &fes::ObjectReference,
        numbers: &mut State,
        mut load_table: impl FnMut(&[u8]) -> Result<Option<Vec<u8>>>,
    ) -> Result<bool> {
        let banks = if target.bank == -1 {
            0..i64::from(self.count(&target.name))
        } else {
            i64::from(target.bank)..i64::from(target.bank) + 1
        };
        let mut targets = banks
            .filter_map(|bank| {
                let handle = self.lookup(&target.name, bank as i32)?;
                matches!(self.get(handle)?.state, ObjectState::BackgroundTable(_))
                    .then_some((bank as u32, handle))
            })
            .peekable();
        if targets.peek().is_none() {
            return Ok(false);
        }
        let command = args[1].as_slice();
        if target.bank == -1 && matches!(command, b"max" | b"exist") {
            return Ok(true);
        }
        let targets: Vec<_> = targets.collect();
        if command == b"tonetbl" {
            if let Some(name) = args.get(2) {
                let bytes = load_table(name)?;
                for (_, handle) in targets {
                    let ObjectState::BackgroundTable(table) =
                        &mut self.get_mut(handle).unwrap().state
                    else {
                        unreachable!()
                    };
                    table.tones.clear();
                    if let Some(bytes) = &bytes {
                        table.load_tones(bytes);
                    }
                }
            }
            return Ok(true);
        }
        let mut values = [if command == b"gettonetbl" { -1 } else { 0 }; 3];
        let count = match command {
            b"load" => 2,
            b"max" => 1,
            _ => 3,
        };
        for (value, arg) in values.iter_mut().take(count).zip(args.iter().skip(2)) {
            if !arg.is_empty() {
                *value = interpreter
                    .eval(arg, numbers)
                    .map_err(|error| Error::new(offset + error.offset, error.message))?;
            }
        }
        for (bank, handle) in targets {
            let object = self.objects.get_mut(&handle).unwrap();
            let ObjectState::BackgroundTable(table) = &mut object.state else {
                unreachable!()
            };
            let mut write = |index, value| {
                if let Some(slot) = numbers.shared.get_mut(index as usize) {
                    *slot = value;
                }
            };
            match command {
                b"max" => write(values[0], table.backgrounds.len() as i32),
                b"exist" => write(
                    values[2],
                    i32::from(table.file(values[0], values[1]).is_some()),
                ),
                b"load" => {
                    if let Some(file) = table.file(values[0], values[1]).map(<[u8]>::to_vec) {
                        table.image.apply(
                            &fes::ObjectAttributes {
                                file: Some(file),
                                ..Default::default()
                            },
                            bank,
                            |name| self.languages.expand_image(name),
                        )?;
                        table.image.base.updates.source_rebuild = true;
                    }
                }
                b"gettonetbl" => {
                    for (index, value) in values.into_iter().zip(table.tone()) {
                        write(index, value);
                    }
                }
                _ => unreachable!(),
            }
        }
        Ok(true)
    }
}
