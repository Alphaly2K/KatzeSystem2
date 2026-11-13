use super::{Image, ProjectedVertex, Rect, Surface, TextureFilter, pixel_length};
use crate::{Error, Result, read};

/// 按行排列的 CPU 渲染面块和对应的更新标记。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderTarget {
    tile_size: [u32; 2],
    grid: [u32; 2],
    size: [u32; 2],
    tiles: Vec<Option<Surface>>,
    dirty: Vec<bool>,
    bit_depths: Vec<u32>,
}

const MAX_CANVAS_BYTES: usize = 128 * 1024 * 1024;

impl RenderTarget {
    /// 创建一块零初始化的完整渲染面。
    pub fn new(size: [u32; 2]) -> Result<Self> {
        Self::from_tiles(size, [1, 1], vec![Some(Surface::new(size)?)])
    }

    /// 每块尺寸必须一致；None 对应尚未提供表面的槽位，像素操作跳过该槽。
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
        if size.iter().any(|&value| value > i32::MAX as u32) {
            return Err(Error::new(0, "画布尺寸超出坐标范围"));
        }
        pixel_length(tile_size)?;
        let count = (grid[0] as usize)
            .checked_mul(grid[1] as usize)
            .filter(|&n| n <= read::MAX_ITEMS)
            .ok_or_else(|| Error::new(0, "渲染面块数量超过上限"))?;
        if tiles.len() != count || tiles.iter().flatten().any(|tile| tile.size() != tile_size) {
            return Err(Error::new(0, "渲染面块布局与尺寸不符"));
        }
        if tiles
            .iter()
            .flatten()
            .map(|s| s.pixels.len())
            .sum::<usize>()
            > MAX_CANVAS_BYTES
        {
            return Err(Error::new(0, "画布像素存储超过上限"));
        }
        let bit_depths = tiles
            .iter()
            .map(|tile| if tile.is_some() { 32 } else { 0 })
            .collect();
        Ok(Self {
            tile_size,
            grid,
            size,
            tiles,
            dirty: vec![false; count],
            bit_depths,
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

    pub fn tile_bit_depth(&self, index: usize) -> Option<u32> {
        self.tiles.get(index)?.as_ref()?;
        Some(self.bit_depths[index])
    }

    /// 按区域补建缺失面块，先处理 16 位行，再处理其余 32 位行。
    /// 保留已有面块、像素及脏标记；区域按有符号回绕端点解释。
    pub fn prepare_tiles(&mut self, areas: &[[u32; 4]], split_row: u32) -> Result<usize> {
        let bytes = pixel_length(self.tile_size)?;
        let mut used = self
            .tiles
            .iter()
            .flatten()
            .map(|s| s.pixels.len())
            .sum::<usize>();
        let mut created = 0;
        let mut visits = 0;
        for use_16 in [true, false] {
            for &[x, y, width, height] in areas {
                if x == u32::MAX {
                    continue;
                }
                let right = x.wrapping_add(width) as i32;
                let bottom = y.wrapping_add(height) as i32;
                if right <= x as i32 || bottom <= y as i32 {
                    continue;
                }
                for row in y as i32..bottom {
                    for column in x as i32..right {
                        visits += 1;
                        if visits > read::MAX_ITEMS {
                            return Err(Error::new(0, "画布面块准备工作量超过上限"));
                        }
                        let index = (row as u32)
                            .wrapping_mul(self.grid[0])
                            .wrapping_add(column as u32)
                            as usize;
                        let tile = self
                            .tiles
                            .get_mut(index)
                            .ok_or_else(|| Error::new(0, "画布区域指向网格外面块"))?;
                        if tile.is_some() || (row >= split_row as i32) != use_16 {
                            continue;
                        }
                        used = used
                            .checked_add(bytes)
                            .filter(|&n| n <= MAX_CANVAS_BYTES)
                            .ok_or_else(|| Error::new(0, "画布像素存储超过上限"))?;
                        *tile = Some(Surface::new(self.tile_size)?);
                        self.bit_depths[index] = if use_16 { 16 } else { 32 };
                        created += 1;
                    }
                }
            }
        }
        Ok(created)
    }

    /// 呈现宿主消费更新后清除标记，不改变像素。
    pub fn clear_dirty(&mut self) {
        self.dirty.fill(false);
    }

    /// 以全局整数像素中心绘制纯色三角形，跨面块保留相同的亚像素边界。
    pub fn blend_triangle(&mut self, points: [[f32; 2]; 3], argb: u32) -> bool {
        self.blend_triangle_clipped(points, argb, super::raster::RasterState::default())
    }

    /// 以全局顶点插值颜色，跨面块保留同一组插值方程。
    pub fn blend_projected_triangle(&mut self, vertices: [ProjectedVertex; 3]) -> bool {
        self.blend_projected_triangle_clipped(vertices, super::raster::RasterState::default())
    }

    /// 在各面块上使用相同的全局覆盖、透视插值及纹理寻址。
    pub fn blend_textured_triangle(
        &mut self,
        vertices: [ProjectedVertex; 3],
        uv: [[f32; 2]; 3],
        texture: &Surface,
        filter: TextureFilter,
    ) -> bool {
        self.blend_textured_triangle_clipped(
            vertices,
            uv,
            texture,
            filter,
            super::raster::RasterState::default(),
        )
    }

    pub(super) fn blend_triangle_clipped(
        &mut self,
        points: [[f32; 2]; 3],
        argb: u32,
        state: super::raster::RasterState,
    ) -> bool {
        self.draw_tiles(|surface, origin| {
            surface.blend_triangle_at(points, argb, super::raster::RasterState { origin, ..state })
        })
    }

    pub(super) fn blend_projected_triangle_clipped(
        &mut self,
        vertices: [ProjectedVertex; 3],
        state: super::raster::RasterState,
    ) -> bool {
        self.draw_tiles(|surface, origin| {
            surface.blend_projected_triangle_at(
                vertices,
                super::raster::RasterState { origin, ..state },
            )
        })
    }

    pub(super) fn blend_textured_triangle_clipped(
        &mut self,
        vertices: [ProjectedVertex; 3],
        uv: [[f32; 2]; 3],
        texture: &Surface,
        filter: TextureFilter,
        state: super::raster::RasterState,
    ) -> bool {
        self.draw_tiles(|surface, origin| {
            surface.blend_textured_triangle_at(
                vertices,
                uv,
                texture,
                filter,
                super::raster::RasterState { origin, ..state },
            )
        })
    }

    fn draw_tiles(&mut self, mut draw: impl FnMut(&mut Surface, [u32; 2]) -> bool) -> bool {
        let mut touched = false;
        for row in 0..self.grid[1] {
            for column in 0..self.grid[0] {
                let index = row as usize * self.grid[0] as usize + column as usize;
                if let Some(surface) = &mut self.tiles[index] {
                    let origin = [column * self.tile_size[0], row * self.tile_size[1]];
                    if draw(surface, origin) {
                        if self.bit_depths[index] == 16 {
                            quantize_16(&mut surface.pixels);
                        }
                        self.dirty[index] = true;
                        touched = true;
                    }
                }
            }
        }
        touched
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

    /// 普通 115 的当前目标分支；源矩形为全局图像坐标，复制原始 BGRA，不混合或缩放。
    pub fn copy_image(&mut self, source: &Image, rect: Rect, at: [i32; 2]) -> bool {
        let info = source.info();
        let Some(rect) = rect.intersect(Rect::from_xywh(
            info.offset[0],
            info.offset[1],
            info.size[0] as i32,
            info.size[1] as i32,
        )) else {
            return false;
        };
        let source_origin = [
            rect.left.wrapping_sub(info.offset[0]),
            rect.top.wrapping_sub(info.offset[1]),
        ];
        let at = [
            at[0].wrapping_add(info.offset[0]),
            at[1].wrapping_add(info.offset[1]),
        ];
        self.copy_surface_region(source.surface(), rect, source_origin, at, true)
    }

    /// 从顶端起始的渲染面复制原始 BGRA，保留 Alpha 和透明像素的 RGB。
    /// 来源独立裁剪，裁剪不平移传入的目标起点。
    pub fn copy_surface(&mut self, source: &Surface, rect: Rect, at: [i32; 2]) -> bool {
        let Some(rect) = rect.intersect(source.bounds()) else {
            return false;
        };
        self.copy_surface_region(source, rect, [rect.left, rect.top], at, false)
    }

    fn copy_surface_region(
        &mut self,
        input: &Surface,
        rect: Rect,
        source_origin: [i32; 2],
        at: [i32; 2],
        bottom_up: bool,
    ) -> bool {
        let target_rect =
            Rect::from_xywh(at[0], at[1], rect.right - rect.left, rect.bottom - rect.top);
        let Some(target_rect) = target_rect.intersect(Rect::from_xywh(
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
                let Some(clipped) = target_rect.intersect(Rect::from_xywh(
                    x,
                    y,
                    self.tile_size[0] as i32,
                    self.tile_size[1] as i32,
                )) else {
                    continue;
                };
                self.dirty[index] = true;
                let width = (clipped.right - clipped.left) as usize * 4;
                let source_x = source_origin[0] + clipped.left - at[0];
                let source_y = source_origin[1] + clipped.top - at[1];
                for line in 0..clipped.bottom - clipped.top {
                    let source_line = if bottom_up {
                        input.size()[1] as i32 - source_y - line - 1
                    } else {
                        source_y + line
                    };
                    let from = source_line as usize * input.stride() + source_x as usize * 4;
                    let dest = (clipped.top - y + line) as usize * surface.stride()
                        + (clipped.left - x) as usize * 4;
                    surface.pixels[dest..dest + width]
                        .copy_from_slice(&input.pixels[from..from + width]);
                    if self.bit_depths[index] == 16 {
                        quantize_16(&mut surface.pixels[dest..dest + width]);
                    }
                }
                touched = true;
            }
        }
        touched
    }
}

fn quantize_16(bytes: &mut [u8]) {
    for channel in bytes {
        *channel = (*channel >> 4) * 17;
    }
}

/// 当前绘图上下文；仅在模式为零、两个开关均开启时操作目标。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrawingContext {
    pub mode: u32,
    pub enabled: bool,
    pub active: bool,
    pub target: Option<RenderTarget>,
}

impl DrawingContext {
    pub fn render_triangles(
        &mut self,
        drawables: &mut super::Drawables,
        projection: &super::Projection,
        parent: [f32; 16],
    ) -> Result<bool> {
        self.active_target().map_or(Ok(false), |target| {
            drawables.render_triangles(target, projection, parent)
        })
    }

    pub fn clear_rect(&mut self, rect: Rect) -> bool {
        self.active_target()
            .is_some_and(|target| target.clear_rect(rect))
    }

    pub fn copy_image(&mut self, source: &Image, rect: Rect, at: [i32; 2]) -> bool {
        self.active_target()
            .is_some_and(|target| target.copy_image(source, rect, at))
    }

    pub fn copy_surface(&mut self, source: &Surface, rect: Rect, at: [i32; 2]) -> bool {
        self.active_target()
            .is_some_and(|target| target.copy_surface(source, rect, at))
    }

    fn active_target(&mut self) -> Option<&mut RenderTarget> {
        if self.mode != 0 || !self.enabled || !self.active {
            return None;
        }
        self.target.as_mut()
    }
}
