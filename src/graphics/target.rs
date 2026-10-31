use super::{Rect, Surface, pixel_length};
use crate::{Error, Result, read};

/// 按行排列的 CPU 渲染面块和对应的更新标记。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderTarget {
    tile_size: [u32; 2],
    grid: [u32; 2],
    size: [u32; 2],
    tiles: Vec<Option<Surface>>,
    dirty: Vec<bool>,
}

impl RenderTarget {
    /// 创建一块零初始化的完整渲染面。
    pub fn new(size: [u32; 2]) -> Result<Self> {
        Self::from_tiles(size, [1, 1], vec![Some(Surface::new(size)?)])
    }

    /// 每块尺寸必须一致；None 对应尚未提供表面的槽位，清除操作跳过该槽。
    pub fn from_tiles(
        tile_size: [u32; 2],
        grid: [u32; 2],
        tiles: Vec<Option<Surface>>,
    ) -> Result<Self> {
        let size = [
            tile_size[0].checked_mul(grid[0]),
            tile_size[1].checked_mul(grid[1]),
        ];
        let [Some(width), Some(height)] = size else {
            return Err(Error::new(0, "渲染目标尺寸溢出"));
        };
        let size = [width, height];
        pixel_length(size)?;
        let count = (grid[0] as usize)
            .checked_mul(grid[1] as usize)
            .filter(|&n| n <= read::MAX_ITEMS)
            .ok_or_else(|| Error::new(0, "渲染面块数量超过上限"))?;
        if tiles.len() != count || tiles.iter().flatten().any(|tile| tile.size() != tile_size) {
            return Err(Error::new(0, "渲染面块布局与尺寸不符"));
        }
        Ok(Self {
            tile_size,
            grid,
            size,
            tiles,
            dirty: vec![false; count],
        })
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn tile_size(&self) -> [u32; 2] {
        self.tile_size
    }

    pub fn grid(&self) -> [u32; 2] {
        self.grid
    }

    pub fn tiles(&self) -> &[Option<Surface>] {
        &self.tiles
    }

    pub fn dirty_tiles(&self) -> &[bool] {
        &self.dirty
    }

    /// 呈现宿主消费更新后清除标记，不改变像素。
    pub fn clear_dirty(&mut self) {
        self.dirty.fill(false);
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<u32> {
        if x >= self.size[0] || y >= self.size[1] {
            return None;
        }
        let column = x / self.tile_size[0];
        let row = y / self.tile_size[1];
        let index = row as usize * self.grid[0] as usize + column as usize;
        self.tiles[index]
            .as_ref()?
            .pixel(x % self.tile_size[0], y % self.tile_size[1])
    }

    /// 按顶端起始坐标清零相交的像素和 Alpha；即使原像素为零也标记该块。
    pub fn clear_rect(&mut self, rect: Rect) -> bool {
        let Some(rect) = rect.intersect(Rect::from_xywh(
            0,
            0,
            self.size[0] as i32,
            self.size[1] as i32,
        )) else {
            return false;
        };
        let mut touched = false;
        for row in 0..self.grid[1] {
            for column in 0..self.grid[0] {
                let index = row as usize * self.grid[0] as usize + column as usize;
                let Some(surface) = &mut self.tiles[index] else {
                    continue;
                };
                let x = (column * self.tile_size[0]) as i32;
                let y = (row * self.tile_size[1]) as i32;
                let Some(clipped) = rect.intersect(Rect::from_xywh(
                    x,
                    y,
                    self.tile_size[0] as i32,
                    self.tile_size[1] as i32,
                )) else {
                    continue;
                };
                self.dirty[index] = true;
                surface.fill_rect(
                    Rect {
                        left: clipped.left - x,
                        top: clipped.top - y,
                        right: clipped.right - x,
                        bottom: clipped.bottom - y,
                    },
                    0,
                );
                touched = true;
            }
        }
        touched
    }
}

/// 当前绘图上下文；117 的零图像分支仅在模式为零、两个开关均开启时清除目标。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrawingContext {
    pub mode: u32,
    pub enabled: bool,
    pub active: bool,
    pub target: Option<RenderTarget>,
}

impl DrawingContext {
    pub fn clear_rect(&mut self, rect: Rect) -> bool {
        if self.mode != 0 || !self.enabled || !self.active {
            return false;
        }
        self.target
            .as_mut()
            .is_some_and(|target| target.clear_rect(rect))
    }
}
