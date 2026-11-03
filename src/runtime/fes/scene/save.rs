use super::Scene;
use crate::{
    Error, Result, read,
    runtime::{ObjectFactory, Store},
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
        if args.first().is_none_or(|name| name != b"newsave") {
            return Ok(None);
        }
        let result = (|| {
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
}
