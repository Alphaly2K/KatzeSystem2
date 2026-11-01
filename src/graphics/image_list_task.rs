use super::{ImageList, ImageListProperties, Images, Rect};
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, TaskObject, kind};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result};

/// 类型 15 的图像列表任务；当前接收无变换的像素与 HG-3 原始数据来源。
pub struct ImageListObject {
    id: u32,
    base: BaseObject,
    list: Option<ImageList>,
    plane: Option<u32>,
    offset: [i32; 2],
    submit: u32,
    force: u32,
    enabled: u32,
}

impl ImageListObject {
    /// 原构造未初始化的输出偏移在可移植对象中设为零。
    pub fn new(id: u32) -> Self {
        Self {
            id,
            base: BaseObject::new(),
            list: None,
            plane: None,
            offset: [0; 2],
            submit: 1,
            force: 0,
            enabled: 1,
        }
    }

    pub fn list(&self) -> Option<&ImageList> {
        self.list.as_ref()
    }

    pub fn plane(&self) -> Option<u32> {
        self.plane
    }

    fn initialize(&mut self, size: [u32; 2], host: &mut dyn Host) -> Result<()> {
        if let Some(mut previous) = self.list.take() {
            previous.release(images(host)?)?;
        }
        let tiles = host.canvas_layout()?.map(|layout| layout.tile_size());
        self.list = Some(match tiles {
            Some(tiles) => ImageList::new_tiled(self.id, size, tiles, images(host)?)?,
            None => ImageList::new(self.id, size, images(host)?)?,
        });
        if self.plane.is_none() {
            let size = host.surface_size().map(|value| (value as f32).to_bits());
            let plane = host.create_task_child(kind::PLANE, b"base plane", b"")?;
            self.plane = Some(plane);
            let mut rectangle = [0; 32];
            for (i, value) in [0, 0, 0, size[0], size[1], 32, 1, u32::MAX]
                .into_iter()
                .enumerate()
            {
                rectangle[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
            }
            self.send(plane, 41, [0; 3], Some(&mut rectangle), host)?;
            self.send(plane, 47, [0, 0, 1], None, host)?;
            let mut position = [0; 12];
            position[8..].copy_from_slice(&1000f32.to_le_bytes());
            self.send(plane, 49, [0, 0, 1], Some(&mut position), host)?;
        }
        Ok(())
    }

    fn send(
        &self,
        target: u32,
        code: u32,
        data: [u32; 3],
        buffer: Option<&mut [u8]>,
        host: &mut dyn Host,
    ) -> Result<()> {
        host.dispatch_task_command(
            Command {
                target,
                sender: self.id,
                code,
                data,
            },
            &mut CommandBuffers {
                data0: None,
                data1: buffer,
            },
            &mut 0,
        )?;
        Ok(())
    }

    fn update_list(&mut self, host: &mut dyn Host) -> Result<()> {
        if let Some(list) = &mut self.list {
            host.update_image_list(list, self.submit != 0, self.force != 0)?;
            // 原入口返回的是提交路径是否运行，与像素访问开关及脏区数量无关。
            if self.submit != 0 && (self.force != 0 || list.has_blocks()) {
                self.force = 0;
            }
        }
        Ok(())
    }
}

impl TaskObject for ImageListObject {
    fn control(&self) -> &Control {
        self.base.control()
    }
    fn clock(&self) -> &TaskClock {
        self.base.clock()
    }
    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        self.base.state()
    }
    fn interpreter(&self) -> Option<&Interpreter> {
        None
    }
    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        None
    }
    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.base.set_creation_context(context)
    }

    fn update_body(
        &mut self,
        host: &mut dyn Host,
        _increment: u32,
        _budget: usize,
    ) -> Result<bool> {
        if self.enabled != 0 {
            self.update_list(host)?;
        }
        Ok(true)
    }

    fn dispatch_command(
        &mut self,
        kind: u32,
        command: &Command,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        self.dispatch_command_with_buffers(
            kind,
            command,
            &mut CommandBuffers::default(),
            output,
            host,
            budget,
        )
    }

    fn dispatch_command_with_buffers(
        &mut self,
        kind: u32,
        command: &Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        if self.base.control.deletion_requested != 0 {
            return Ok(CommandOutcome::finished(false));
        }
        let [a, b, _] = command.data;
        let key = ImageList::key(command.sender, a);
        match command.code {
            19 | 300 | 311 | 312 => (),
            285 => self.initialize([a, b], host)?,
            286 => {
                if let Some(list) = &mut self.list {
                    let data = words::<15>(buffers.data1.as_deref())?;
                    if data[0] != 0 || data[1] != 0 {
                        if data[13] != 0 {
                            return Err(Error::new(0, "图像列表变换来源尚未接入"));
                        }
                        let opacity = u8::try_from(data[11])
                            .map_err(|_| Error::new(0, "图像列表超范围透明度尚未核实"))?;
                        let properties = ImageListProperties {
                            position: [data[4], data[5]].map(f32::from_bits),
                            base_point: [data[6], data[7]].map(f32::from_bits),
                            opacity,
                            visible: data[12] != 0,
                        };
                        if data[0] != 0 {
                            list.set_pixels(key, data[0], properties, images(host)?)?;
                        } else {
                            let frames = ImageList::decode_hg3(host.image_data(data[1])?)?;
                            list.set_frames(key, frames, properties, images(host)?)?;
                        }
                    } else if data[1..4].iter().any(|&value| value != 0) {
                        return Err(Error::new(0, "图像列表其他来源类型尚未接入"));
                    }
                    if data[14] != 0 {
                        list.set_source_identity(key, data[14]);
                    }
                }
            }
            287 => {
                if let Some(list) = &mut self.list {
                    list.remove(key, images(host)?)?;
                }
            }
            289 => {
                self.offset = [a as i32, b as i32];
                if let Some(list) = &mut self.list {
                    list.set_offset(self.offset);
                }
            }
            290 => {
                write(buffers.data0.as_deref_mut(), self.offset[0] as u32)?;
                write(buffers.data1.as_deref_mut(), self.offset[1] as u32)?;
            }
            291 => {
                let list = self
                    .list
                    .as_ref()
                    .ok_or_else(|| Error::new(0, "图像列表尚未创建"))?;
                write(buffers.data1.as_deref_mut(), list.image())?;
            }
            292 => {
                self.submit = b;
                self.force = b;
                if b != 0 {
                    self.enabled = 1;
                }
            }
            293 => {
                let list = self
                    .list
                    .as_mut()
                    .ok_or_else(|| Error::new(0, "图像列表尚未创建"))?;
                list.set_visible(key, b != 0, images(host)?)?;
            }
            294 => {
                *output = self
                    .list
                    .as_ref()
                    .ok_or_else(|| Error::new(0, "图像列表尚未创建"))?
                    .len() as u32;
            }
            295 => {
                let list = self
                    .list
                    .as_mut()
                    .ok_or_else(|| Error::new(0, "图像列表尚未创建"))?;
                if !list.contains_key(key) {
                    return Ok(CommandOutcome::finished(false));
                }
                if b != 0 && buffers.data1.is_none() {
                    return Err(Error::new(0, "图像列表裁剪缓冲区缺失"));
                }
                let clip = buffers
                    .data1
                    .as_deref()
                    .map(|buffer| {
                        let [left, top, right, bottom] = words::<4>(Some(buffer))?;
                        Ok::<_, Error>(Rect {
                            left: left as i32,
                            top: top as i32,
                            right: right as i32,
                            bottom: bottom as i32,
                        })
                    })
                    .transpose()?;
                list.set_clip(key, clip, images(host)?)?;
            }
            296 | 316 => {
                self.enabled = 1;
                if command.code == 316 && self.list.is_some() && b != 0 {
                    return Err(Error::new(0, "图像列表局部提交裁剪尚未接入"));
                }
                self.update_list(host)?;
            }
            297 => {
                if let Some(list) = &mut self.list {
                    list.clear(images(host)?)?;
                }
            }
            298 => {
                self.enabled = 1;
                if let Some(list) = &mut self.list {
                    list.invalidate_all();
                    host.update_image_list(list, true, true)?;
                    self.force = 0;
                }
            }
            304 => {
                let list = self
                    .list
                    .as_ref()
                    .ok_or_else(|| Error::new(0, "图像列表尚未创建"))?;
                let priority = list.priority(key);
                if let Some(priority) = priority {
                    write(buffers.data1.as_deref_mut(), priority as u32)?;
                }
                *output = priority.is_some() as u32;
            }
            305 => {
                let list = self
                    .list
                    .as_mut()
                    .ok_or_else(|| Error::new(0, "图像列表尚未创建"))?;
                *output = list.set_priority(key, b as i32, images(host)?)? as u32;
            }
            308 => {
                let list = self
                    .list
                    .as_ref()
                    .ok_or_else(|| Error::new(0, "图像列表尚未创建"))?;
                let key = list.find_source_identity(a);
                *output = key.is_some() as u32;
                if let Some(key) = key {
                    write(buffers.data1.as_deref_mut(), key >> 16)?;
                }
            }
            310 => self.enabled = b,
            288..=308 | 310..=319 => {
                return Err(Error::new(
                    0,
                    format!("图像列表命令 {} 尚未接入", command.code),
                ));
            }
            _ => {
                return self
                    .base
                    .dispatch_command(kind, command, output, host, budget);
            }
        }
        Ok(CommandOutcome::finished(false))
    }

    fn enqueue_message(&mut self, message: [u32; 4]) -> Result<()> {
        self.base.enqueue_message(message)
    }
    fn dispatch_message(
        &mut self,
        message: &mut [u32; 4],
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<bool> {
        self.base.dispatch_message(message, output, host, budget)
    }
    fn request_delete(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool> {
        self.base.request_delete(host, budget)
    }
}

fn images(host: &mut dyn Host) -> Result<&mut Images> {
    host.images_mut()
        .ok_or_else(|| Error::new(0, "宿主未提供图像存储"))
}

fn words<const N: usize>(buffer: Option<&[u8]>) -> Result<[u32; N]> {
    let buffer = buffer
        .filter(|buffer| buffer.len() >= N * 4)
        .ok_or_else(|| Error::new(0, "图像列表输入缓冲区缺失或过短"))?;
    Ok(std::array::from_fn(|i| {
        u32::from_le_bytes(buffer[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}

fn write(buffer: Option<&mut [u8]>, value: u32) -> Result<()> {
    let buffer = buffer
        .filter(|buffer| buffer.len() >= 4)
        .ok_or_else(|| Error::new(0, "图像列表输出缓冲区缺失或过短"))?;
    buffer[..4].copy_from_slice(&value.to_le_bytes());
    Ok(())
}
