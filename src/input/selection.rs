use super::{Region, Regions};
use crate::task::Command;
use crate::{Error, Result, read};

impl Regions {
    /// point 为逻辑坐标，None 使用基线的 i32::MIN 哨兵坐标。
    /// 回调响应不参与命中；离开通知先于清标记，进入通知后于设标记。
    pub fn update_pointer_selection(
        &mut self,
        sender: u32,
        point: Option<[i32; 2]>,
        mut route: impl FnMut(&mut Self, Command) -> Result<()>,
    ) -> Result<()> {
        let point = point.unwrap_or([i32::MIN; 2]);
        let mut candidate = self
            .records
            .iter()
            .find(|region| eligible(region) && contains(region, point))
            .map_or(u32::MAX, Region::id);
        let mut index = 0;
        let mut visits = 0;
        while let Some(region) = self.records.get(index) {
            visits += 1;
            if visits > read::MAX_ITEMS {
                return Err(Error::new(0, "输入选择重算遍历超过上限"));
            }
            if !eligible(region) || region.selected() == 0 {
                index += 1;
                continue;
            }
            if region.id() == candidate {
                candidate = u32::MAX;
                index += 1;
                continue;
            }
            let instance = region.instance;
            route(self, packet(sender, region.id(), 8, point))?;
            index = self.current(index, instance)?.0;
            self.records[index].words[7] = 0;
            index += 1;
        }
        if candidate != u32::MAX {
            if let Some(region) = self
                .records
                .iter_mut()
                .find(|region| region.id() == candidate && region.selected() == 0)
            {
                region.words[7] = 1;
                route(self, packet(sender, candidate, 7, point))?;
            }
        }
        Ok(())
    }
}

fn eligible(region: &Region) -> bool {
    region.enabled() != 0 && region.selection_enabled() != 0
}

fn contains(region: &Region, point: [i32; 2]) -> bool {
    let rect = region.bounds();
    if let Some(mask) = region.hit_mask() {
        return mask.contains([rect.left, rect.top], point);
    }
    let [x, y] = point;
    rect.left <= x && x < rect.right && rect.top <= y && y < rect.bottom
}

fn packet(sender: u32, target: u32, code: u32, point: [i32; 2]) -> Command {
    Command {
        sender,
        target,
        code,
        data: [point[0] as u32, point[1] as u32, 0],
    }
}
