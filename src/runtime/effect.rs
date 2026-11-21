use super::{BaseObject, ObjectFactory, ObjectHost, TaskObject, route_command};
use crate::graphics::{Drawable, Drawables, query_geometry};
use crate::host::effect::EffectPlayer;
use crate::kcs::{Host, Interpreter, NativeReturn, format_string};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};

pub(super) struct EffectObject {
    base: BaseObject,
    id: u32,
    player: Option<Box<dyn EffectPlayer>>,
    loaded: bool,
    instance: Option<u32>,
    playback: Option<bool>,
    color: u32,
    position: [f32; 3],
}

impl EffectObject {
    pub(super) fn new(id: u32) -> Self {
        Self {
            base: BaseObject::new(),
            id,
            player: None,
            loaded: false,
            instance: None,
            playback: None,
            color: u32::MAX,
            position: [0.; 3],
        }
    }

    fn stop(&mut self) {
        if let (Some(player), Some(instance)) = (self.player.as_mut(), self.instance.take()) {
            player.stop(instance);
        }
        self.playback = None;
    }

    fn unload(&mut self) {
        if self.loaded {
            if let Some(player) = self.player.as_mut() {
                player.unload();
            }
            self.loaded = false;
        }
    }

    fn play(&mut self) {
        if let Some(player) = self.player.as_mut() {
            self.instance = player.play(backend_position(self.position));
            if let Some(instance) = self.instance {
                player.set_color(instance, self.color.to_le_bytes());
            }
        }
    }

    fn forward(
        &self,
        code: u32,
        command: &Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
        host: &mut dyn Host,
    ) -> Result<()> {
        let mut previous = None;
        let mut visits = 0;
        while let Some(target) = host.next_task_child(previous)? {
            visits += 1;
            if visits > read::MAX_ITEMS {
                return Err(Error::new(0, "效果子任务传播超过上限"));
            }
            previous = Some(target);
            host.dispatch_task_command(
                Command {
                    target,
                    sender: self.id,
                    code,
                    data: command.data,
                },
                buffers,
                output,
            )?;
        }
        Ok(())
    }
}

fn command_vector(buffers: &CommandBuffers<'_>) -> Result<[f32; 3]> {
    let bytes = buffers
        .data1
        .as_deref()
        .ok_or_else(|| Error::new(0, "缺少效果变换参数"))?;
    Ok([
        f32::from_bits(read::u32_at(bytes, 0)?),
        f32::from_bits(read::u32_at(bytes, 4)?),
        f32::from_bits(read::u32_at(bytes, 8)?),
    ])
}

fn backend_position([x, y, z]: [f32; 3]) -> [f32; 3] {
    [x * 0.0625, y * -0.0625, z]
}

fn configure_camera(player: &mut dyn EffectPlayer, size: [u32; 2], scale: [f32; 3]) {
    let mut projection = [0.; 16];
    for (axis, slot) in [0, 5].into_iter().enumerate() {
        let extent = size[axis] as f32 / (scale[axis] * 16.);
        projection[slot] = (1. / extent) * 2.;
    }
    projection[10] = 1. / 299.;
    projection[14] = -1. / 299.;
    projection[15] = 1.;

    // Fixed camera: the original normalizes its basis with approximate reciprocals.
    let mut view = [0.; 16];
    view[0] = f32::from_bits(0x3f7fffb7);
    view[5] = f32::from_bits(0x3f7fffb9);
    view[10] = f32::from_bits(0x3f7fffbc);
    view[12] = -0.;
    view[13] = -0.;
    view[14] = f32::from_bits(0x42c7ffcb);
    view[15] = 1.;
    player.set_camera(projection, view);
}

impl Drop for EffectObject {
    fn drop(&mut self) {
        if self.playback.is_some() {
            self.stop();
        }
        self.unload();
    }
}

impl TaskObject for EffectObject {
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

    fn set_creation_context(&mut self, _: &[u8]) -> Result<()> {
        Ok(())
    }

    fn initialize_resources(&mut self, host: &mut dyn Host) -> Result<()> {
        let size = host.surface_size();
        if let Some(services) = host.effect_services() {
            self.player = services.create_player(self.id)?;
            if let Some(player) = self.player.as_deref_mut() {
                configure_camera(player, size, [1.; 3]);
            }
        }
        Ok(())
    }

    fn register_graphics(&mut self, drawables: &mut Drawables) -> Result<()> {
        if self.player.is_some() {
            drawables.insert(self.id, Drawable::new(Vec::new())?)?;
        }
        Ok(())
    }

    fn update_body(&mut self, _: &mut dyn Host, _: u32, _: usize) -> Result<bool> {
        if let Some(repeat) = self.playback.filter(|_| self.loaded) {
            if let (Some(player), Some(instance)) = (self.player.as_mut(), self.instance) {
                if !player.is_playing(instance) {
                    self.stop();
                    if repeat {
                        self.play();
                        self.playback = Some(true);
                    }
                }
            }
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
        if self.control().deletion_requested == 0 && self.player.is_some() {
            match command.code {
                19 | 63 => {}
                49 => {
                    let point = command_vector(buffers)?;
                    let source = command.data[0] != 0;
                    if let Some(drawables) = host.drawables() {
                        drawables.set_geometry_base_point(self.id, point, source);
                    }
                    self.player.as_mut().unwrap().set_base_point(point, source);
                    self.forward(50, command, buffers, output, host)?;
                }
                51 => {
                    self.position = command_vector(buffers)?;
                    if let Some(instance) = self.instance {
                        self.player
                            .as_mut()
                            .unwrap()
                            .set_location(instance, backend_position(self.position));
                    }
                    self.forward(52, command, buffers, output, host)?;
                }
                55 => {
                    if let Some(instance) = self.instance {
                        let rotation = command_vector(buffers)?;
                        self.player
                            .as_mut()
                            .unwrap()
                            .set_rotation(instance, rotation);
                    }
                    self.forward(56, command, buffers, output, host)?;
                }
                57 => self.forward(58, command, buffers, output, host)?,
                59 => {
                    self.color = command.data[1];
                    if let (Some(player), Some(instance)) = (self.player.as_mut(), self.instance) {
                        player.set_color(instance, self.color.to_le_bytes());
                    }
                }
                61 | 62 => {
                    if let Some(drawables) = host.drawables() {
                        query_geometry(drawables, self.id, command, buffers)?;
                    }
                }
                66 => {
                    let scale = command_vector(buffers)?;
                    configure_camera(
                        self.player.as_deref_mut().unwrap(),
                        host.surface_size(),
                        scale,
                    );
                    self.forward(67, command, buffers, output, host)?;
                }
                578 => {
                    let name = read::zstr(
                        buffers
                            .data1
                            .as_deref()
                            .ok_or_else(|| Error::new(0, "缺少效果名称"))?,
                        0,
                    )?;
                    if self.loaded {
                        self.stop();
                        self.unload();
                    }
                    self.playback = None;
                    if !name.is_empty() {
                        self.loaded = self.player.as_mut().unwrap().load(name)?;
                    }
                }
                579 => self.unload(),
                580 if self.loaded => {
                    self.play();
                    self.playback = Some(command.data[1] != 0);
                }
                580 => {}
                581 if self.loaded => self.stop(),
                581 => self.playback = None,
                582 => {
                    *output = u32::from(
                        self.loaded
                            && self.instance.is_some_and(|instance| {
                                self.player.as_mut().unwrap().is_playing(instance)
                            }),
                    );
                }
                _ => {
                    return self
                        .base
                        .dispatch_command(kind, command, output, host, budget);
                }
            }
            return Ok(CommandOutcome::finished(false));
        }
        self.base
            .dispatch_command(kind, command, output, host, budget)
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

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn effect_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &Interpreter,
    ) -> Result<NativeReturn> {
        let target = read::u32_at(arguments, 0)?;
        let expected = if matches!(index, 1014 | 1016) { 8 } else { 4 };
        if arguments.len() != expected && !(target == 0 && arguments.len() == 4) {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        if target == 0 {
            return Ok(if index == 1076 {
                NativeReturn::Value(0)
            } else {
                NativeReturn::Continue
            });
        }
        match index {
            1014 => {
                let mut name = format_string(vm, read::u32_at(arguments, 4)?, 0)?;
                if let Some(end) = name.iter().position(|&byte| byte == 0) {
                    name.truncate(end);
                }
                if name.len() >= 260 {
                    return Err(Error::new(vm.pc, "KCS 效果名称超过缓冲区上限"));
                }
                name.push(0);
                self.command_target_with_buffers(
                    target,
                    578,
                    [0; 3],
                    &mut CommandBuffers {
                        data1: Some(&mut name),
                        ..Default::default()
                    },
                )?;
            }
            1015 => {
                self.command_target(target, 579, [0; 3])?;
            }
            1016 => {
                self.command_target(target, 580, [0, read::u32_at(arguments, 4)?, 0])?;
            }
            1017 => {
                self.command_target(target, 581, [0; 3])?;
            }
            1076 => {
                let sender = self.host.task_id;
                let mut output = u32::MAX;
                route_command(
                    self.store(),
                    sender,
                    Command {
                        target,
                        sender,
                        code: 582,
                        data: [0; 3],
                    },
                    &mut CommandBuffers::default(),
                    &mut output,
                )?;
                if output == u32::MAX {
                    return Err(Error::new(vm.pc, "效果播放查询未写入结果"));
                }
                return Ok(NativeReturn::Value(output));
            }
            _ => unreachable!(),
        }
        Ok(NativeReturn::Continue)
    }
}
