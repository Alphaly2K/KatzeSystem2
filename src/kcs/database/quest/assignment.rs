use super::{QuestTable, Record, UNASSIGNED, cell_word};
use crate::Result;

impl QuestTable {
    pub(super) fn refresh_availability(&mut self) -> Result<()> {
        let available = self
            .rows
            .values()
            .map(|record| self.available(record))
            .collect::<Result<Vec<_>>>()?;
        for (record, available) in self.rows.values_mut().zip(available) {
            record.progress.flags = u32::from(available);
        }
        Ok(())
    }

    fn available(&self, record: &Record) -> Result<bool> {
        let number = |column| cell_word(&self.table, record.row, column);
        let kind = number(b"TYPE")?;
        let min = number(b"LV_MIN")? as i32;
        let max = number(b"LV_MAX")? as i32;
        let level = self.state[0] as i32;
        let key = number(b"KEY")? as i32;
        if kind > 5
            || (kind != 0 && record.progress.stamp != 0)
            || level < min
            || level > if max == -1 { min } else { max }
            || (key != -1 && self.rows.get(&key).is_some_and(|r| r.progress.stamp == 0))
        {
            return Ok(false);
        }

        let groups = &record.characters.parameters;
        let counter =
            |slot: i32, item: usize| self.counters.get(slot as usize).map(|s| s[item] as i32);
        let meets = |group: &Vec<i32>, item| match group.as_slice() {
            [slot, threshold, ..] => counter(*slot, item).map(|value| value >= *threshold),
            _ => None,
        };
        let threshold = |value| {
            groups
                .first()
                .and_then(|g| g.first())
                .is_some_and(|&min| value >= min)
        };
        Ok(match number(b"COND")? {
            0 => true,
            1 | 20 => groups.iter().all(|group| meets(group, 3).unwrap_or(true)),
            2 => groups.iter().any(|group| meets(group, 3).unwrap_or(false)),
            3 => groups.get(1).and_then(|g| g.first()).is_some_and(|&min| {
                groups[0]
                    .iter()
                    .filter_map(|&slot| counter(slot, 3))
                    .fold(0i32, i32::wrapping_add)
                    >= min
            }),
            10 => groups
                .iter()
                .flatten()
                .all(|id| self.rows.get(id).is_none_or(|r| r.progress.stamp != 0)),
            11 => groups
                .iter()
                .flatten()
                .any(|id| self.rows.get(id).is_some_and(|r| r.progress.stamp != 0)),
            21 => groups.iter().all(|group| meets(group, 4).unwrap_or(true)),
            30 => threshold(self.state[1] as i32),
            40 => threshold(self.attempts as i32),
            41 => threshold(
                self.rows
                    .range(..100)
                    .filter(|(_, r)| r.progress.stamp != 0)
                    .count() as i32,
            ),
            _ => false,
        })
    }

    pub(super) fn assign_places(&mut self) -> Result<()> {
        for record in self.rows.values_mut() {
            record.progress.assignment = UNASSIGNED;
        }
        let candidates: Vec<_> = self
            .rows
            .iter()
            .filter(|(_, r)| r.progress.flags != 0)
            .map(|(&id, _)| id)
            .collect();
        for id in candidates {
            let row = self.rows[&id].row;
            let place = cell_word(&self.table, row, b"PLACE")?;
            let mut previous = None;
            for (&other, record) in &self.rows {
                if record.progress.assignment == place
                    && (2..=4).contains(&cell_word(&self.table, record.row, b"TYPE")?)
                {
                    previous = (other != -1).then_some((other, record.row));
                    break;
                }
            }
            if let Some((other, prior_row)) = previous {
                let old = cell_word(&self.table, prior_row, b"PRI")? as i32;
                let new = cell_word(&self.table, row, b"PRI")? as i32;
                if old >= new {
                    continue;
                }
                self.rows.get_mut(&other).unwrap().progress.assignment = UNASSIGNED;
            }
            self.rows.get_mut(&id).unwrap().progress.assignment = place;
        }
        Ok(())
    }
}
