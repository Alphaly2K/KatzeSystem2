use super::{ImageList, ImageListProperties, Images, Rect};
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, TaskObject, kind};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result};
use std::collections::BTreeMap;

struct SoundBinding {
    channel: u32,
    threshold: u32,
}

/// 类型 15 的图像列表任务；接收像素、HG-3 原始数据和捕获图像来源。
pub struct ImageListObject {
    id: u32,
    base: BaseObject,
    list: Option<ImageList>,
    plane: Option<u32>,
    offset: [i32; 2],
    submit: u32,
    force: u32,
    enabled: u32,
    background: Option<u32>,
    background_active: bool,
    sound_bindings: BTreeMap<i32, SoundBinding>,
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
            background: None,
            background_active: false,
            sound_bindings: BTreeMap::new(),
        }
    }

    pub fn list(&self) -> Option<&ImageList> {
        self.list.as_ref()
    }

    pub fn plane(&self) -> Option<u32> {
        self.plane
    }

    /// 背景快照包装；清除背景后继续保留，随所属任务回收。
    pub fn background_image(&self) -> Option<u32> {
        self.background
    }

    pub fn background_active(&self) -> bool {
        self.background_active
    }

    fn register_background(&mut self, host: &mut dyn Host) -> Result<()> {
        let image = self.background.unwrap();
        self.list
            .as_mut()
            .ok_or_else(|| Error::new(0, "图像列表尚未创建"))?
            .set(0, image, ImageListProperties::default(), images(host)?)
    }

    fn set_background(
        &mut self,
        source: u32,
        rectangle: Option<Rect>,
        host: &mut dyn Host,
    ) -> Result<()> {
        self.list
            .as_mut()
            .ok_or_else(|| Error::new(0, "图像列表尚未创建"))?
            .remove(0, images(host)?)?;
        if source != 0 {
            let snapshot = match self.background {
                Some(image) => image,
                None => {
                    let image = images(host)?.create_empty(self.id)?;
                    self.background = Some(image);
                    image
                }
            };
            images(host)?.copy_snapshot(snapshot, source, rectangle)?;
            self.register_background(host)?;
        }
        if let Some(plane) = self.plane {
            self.send(plane, 47, [0, u32::from(source != 0), 1], None, host)?;
        }
        self.background_active = source != 0;
        Ok(())
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
                external: None,
            },
            &mut 0,
        )?;
        Ok(())
    }

    fn update_list(&mut self, host: &mut dyn Host, clip: Option<Rect>) -> Result<()> {
        if let Some(list) = &mut self.list {
            host.update_image_list(list, self.submit != 0, self.force != 0, clip)?;
            // 原入口返回的是提交路径是否运行，与像素访问开关及脏区数量无关。
            if self.submit != 0 && (self.force != 0 || list.has_blocks()) {
                self.force = 0;
            }
        }
        Ok(())
    }

    fn sound_task(&self, host: &mut dyn Host) -> Result<u32> {
        let mut target = 0;
        host.dispatch_task_command(
            Command {
                target: 0,
                sender: self.id,
                code: 35,
                data: [0; 3],
            },
            &mut CommandBuffers::default(),
            &mut target,
        )?;
        Ok(target)
    }

    fn sound_command(
        &self,
        target: u32,
        code: u32,
        data: [u32; 3],
        host: &mut dyn Host,
    ) -> Result<u32> {
        let mut output = 0;
        host.dispatch_task_command(
            Command {
                target,
                sender: self.id,
                code,
                data,
            },
            &mut CommandBuffers::default(),
            &mut output,
        )?;
        Ok(output)
    }

    fn update_sound_bindings(&mut self, host: &mut dyn Host) -> Result<()> {
        if self.sound_bindings.is_empty() {
            return Ok(());
        }
        let target = self.sound_task(host)?;
        for (&key, binding) in &self.sound_bindings {
            let level = self.sound_command(target, 213, [binding.channel, 0, 0], host)?;
            if let Some(list) = &mut self.list {
                list.set_animation_variable(key as u32, 0, i32::from(level >= binding.threshold))?;
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

    fn update_body(&mut self, host: &mut dyn Host, increment: u32, budget: usize) -> Result<bool> {
        if !self
            .list
            .as_ref()
            .is_some_and(ImageList::animations_pending)
        {
            self.update_sound_bindings(host)?;
        }
        if self.enabled != 0 {
            if let Some(list) = &mut self.list {
                if !host.update_image_list_animations(list, increment, budget)? {
                    return Ok(false);
                }
            }
            self.update_list(host, None)?;
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
            286 | 288 => {
                if let Some(list) = &mut self.list {
                    let data = words::<15>(buffers.data1.as_deref())?;
                    if data[0] != 0 || data[1] != 0 || data[3] != 0 {
                        if data[13] != 0 {
                            return Err(Error::new(0, "图像列表变换来源尚未接入"));
                        }
                        let properties = ImageListProperties {
                            position: [data[4], data[5]].map(f32::from_bits),
                            base_point: [data[6], data[7]].map(f32::from_bits),
                            opacity: data[11],
                            visible: data[12] != 0,
                        };
                        if data[0] != 0 {
                            list.set_pixels(key, data[0], properties, images(host)?)?;
                        } else if data[1] != 0 {
                            if command.code == 288 {
                                return Err(Error::new(0, "图像列表原始来源属性更新尚未接入"));
                            }
                            let frames = ImageList::decode_hg3(host.image_data(data[1])?)?;
                            list.set_frames(key, frames, properties, images(host)?)?;
                        } else {
                            list.set_resource(key, data[3], properties, images(host)?)?;
                        }
                    } else if data[1..4].iter().any(|&value| value != 0) {
                        return Err(Error::new(0, "图像列表其他来源类型尚未接入"));
                    }
                    if command.code == 286 && data[14] != 0 {
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
                let clip = if command.code == 316 && self.list.is_some() {
                    if b != 0 && buffers.data1.is_none() {
                        return Err(Error::new(0, "图像列表局部更新缓冲区缺失"));
                    }
                    buffers
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
                        .transpose()?
                } else {
                    None
                };
                self.update_list(host, clip)?;
            }
            297 => {
                if let Some(list) = &mut self.list {
                    list.clear(images(host)?)?;
                }
                if self.background_active {
                    self.register_background(host)?;
                }
            }
            302 => {
                let rectangle = if b != 0 {
                    buffers
                        .data0
                        .as_deref()
                        .map(|buffer| {
                            let [left, top, right, bottom] = words::<4>(Some(buffer))?;
                            Ok(Rect {
                                left: left as i32,
                                top: top as i32,
                                right: right as i32,
                                bottom: bottom as i32,
                            })
                        })
                        .transpose()?
                } else {
                    None
                };
                self.set_background(b, rectangle, host)?;
            }
            298 => {
                self.enabled = 1;
                if let Some(list) = &mut self.list {
                    list.invalidate_all();
                    host.update_image_list(list, true, true, None)?;
                    self.force = 0;
                }
            }
            299 => {
                let bytes = buffers
                    .data1
                    .as_deref()
                    .ok_or_else(|| Error::new(0, "图像列表动画名称缓冲缺失"))?;
                let end = bytes
                    .iter()
                    .position(|&byte| byte == 0)
                    .ok_or_else(|| Error::new(0, "图像列表动画名称缺少结尾零"))?;
                let services = host
                    .resource_services()
                    .ok_or_else(|| Error::new(0, "宿主未提供动画资源读取服务"))?;
                let bytes =
                    services.read(&bytes[..end], crate::resource::ResourceMode::LooseFirst)?;
                if let Some(bytes) = bytes.filter(|bytes| !bytes.is_empty()) {
                    self.list
                        .as_mut()
                        .ok_or_else(|| Error::new(0, "图像列表尚未创建"))?
                        .load_animation(key, &bytes)?;
                }
            }
            301 => {
                let now = self.base.clock().elapsed();
                let list = self
                    .list
                    .as_mut()
                    .ok_or_else(|| Error::new(0, "图像列表尚未创建"))?;
                if !list.animations_pending() {
                    if !list.set_animation_time(key, now)? {
                        return Ok(CommandOutcome::finished(false));
                    }
                    list.set_animation_playing(key, b as i32)?;
                }
                if !host.update_image_list_animations(list, now, budget)? {
                    return Ok(CommandOutcome::RESUME);
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
            306 => {
                let list = self
                    .list
                    .as_mut()
                    .ok_or_else(|| Error::new(0, "图像列表尚未创建"))?;
                if !host.restart_image_list_animation(list, key, budget)? {
                    return Ok(CommandOutcome::RESUME);
                }
            }
            307 | 308 => {
                let list = self
                    .list
                    .as_ref()
                    .ok_or_else(|| Error::new(0, "图像列表尚未创建"))?;
                let key = if command.code == 307 {
                    list.find_pixels(a, images(host)?)?
                } else {
                    list.find_source_identity(a)
                };
                *output = key.is_some() as u32;
                if let Some(key) = key {
                    write(buffers.data1.as_deref_mut(), key >> 16)?;
                }
            }
            310 => self.enabled = b,
            313 => {
                write(buffers.data1.as_deref_mut(), 0)?;
                if let Some(list) = &self.list {
                    let pixels = list.source_pixels(key, images(host)?)?;
                    write(buffers.data1.as_deref_mut(), pixels)?;
                }
            }
            314 => {
                if let Some(list) = &mut self.list {
                    let size = [a, b];
                    let tiles = host
                        .canvas_layout()?
                        .map_or([size[0].max(size[1]).max(1); 2], |layout| {
                            layout.tile_size()
                        });
                    list.resize(size, tiles, images(host)?)?;
                }
            }
            318 => {
                let [index, value] = words::<2>(buffers.data1.as_deref())?;
                self.list
                    .as_mut()
                    .ok_or_else(|| Error::new(0, "图像列表尚未创建"))?
                    .set_animation_variable(key, index, value as i32)?;
            }
            317 => {
                let target = self.sound_task(host)?;
                if b == u32::MAX {
                    if let Some(binding) = self.sound_bindings.remove(&(key as i32)) {
                        self.sound_command(target, 212, [binding.channel, 0, 0], host)?;
                        if let Some(list) = &mut self.list {
                            list.set_animation_variable(key, 0, 0)?;
                        }
                    }
                } else {
                    self.sound_bindings
                        .entry(key as i32)
                        .and_modify(|binding| binding.channel = b)
                        .or_insert(SoundBinding {
                            channel: b,
                            threshold: 1000,
                        });
                    self.sound_command(target, 212, [b, 1, 0], host)?;
                }
            }
            319 => {
                if let Some(binding) = self.sound_bindings.get_mut(&(key as i32)) {
                    binding.threshold = b;
                }
            }
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

    fn prepare_release(&mut self, host: &mut dyn Host, _budget: usize) -> Result<bool> {
        if let Some(mut list) = self.list.take() {
            list.release(images(host)?)?;
        }
        Ok(true)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kcs::NativeReturn;

    #[derive(Default)]
    struct SoundHost {
        level: u32,
        controls: Vec<[u32; 3]>,
    }
    impl Host for SoundHost {
        fn call(&mut self, _: u32, _: &[u8], _: &mut Interpreter) -> Result<NativeReturn> {
            unreachable!()
        }
        fn dispatch_task_command(
            &mut self,
            command: Command,
            _: &mut CommandBuffers<'_>,
            output: &mut u32,
        ) -> Result<bool> {
            assert_eq!(command.sender, 7);
            match command.code {
                35 => {
                    assert_eq!(command.target, 0);
                    *output = 42;
                }
                212 => {
                    assert_eq!(command.target, 42);
                    self.controls.push(command.data);
                }
                213 => {
                    assert_eq!(command.target, 42);
                    *output = self.level;
                }
                _ => unreachable!(),
            }
            Ok(false)
        }
    }
    fn send(
        object: &mut ImageListObject,
        host: &mut SoundHost,
        sender: u32,
        code: u32,
        value: u32,
    ) {
        let mut output = 0x12345678;
        object
            .dispatch_command(
                kind::IMAGE_LIST,
                &Command {
                    target: 7,
                    sender,
                    code,
                    data: [90, value, 0],
                },
                &mut output,
                host,
                1,
            )
            .unwrap();
        assert_eq!(output, 0x12345678);
    }
    fn variable(object: &ImageListObject, sender: u32) -> i32 {
        object
            .list()
            .unwrap()
            .animation(ImageList::key(sender, 90))
            .unwrap()
            .timeline
            .as_ref()
            .unwrap()
            .interpreter
            .variables[0]
    }
    #[test]
    fn sound_bindings_keep_sender_keys_unsigned_thresholds_and_rebinding_cleanup() {
        let mut images = Images::default();
        let source = images.create(99, [1, 1]).unwrap();
        let mut list = ImageList::new(7, [1, 1], &mut images).unwrap();
        let mut animation = vec![0; 100];
        animation[..4].copy_from_slice(b"ANM\0");
        animation[8..12].copy_from_slice(&1u32.to_le_bytes());
        for sender in [10, 11] {
            let key = ImageList::key(sender, 90);
            list.set(key, source, ImageListProperties::default(), &mut images)
                .unwrap();
            list.load_animation(key, &animation).unwrap();
        }
        let mut object = ImageListObject::new(7);
        object.list = Some(list);
        let mut host = SoundHost::default();
        send(&mut object, &mut host, 10, 317, 16);
        host.level = 999;
        object.update_sound_bindings(&mut host).unwrap();
        assert_eq!(variable(&object, 10), 0);
        host.level = 1000;
        object.update_sound_bindings(&mut host).unwrap();
        assert_eq!(variable(&object, 10), 1);
        send(&mut object, &mut host, 10, 319, u32::MAX);
        send(&mut object, &mut host, 10, 317, 17);
        send(&mut object, &mut host, 11, 317, 18);
        object.update_sound_bindings(&mut host).unwrap();
        assert_eq!(variable(&object, 10), 0);
        assert_eq!(variable(&object, 11), 1);
        host.level = u32::MAX;
        object.update_sound_bindings(&mut host).unwrap();
        assert_eq!(variable(&object, 10), 1);
        send(&mut object, &mut host, 10, 317, u32::MAX);
        assert_eq!(variable(&object, 10), 0);
        host.level = 0;
        object.update_sound_bindings(&mut host).unwrap();
        assert_eq!(variable(&object, 11), 0);
        assert_eq!(
            host.controls,
            [[16, 1, 0], [17, 1, 0], [18, 1, 0], [17, 0, 0]]
        );
        send(&mut object, &mut host, 10, 319, 0);
        send(&mut object, &mut host, 10, 317, u32::MAX);
        assert_eq!(host.controls.len(), 4);
    }
}
