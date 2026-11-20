use crate::{Error, Result, kcs::ScriptRandom};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SoundTable {
    entries: Vec<(bool, Vec<u8>)>,
    order: Vec<usize>,
    cursor: Option<usize>,
    pub(crate) repeat: bool,
    shuffled: bool,
}

pub(crate) enum TrackSelection {
    Load(Vec<u8>),
    Stop,
}

impl SoundTable {
    pub(super) fn resize(&mut self, count: i32) -> Result<()> {
        let count = usize::try_from(count)
            .ok()
            .filter(|&count| count <= crate::read::MAX_ITEMS)
            .ok_or_else(|| Error::new(0, "FES SNDTABLE 曲目数量超出范围"))?;
        self.entries = vec![(false, Vec::new()); count];
        self.order = (0..count).collect();
        self.cursor = self.cursor.map(|_| 0).filter(|_| count > 0);
        self.shuffled = false;
        Ok(())
    }

    pub(crate) fn set_track(&mut self, id: i32, file: &[u8], enabled: bool) -> Result<()> {
        if let Some(entry) = self.entries.get_mut(id as usize) {
            *entry = (enabled, super::name(file, 32)?.to_vec());
        }
        Ok(())
    }

    pub(crate) fn id(&self) -> i32 {
        self.cursor.map_or(0, |at| self.order[at] as i32)
    }

    fn enabled(&self, position: usize) -> bool {
        self.entries[self.order[position]].0
    }

    pub(crate) fn next(&mut self) -> TrackSelection {
        self.select(self.cursor.map_or(0, |at| at as i32 + 1))
    }

    pub(crate) fn previous(&mut self) -> TrackSelection {
        let position = self
            .cursor
            .and_then(|at| (0..at).rfind(|&i| self.enabled(i)));
        self.select(position.map_or(-1, |at| at as i32))
    }

    pub(crate) fn select(&mut self, position: i32) -> TrackSelection {
        let count = self.order.len();
        if count == 0 {
            return TrackSelection::Stop;
        }
        let start = if position < 0 {
            if self.repeat { count - 1 } else { 0 }
        } else if position as usize >= count {
            if self.repeat { 0 } else { count - 1 }
        } else {
            position as usize
        };
        let mut position = (start..count).find(|&at| self.enabled(at));
        if position.is_none() && self.repeat {
            position = (0..self.cursor.unwrap_or(0)).find(|&at| self.enabled(at));
        }
        if let Some(position) = position {
            self.cursor = Some(position);
            TrackSelection::Load(self.entries[self.order[position]].1.clone())
        } else {
            self.cursor = (0..count).rfind(|&at| self.enabled(at));
            TrackSelection::Stop
        }
    }

    pub(crate) fn reset_order(&mut self) {
        self.cursor = self.cursor.map(|at| self.order[at]);
        for (id, entry) in self.order.iter_mut().enumerate() {
            *entry = id;
        }
        self.shuffled = false;
    }

    pub(crate) fn shuffle(
        &mut self,
        first: i32,
        random: &mut ScriptRandom,
        playing: bool,
        pending_load: bool,
    ) -> Option<Vec<u8>> {
        let count = self.order.len();
        if count == 0 {
            return None;
        }
        let replace = pending_load || (playing && self.shuffled);
        self.reset_order();
        for _ in 0..1000 {
            let a = random.next_integer() as usize % count;
            let b = random.next_integer() as usize % count;
            self.order.swap(a, b);
        }
        if (first as usize) < count {
            let at = self
                .order
                .iter()
                .position(|&id| id == first as usize)
                .unwrap();
            self.order.swap(0, at);
        }
        if let Some(at) = (0..count).find(|&at| self.enabled(at)) {
            self.cursor = Some(at);
        }
        self.shuffled = true;
        self.cursor
            .filter(|_| replace)
            .map(|at| self.entries[self.order[at]].1.clone())
    }
}
