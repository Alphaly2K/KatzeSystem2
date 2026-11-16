use super::Scene;
use crate::{
    Error, Result,
    runtime::{ObjectFactory, Store},
};

fn text(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn fes_database_command(
        &mut self,
        scene: &mut Scene,
        args: &[Vec<u8>],
        offset: usize,
    ) -> Result<bool> {
        let Some(command) = args.first().map(Vec::as_slice) else {
            return Ok(false);
        };
        let count = match command {
            b"dic_find" => 4,
            b"dic_title" | b"dic_update" => 3,
            b"dic_update_clear" => 1,
            b"dic_appear" | b"dic_max_appear" | b"dic_id" | b"dic_ind" => 2,
            _ => return Ok(false),
        };
        let mut values = Vec::with_capacity(count);
        for i in 1..=count {
            let Some(bytes) = args.get(i) else {
                return Ok(true);
            };
            let value = if bytes.is_empty() {
                0
            } else {
                scene
                    .interpreter
                    .eval(bytes, self.numbers)
                    .map_err(|error| Error::new(offset + error.offset, error.message))?
                    as u32
            };
            values.push(value);
            if i == 1 && command == b"dic_find" && value == 0 {
                return Ok(true);
            }
        }
        let name = args.get(count + 1).map(|bytes| text(bytes));
        if command != b"dic_update_clear" && name.is_none() {
            return Ok(true);
        }
        let name = name.unwrap_or_default();
        if command == b"dic_find" {
            let Some(to) = args.get(count + 2) else {
                return Ok(true);
            };
            if name.len() >= 32 || text(to).len() >= 32 {
                return Err(Error::new(offset, "数据库范围名称超过固定缓冲区上限"));
            }
        } else if name.len() >= 2048 {
            return Err(Error::new(offset, "数据库查询名称超过固定缓冲区上限"));
        }
        let database = self
            .host
            .database_mut(values[0])
            .map_err(|error| Error::new(offset, error.message))?;
        let write_number = |state: &mut crate::expression::State, index: u32, value: u32| {
            if let Some(slot) = state.shared.get_mut(index as usize) {
                *slot = value as i32;
            }
        };
        match command {
            b"dic_find" => {
                let found = database.find(name, text(&args[count + 2]), values[3]);
                self.numbers
                    .strings
                    .insert(values[2], found.unwrap_or_default().to_vec());
                write_number(self.numbers, values[1], u32::from(found.is_some()));
            }
            b"dic_appear" | b"dic_max_appear" | b"dic_id" => {
                let value = match command {
                    b"dic_appear" => database.record(name).map_or(0, |record| record.seen),
                    b"dic_max_appear" => database.max_appear(name),
                    _ => database.position(name),
                };
                write_number(self.numbers, values[1], value);
            }
            b"dic_title" => {
                let record = database.record(name);
                self.numbers.strings.insert(
                    values[1],
                    record.map_or(&[][..], |r| r.title.as_slice()).to_vec(),
                );
                self.numbers.strings.insert(
                    values[2],
                    record
                        .map_or(&[][..], |r| r.short_title.as_slice())
                        .to_vec(),
                );
            }
            b"dic_ind" => {
                let output = database
                    .visible_terms(name)
                    .map_err(|error| Error::new(offset, error.message))?;
                self.numbers.strings.insert(values[1], output);
            }
            b"dic_update" => {
                let record = database.record(name);
                write_number(self.numbers, values[1], record.map_or(0, |r| r.first_seen));
                write_number(self.numbers, values[2], record.map_or(0, |r| r.pending));
            }
            b"dic_update_clear" => {
                database.clear_updates(args.get(count + 1).map(|bytes| text(bytes)))
            }
            _ => unreachable!(),
        }
        Ok(true)
    }
}
