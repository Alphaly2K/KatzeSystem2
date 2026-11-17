use super::{ObjectState, Objects};
use crate::runtime::{ObjectFactory, Store, kind, route_with_buffers};
use crate::task::{Command, CommandBuffers};
use crate::{Error, Result, expression::State, fes};

/// PARTICLE 的装载配置及待提交动作；媒体更新复用类型 16 粒子任务。
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleState {
    pub program: Vec<u8>,
    pub image: Vec<u8>,
    pub count: u32,
    pub depth: i32,
    pub mode: i32,
    pub parameters: [f32; 32],
    pub opacity: i32,
    pub(super) dirty: bool,
    rebuild: bool,
    start: bool,
    pub(super) stop: bool,
    playing: bool,
    source: u32,
    source_changed: bool,
}

impl Default for ParticleState {
    fn default() -> Self {
        Self {
            program: Vec::new(),
            image: Vec::new(),
            count: 0,
            depth: 0,
            mode: 0,
            parameters: [0.; 32],
            opacity: 255,
            dirty: false,
            rebuild: false,
            start: false,
            stop: false,
            playing: false,
            source: 0,
            source_changed: false,
        }
    }
}

impl ParticleState {
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn needs_load(&self) -> bool {
        self.rebuild
    }

    /// 最近一次媒体更新缓存的播放状态。
    pub fn playing(&self) -> bool {
        self.playing
    }

    pub(super) fn attach(&mut self, source: u32) {
        self.source = source;
        self.source_changed = true;
    }
}

impl Objects {
    pub(super) fn dispatch_particle_command(
        &mut self,
        interpreter: &mut fes::Interpreter,
        offset: usize,
        args: &[Vec<u8>],
        target: &fes::ObjectReference,
        numbers: &mut State,
    ) -> Result<bool> {
        let command = args[1].as_slice();
        let mut number = |index: usize| {
            args.get(index).map_or(Ok(0), |bytes| {
                interpreter
                    .eval(bytes, numbers)
                    .map_err(|error| Error::new(offset + error.offset, error.message))
            })
        };
        let mut configuration = ParticleState::default();
        if command == b"init" {
            configuration.mode = number(2)?;
            let name = |index: usize| -> Result<Vec<u8>> {
                let bytes = args.get(index).map_or(b"".as_slice(), Vec::as_slice);
                let bytes = &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())];
                if bytes.len() >= 64 {
                    return Err(Error::new(offset, "FES 粒子名称超过存储上限"));
                }
                Ok(bytes.to_vec())
            };
            configuration.program = name(3)?;
            configuration.image = name(4)?;
            configuration.count = number(5)? as u32;
            configuration.depth = number(6)?;
        } else if command == b"param" {
            for (i, value) in configuration.parameters.iter_mut().enumerate() {
                if args.get(i + 2).is_none() {
                    break;
                }
                *value = number(i + 2)? as f32;
            }
        }
        let banks = if target.bank == -1 {
            0..i64::from(self.count(&target.name) as i32)
        } else {
            i64::from(target.bank)..i64::from(target.bank) + 1
        };
        for bank in banks {
            let Some(handle) = self.lookup(&target.name, bank as i32) else {
                continue;
            };
            let ObjectState::Particle(state) = &mut self.get_mut(handle).unwrap().state else {
                continue;
            };
            match command {
                b"init" => {
                    state.program.clone_from(&configuration.program);
                    state.image.clone_from(&configuration.image);
                    state.mode = configuration.mode;
                    state.count = configuration.count;
                    state.depth = configuration.depth;
                    state.rebuild = true;
                }
                b"param" => {
                    state.parameters = configuration.parameters;
                    self.invalidate(handle)?;
                }
                b"start" => state.start = true,
                _ => unreachable!(),
            }
        }
        Ok(true)
    }
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn check_fes_particle_binding(&self, owner: u32, task: Option<u32>) -> Result<()> {
        if task.is_some_and(|task| {
            !self
                .tree
                .node(task)
                .is_some_and(|node| node.parent == Some(owner) && node.kind == kind::PARTICLE_3D)
        }) {
            return Err(Error::new(0, "FES 下层粒子任务绑定失效"));
        }
        Ok(())
    }

    pub(super) fn update_fes_particle(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<()> {
        let object = objects.get(handle).unwrap();
        let task = object.task_id;
        self.check_fes_particle_binding(owner, task)?;
        if task.is_none() {
            return Ok(());
        }
        let ObjectState::Particle(state) = &mut objects.get_mut(handle).unwrap().state else {
            unreachable!()
        };
        if state.rebuild {
            let mut record = [0; 0x290];
            record[..4].copy_from_slice(&state.mode.to_le_bytes());
            let services = self
                .resources
                .services
                .as_deref_mut()
                .ok_or_else(|| Error::new(0, "运行时未配置 FES 粒子资源服务"))?;
            let program = if state.mode != 0 {
                services.resolve_file(b"ptclpi", &state.program)?
            } else {
                services.resolve(b"ptcl", &state.program)?
            };
            let image = services.resolve(b"image", &state.image)?;
            for (at, name) in [(4, program), (0x84, image)] {
                let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
                if name.len() >= 128 {
                    return Err(Error::new(0, "FES 粒子资源名称超过缓冲区上限"));
                }
                record[at..at + name.len()].copy_from_slice(name);
            }
            record[0x104..0x108].copy_from_slice(&state.count.to_le_bytes());
            record[0x108..0x10c].copy_from_slice(&(state.depth as f32).to_le_bytes());
            self.fes_command(owner, task, 320, [0; 3], Some(&mut record))?;
            state.rebuild = false;
        }
        if state.dirty {
            let mut parameters = state.parameters.map(f32::to_le_bytes).concat();
            self.fes_command(owner, task, 325, [0; 3], Some(&mut parameters))?;
            self.fes_command(owner, task, 326, [0, state.opacity as u32, 0], None)?;
            state.dirty = false;
        }
        if state.start {
            self.fes_command(owner, task, 321, [0; 3], None)?;
            state.start = false;
            state.playing = true;
        }
        if state.stop {
            self.fes_command(owner, task, 322, [0; 3], None)?;
            state.stop = false;
            state.playing = false;
        }
        if state.playing {
            let playing = route_with_buffers(
                self.reborrow(),
                owner,
                Command {
                    target: task.unwrap(),
                    sender: owner,
                    code: 324,
                    data: [0; 3],
                },
                &mut CommandBuffers::default(),
            )?;
            if playing == 0 {
                state.playing = false;
            }
        }
        if state.source_changed {
            self.fes_command(owner, task, 327, [0, state.source, 0], None)?;
            state.source_changed = false;
        }
        Ok(())
    }
}
