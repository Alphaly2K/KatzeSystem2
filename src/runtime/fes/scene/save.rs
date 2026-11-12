use super::Scene;
use crate::{
    Error, Result, read,
    runtime::{ObjectFactory, Store},
    save::DataContainer,
};

pub(super) struct SaveQuery {
    destination: i32,
    next: i64,
    end: i64,
    winner: i32,
    time: u64,
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn fes_save_command(
        &mut self,
        scene: &mut Scene,
        args: &[Vec<u8>],
        offset: usize,
    ) -> Result<Option<bool>> {
        let Some(name) = args.first().map(Vec::as_slice) else {
            return Ok(None);
        };
        if !matches!(name, b"newsave" | b"saveexist" | b"datasave") {
            return Ok(None);
        }
        let result = (|| {
            if name == b"saveexist" {
                self.fes_save_exists(scene, args)?;
                return Ok(Some(true));
            }
            if name == b"datasave" {
                self.fes_data_save(scene, args)?;
                return Ok(Some(true));
            }
            if scene.save_query.is_none() {
                let mut values = [0; 3];
                for (i, value) in values.iter_mut().enumerate() {
                    let Some(input) = args.get(i + 1) else {
                        return Ok(Some(true));
                    };
                    *value = scene.interpreter.eval(input, self.numbers)?;
                }
                let [destination, start, end] = values;
                if start <= end
                    && (end == i32::MAX
                        || i64::from(end) - i64::from(start) + 1 > read::MAX_ITEMS as i64)
                {
                    return Err(Error::new(0, "FES 存档扫描范围超过上限或结束槽号会溢出"));
                }
                scene.save_query = Some(SaveQuery {
                    destination,
                    next: i64::from(start),
                    end: i64::from(end),
                    winner: -1,
                    time: 0,
                });
            }
            let query = scene.save_query.as_mut().unwrap();
            for _ in 0..self.budget.max(1) {
                if query.next > query.end {
                    break;
                }
                let name = format!("save{:04}.dat", query.next);
                let services = self
                    .host
                    .save_services
                    .as_mut()
                    .ok_or_else(|| Error::new(0, "运行时未配置存档修改时间查询服务"))?;
                if let Some(time) = services.modified_time(scene.owner.unwrap(), name.as_bytes())? {
                    // -1 同时是合法槽号和未选择标记；选到它后仍走首次选择分支。
                    if query.winner == -1 || time > query.time {
                        query.winner = query.next as i32;
                        query.time = time;
                    }
                }
                query.next += 1;
            }
            if query.next <= query.end {
                return Ok(Some(false));
            }
            let query = scene.save_query.take().unwrap();
            if let Some(slot) = self.numbers.shared.get_mut(query.destination as usize) {
                *slot = query.winner;
            }
            Ok(Some(true))
        })();
        result.map_err(|error: Error| Error::new(offset + error.offset, error.message))
    }

    fn fes_save_exists(&mut self, scene: &mut Scene, args: &[Vec<u8>]) -> Result<()> {
        let mut args = args.iter().skip(1);
        let Some(source) = args.next() else {
            return Ok(());
        };
        let slot = scene.interpreter.eval(source, self.numbers)?;
        let name = if slot < 0 {
            let Some(name) = args.next() else {
                return Ok(());
            };
            name.clone()
        } else {
            format!("save{slot:04}.dat").into_bytes()
        };
        let Some(source) = args.next() else {
            return Ok(());
        };
        let destination = scene.interpreter.eval(source, self.numbers)?;
        let lower = args
            .next()
            .map(|source| scene.interpreter.eval(source, self.numbers))
            .transpose()?
            .unwrap_or(-1) as u32;
        let upper = args
            .next()
            .map(|source| scene.interpreter.eval(source, self.numbers))
            .transpose()?
            .map_or(lower, |value| value as u32);
        let data = self
            .host
            .save_services
            .as_mut()
            .ok_or_else(|| Error::new(0, "运行时未配置存档读取服务"))?
            .read(scene.owner.unwrap(), &name)?
            .map(|file| DataContainer::from_css(&file))
            .transpose()?
            .flatten();
        let result = match data {
            None => 0,
            Some(data) if lower == u32::MAX || (lower..=upper).contains(&data.tag()) => 1,
            Some(_) => -1,
        };
        if let Some(slot) = self.numbers.shared.get_mut(destination as usize) {
            *slot = result;
        }
        Ok(())
    }

    fn fes_data_save(&mut self, scene: &mut Scene, args: &[Vec<u8>]) -> Result<()> {
        let mut values = [0; 2];
        for (index, value) in values.iter_mut().enumerate() {
            let Some(source) = args.get(index + 1) else {
                return Ok(());
            };
            *value = scene.interpreter.eval(source, self.numbers)?;
        }
        let [slot, handle] = values;
        if handle == 0 {
            return Ok(());
        }
        let bytes = self.host.data_container(handle as u32)?.to_css()?;
        let name = format!("save{slot:04}.dat");
        self.host
            .save_services
            .as_mut()
            .ok_or_else(|| Error::new(0, "运行时未配置存档写入服务"))?
            .write(scene.owner.unwrap(), name.as_bytes(), &bytes)?;
        Ok(())
    }
}
