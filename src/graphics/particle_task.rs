use super::{Drawable, ParticleMesh, Surface};
use super::{particle_anger::Anger, particle_notice::Notice, particle_plugin, particle_rain::Rain};
use crate::kcs::{
    Host, Interpreter, ParticleHost, ParticleSource, ParticleTask, Program, ScriptRandom,
};
use crate::resource::ResourceMode;
use crate::runtime::{BaseObject, TaskObject};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};
use std::sync::Arc;

enum ParticleProgram {
    Script(Program),
    Builtin(particle_plugin::Kind),
}

enum Effect {
    Rain(Rain),
    Notice(Notice),
    Anger(Anger),
}

impl Effect {
    fn new(
        kind: particle_plugin::Kind,
        host: &ParticleHost,
        image: [f32; 2],
        random: &mut ScriptRandom,
    ) -> Self {
        match kind {
            particle_plugin::Kind::Rain => Self::Rain(Rain::new(host, image, random)),
            particle_plugin::Kind::Notice => Self::Notice(Notice::new(host, image)),
            particle_plugin::Kind::Anger => Self::Anger(Anger::new(host, image, random)),
        }
    }
}

pub(crate) fn refresh_particle_source(particles: &mut ParticleHost, drawings: &super::Drawables) {
    if particles.object_id == -1 {
        particles.source = None;
    } else if let Some(drawable) = drawings.get(particles.object_id as u32) {
        let position = drawable.transform().position();
        particles.source = Some(ParticleSource {
            color: drawable.color().0,
            position_2d: [position[0], position[1]],
            position_3d: drawable.transform().base_point(),
        });
    }
}

enum ParticleRunner {
    Script(Box<ParticleTask>),
    Builtin {
        kind: particle_plugin::Kind,
        host: Box<ParticleHost>,
        effect: Option<Effect>,
        origin: u32,
        previous_time: u32,
    },
}

impl ParticleRunner {
    fn host(&self) -> &ParticleHost {
        match self {
            Self::Script(task) => &task.host,
            Self::Builtin { host, .. } => host,
        }
    }

    fn host_mut(&mut self) -> &mut ParticleHost {
        match self {
            Self::Script(task) => &mut task.host,
            Self::Builtin { host, .. } => host,
        }
    }

    fn start(&mut self, image: [f32; 2], services: &mut dyn Host) -> Result<()> {
        let particles = self.host_mut();
        particles.reset_quads(particles.quads.len(), image)?;
        particles.active = true;
        match self {
            Self::Script(task) => task.origin = 0,
            Self::Builtin {
                kind,
                host,
                effect,
                origin,
                ..
            } => {
                *origin = 0;
                *effect = Some(if let Some(random) = services.random() {
                    Effect::new(*kind, host, image, random)
                } else {
                    let mut random = std::mem::take(&mut host.random);
                    let effect = Effect::new(*kind, host, image, &mut random);
                    host.random = random;
                    effect
                });
            }
        }
        Ok(())
    }

    fn update(&mut self, services: &mut dyn Host, now: u32, budget: usize) -> Result<bool> {
        match self {
            Self::Script(task) => services.update_particle(task, now, budget),
            Self::Builtin {
                host,
                effect,
                origin,
                previous_time,
                ..
            } => {
                if !host.active {
                    return Ok(true);
                }
                host.surface_size = services.surface_size();
                if host.object_id == -1 {
                    host.source = None;
                } else {
                    refresh_particle_source(
                        host,
                        services
                            .drawables()
                            .ok_or_else(|| Error::new(0, "宿主未提供粒子来源查询服务"))?,
                    );
                }
                if *origin == 0 {
                    *origin = now;
                    host.elapsed = 0;
                    host.previous_elapsed = u32::MAX;
                } else {
                    host.elapsed = host.elapsed.wrapping_add(now.wrapping_sub(*previous_time));
                }
                *previous_time = now;
                if let Some(effect) = effect {
                    let delta = host.elapsed.wrapping_sub(host.previous_elapsed);
                    match effect {
                        Effect::Rain(effect) => {
                            effect.update(delta, services.random().unwrap_or(&mut host.random));
                            effect.write_mesh(host);
                        }
                        Effect::Notice(effect) => {
                            effect.update(delta, host.source.unwrap_or_default().position_2d);
                            effect.write_mesh(host);
                        }
                        Effect::Anger(effect) => {
                            effect.update(
                                delta,
                                host.source.unwrap_or_default().position_2d,
                                services.random().unwrap_or(&mut host.random),
                            );
                            effect.write_mesh(host);
                        }
                    }
                }
                host.previous_elapsed = host.elapsed;
                Ok(true)
            }
        }
    }
}

/// 类型 16 的三维粒子对象，支持 KCS 脚本及已识别的内置效果。
pub struct Particle3DObject {
    id: u32,
    base: BaseObject,
    context: Vec<u8>,
    task: Option<ParticleRunner>,
    image_size: [f32; 2],
    parameters: [u32; 32],
    opacity: u32,
    source: i32,
}

impl Particle3DObject {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            base: BaseObject::new(),
            context: Vec::new(),
            task: None,
            image_size: [0.; 2],
            parameters: [0; 32],
            opacity: 256,
            source: -1,
        }
    }

    pub fn task(&self) -> Option<&ParticleTask> {
        match &self.task {
            Some(ParticleRunner::Script(task)) => Some(task),
            _ => None,
        }
    }

    fn clear(&mut self, host: &mut dyn Host) -> Result<()> {
        self.task = None;
        let drawings = host
            .drawables()
            .ok_or_else(|| Error::new(0, "宿主未提供粒子绘图服务"))?;
        drawings.update(self.id, |drawable| {
            drawable.particle_mesh = None;
            drawable.display = 0;
        });
        Ok(())
    }

    fn load(&mut self, record: &[u8], host: &mut dyn Host) -> Result<()> {
        read::range(record, 0, 0x190)?;
        let mode = read::u32_at(record, 0)?;
        if mode > 1 {
            return Err(Error::new(0, "粒子装载模式无效"));
        }
        if read::u32_at(record, 0x18c)? != 0 {
            return Err(Error::new(0x18c, "粒子外部绘图模式尚未接入"));
        }
        self.clear(host)?;
        let name = |at| -> Result<&[u8]> { read::zstr(&record[..at + 128], at) };
        let (program, image) = {
            let services = host
                .resource_services()
                .ok_or_else(|| Error::new(0, "宿主未提供粒子资源服务"))?;
            let read_mode = if mode == 1 {
                ResourceMode::File
            } else {
                ResourceMode::LooseFirst
            };
            let Some(bytes) = services.read(name(4)?, read_mode)? else {
                return Ok(());
            };
            let program = if mode == 1 {
                ParticleProgram::Builtin(
                    particle_plugin::identify(&bytes)
                        .ok_or_else(|| Error::new(0, "粒子插件类型或版本尚未接入"))?,
                )
            } else {
                ParticleProgram::Script(Program::parse(&bytes)?)
            };
            let image = if name(0x84)?.is_empty() {
                None
            } else {
                services
                    .read(name(0x84)?, ResourceMode::LooseFirst)?
                    .map(|bytes| crate::hg3::decode_frame(&bytes, 0))
                    .transpose()?
                    .flatten()
            };
            (program, image)
        };
        self.parameters = std::array::from_fn(|i| {
            u32::from_le_bytes(record[0x10c + i * 4..0x110 + i * 4].try_into().unwrap())
        });
        self.image_size = image
            .as_ref()
            .map_or([256.; 2], |frame| frame.info.size.map(|n| n as f32));
        let texture = image
            .map(|frame| Surface::from_bgra(frame.info.size, frame.pixels).map(Arc::new))
            .transpose()?;
        let depth = f32::from_bits(read::u32_at(record, 0x108)?);
        let reference = (65000. - depth) - 1000.;
        let mut particles = ParticleHost {
            active: false,
            surface_size: host.surface_size(),
            depth_offset: depth,
            camera_depth: reference,
            object_id: self.source,
            parameters: self.parameters,
            ..ParticleHost::default()
        };
        particles.reset_quads(read::u32_at(record, 0x104)? as usize, self.image_size)?;
        let mesh = ParticleMesh::new(&particles, texture, reference)?;
        self.task = Some(match program {
            ParticleProgram::Script(program) => {
                ParticleRunner::Script(Box::new(ParticleTask::new(program, particles)?))
            }
            ParticleProgram::Builtin(kind) => ParticleRunner::Builtin {
                kind,
                host: Box::new(particles),
                effect: None,
                origin: 0,
                previous_time: 0,
            },
        });
        let drawings = host.drawables().unwrap();
        drawings.update(self.id, |drawable| {
            drawable.particle_mesh = Some(mesh);
        });
        drawings.set_base_point(self.id, [0., 0., depth]);
        Ok(())
    }

    fn sync(&mut self, host: &mut dyn Host) -> Result<()> {
        let Some(task) = &self.task else {
            return Ok(());
        };
        let particles = task.host();
        let drawings = host
            .drawables()
            .ok_or_else(|| Error::new(0, "宿主未提供粒子绘图服务"))?;
        let drawable = drawings
            .entries
            .get_mut(&self.id)
            .ok_or_else(|| Error::new(0, "粒子绘图登记缺失"))?;
        if let Some(mesh) = &mut drawable.particle_mesh {
            mesh.update(particles)?;
            mesh.opacity = self.opacity;
        }
        drawable.display = u32::from(particles.active);
        drawable.blend_mode = particles.render_mode;
        Ok(())
    }
}

impl TaskObject for Particle3DObject {
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
        self.task().map(|task| &task.interpreter)
    }
    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        match &mut self.task {
            Some(ParticleRunner::Script(task)) => Some(&mut task.interpreter),
            _ => None,
        }
    }
    fn creation_context(&self) -> Option<&[u8]> {
        Some(&self.context)
    }
    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.context = context.to_vec();
        Ok(())
    }
    fn register_graphics(&mut self, drawings: &mut super::Drawables) -> Result<()> {
        drawings.insert(self.id, Drawable::default())?;
        Ok(())
    }

    fn update_body(&mut self, host: &mut dyn Host, _: u32, budget: usize) -> Result<bool> {
        if let Some(task) = &mut self.task {
            if !task.update(host, self.base.clock.elapsed(), budget)? {
                return Ok(false);
            }
        }
        self.sync(host)?;
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
        match command.code {
            320 => self.load(
                buffers
                    .data1
                    .as_deref()
                    .ok_or_else(|| Error::new(0, "粒子装载命令需要配置缓冲区"))?,
                host,
            )?,
            321 => {
                if let Some(task) = &mut self.task {
                    task.start(self.image_size, host)?;
                }
                self.sync(host)?;
            }
            322 => {
                if let Some(task) = &mut self.task {
                    task.host_mut().active = false;
                }
                self.sync(host)?;
            }
            323 => self.clear(host)?,
            324 => *output = u32::from(self.task.as_ref().is_some_and(|task| task.host().active)),
            325 => {
                let bytes = buffers
                    .data1
                    .as_deref()
                    .ok_or_else(|| Error::new(0, "粒子参数命令需要缓冲区"))?;
                read::range(bytes, 0, 128)?;
                self.parameters = std::array::from_fn(|i| {
                    u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                });
                if let Some(task) = &mut self.task {
                    task.host_mut().parameters = self.parameters;
                }
            }
            326 => {
                self.opacity = if (command.data[1] as i32) < 255 {
                    command.data[1]
                } else {
                    256
                };
                self.sync(host)?;
            }
            327 => {
                self.source = if command.data[1] == 0 {
                    -1
                } else {
                    command.data[1] as i32
                };
                if let Some(task) = &mut self.task {
                    task.host_mut().object_id = self.source;
                }
            }
            591 => {
                let mut camera = 0;
                host.dispatch_task_command(
                    Command {
                        target: command.data[1],
                        sender: self.id,
                        code: 589,
                        data: [0; 3],
                    },
                    &mut CommandBuffers::default(),
                    &mut camera,
                )?;
                host.drawables()
                    .ok_or_else(|| Error::new(0, "宿主未提供粒子绘图服务"))?
                    .set_camera(self.id, camera);
            }
            592 => {
                host.drawables()
                    .ok_or_else(|| Error::new(0, "宿主未提供粒子绘图服务"))?
                    .set_camera(self.id, 0);
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
