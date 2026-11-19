use super::{COUNTERS, Progress, QuestTable};
use crate::{Error, Result, read};

pub(super) const HEADER_SIZE: usize = 36;
const RECORD_SIZE: usize = 24;
const SLOT_SIZE: usize = 28;
const SLOTS_SIZE: usize = SLOT_SIZE * COUNTERS;

pub(super) struct SavedProgress {
    rows: Vec<(i32, Progress)>,
    counters: Vec<(usize, [u32; 6])>,
    attempts: u32,
}

impl SavedProgress {
    pub(super) fn parse(bytes: &[u8], length: u32) -> Result<Option<Self>> {
        let [
            row_offset,
            row_size,
            row_count,
            slot_offset,
            slot_size,
            slot_count,
            attempts_offset,
            _,
            _,
        ] = read_words::<9>(read::range(bytes, 0, HEADER_SIZE)?);
        // The declared size gate excludes the final DWORD, which is still read.
        if u64::from(length) < HEADER_SIZE as u64 + u64::from(row_size) + u64::from(slot_size) {
            return Ok(None);
        }
        let rows = records(bytes, row_offset, row_count, RECORD_SIZE)?
            .map(|record| {
                let [id, flags, assignment, stamp, completed, elapsed] = read_words::<6>(record);
                (
                    id as i32,
                    Progress {
                        flags,
                        assignment,
                        stamp,
                        completed,
                        elapsed,
                    },
                )
            })
            .collect();
        let counters = records(bytes, slot_offset, slot_count, SLOT_SIZE)?
            .filter_map(|record| {
                let [id, rest @ ..] = read_words::<7>(record);
                (id < COUNTERS as u32).then_some((id as usize, rest))
            })
            .collect();
        let attempts = read::u32_at(bytes, attempts_offset as usize)?;
        Ok(Some(Self {
            rows,
            counters,
            attempts,
        }))
    }
}

impl QuestTable {
    pub(super) fn save_length(&self) -> usize {
        HEADER_SIZE + self.rows.len() * RECORD_SIZE + SLOTS_SIZE + 4
    }

    pub(super) fn save(&self) -> Vec<u8> {
        let row_size = self.rows.len() * RECORD_SIZE;
        let slot_offset = HEADER_SIZE + row_size;
        let attempts_offset = slot_offset + SLOTS_SIZE;
        let mut out = Vec::with_capacity(self.save_length());
        append(
            &mut out,
            [
                HEADER_SIZE,
                row_size,
                self.rows.len(),
                slot_offset,
                SLOTS_SIZE,
                COUNTERS,
                attempts_offset,
                4,
                4,
            ]
            .map(|value| value as u32),
        );
        for (&id, record) in &self.rows {
            let p = record.progress;
            append(
                &mut out,
                [
                    id as u32,
                    p.flags,
                    p.assignment,
                    p.stamp,
                    p.completed,
                    p.elapsed,
                ],
            );
        }
        for (index, counters) in self.counters.iter().enumerate() {
            append(&mut out, [index as u32]);
            append(&mut out, *counters);
        }
        append(&mut out, [self.attempts]);
        out
    }

    pub(super) fn restore(&mut self, saved: SavedProgress) {
        for (id, state) in saved.rows {
            if let Some(record) = self.rows.get_mut(&id) {
                record.progress = state;
            }
        }
        for (id, state) in saved.counters {
            self.counters[id] = state;
        }
        self.attempts = saved.attempts;
    }
}

fn records(
    bytes: &[u8],
    offset: u32,
    count: u32,
    width: usize,
) -> Result<impl Iterator<Item = &[u8]>> {
    let count = (count as i32).max(0) as usize;
    if count > read::MAX_ITEMS {
        return Err(Error::new(
            offset as usize,
            "任务记录存档的条目数量超过上限",
        ));
    }
    let section = if count == 0 {
        &[]
    } else {
        read::range(bytes, offset as usize, count * width)?
    };
    Ok(section.chunks_exact(width))
}

fn read_words<const N: usize>(bytes: &[u8]) -> [u32; N] {
    std::array::from_fn(|i| u32::from_le_bytes(bytes[i * 4..(i + 1) * 4].try_into().unwrap()))
}

fn append(out: &mut Vec<u8>, words: impl IntoIterator<Item = u32>) {
    out.extend(words.into_iter().flat_map(u32::to_le_bytes));
}
