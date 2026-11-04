use super::Regions;
use crate::task::{Command, CommandBuffers};
use crate::{Error, Result, read};

/// 前后坐标均已转换为逻辑坐标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerMotion {
    pub code: u32,
    pub previous: [i32; 2],
    pub current: [i32; 2],
}

impl Regions {
    /// data0／data1 为共享的前后点缓冲区，回调修改对后续接收者生效。
    /// 原指针 DWORD 用零代替，坐标只通过显式缓冲区传递。
    pub fn dispatch_pointer_motion(
        &mut self,
        sender: u32,
        motion: PointerMotion,
        mut route: impl FnMut(&mut Self, Command, &mut CommandBuffers<'_>) -> Result<()>,
    ) -> Result<()> {
        let mut previous = encode(motion.previous);
        let mut current = encode(motion.current);
        let mut index = 0;
        let mut visits = 0;
        while let Some(region) = self.records.get(index) {
            visits += 1;
            if visits > read::MAX_ITEMS {
                return Err(Error::new(0, "输入移动通知遍历超过上限"));
            }
            let rect = region.bounds();
            let [x, y] = decode(&previous);
            if region.enabled() == 0
                || region.motion_enabled() == 0
                || region.event_mask() & 2 == 0
                || (rect.left < rect.right
                    && rect.top < rect.bottom
                    && (x < rect.left || y < rect.top || x >= rect.right || y >= rect.bottom))
            {
                index += 1;
                continue;
            }
            let instance = region.instance;
            let command = Command {
                target: region.id(),
                sender,
                code: motion.code,
                data: [0; 3],
            };
            route(
                self,
                command,
                &mut CommandBuffers {
                    data0: Some(&mut previous),
                    data1: Some(&mut current),
                    external: None,
                },
            )?;
            index = self.current(index, instance)?.0 + 1;
        }
        Ok(())
    }
}

fn encode(point: [i32; 2]) -> [u8; 8] {
    let mut bytes = [0; 8];
    bytes[..4].copy_from_slice(&point[0].to_le_bytes());
    bytes[4..].copy_from_slice(&point[1].to_le_bytes());
    bytes
}

fn decode(bytes: &[u8; 8]) -> [i32; 2] {
    [
        i32::from_le_bytes(bytes[..4].try_into().unwrap()),
        i32::from_le_bytes(bytes[4..].try_into().unwrap()),
    ]
}
