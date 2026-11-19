use super::particle_round::{Round, Variant as RoundVariant};
use super::{Drawable, ParticleMesh, Surface};
use super::{
    particle_anger::Anger, particle_angry::Angry, particle_asease::Asease, particle_common::Common,
    particle_concentration::Concentration, particle_dark::Dark, particle_dash::Dash,
    particle_digi::Digi, particle_digi_ring::DigiRing, particle_dis::Dis, particle_double::Double,
    particle_down::Down, particle_emitter::Emitter, particle_energy::Energy,
    particle_exclamation::Exclamation, particle_explosion::Explosion,
    particle_film_noise::FilmNoise, particle_flower_burst::FlowerBurst, particle_glint::Glint,
    particle_hadou::Hadou, particle_ice::Ice, particle_mirror::Mirror, particle_note::Note,
    particle_notice::Notice, particle_paper::Paper, particle_plugin, particle_protect::Protect,
    particle_question::Question, particle_radiate::Radiate, particle_rain::Rain,
    particle_ring::Ring, particle_rising::Rising, particle_rubble::Rubble, particle_sakura::Sakura,
    particle_scene_sparkle::SceneSparkle, particle_shy::Shy, particle_sigh::Sigh,
    particle_silent::Silent, particle_spark::Spark, particle_sparkle::Sparkle,
    particle_steam::Steam, particle_surprise::Surprise, particle_sweat::Sweat,
    particle_sword_spark::SwordSpark, particle_thunder::Thunder, particle_warukira::Warukira,
};
use crate::kcs::{
    Host, Interpreter, ParticleHost, ParticleSource, ParticleTask, Program, ScriptRandom,
};
use crate::resource::ResourceMode;
use crate::runtime::{BaseObject, TaskObject};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};
use std::sync::{Arc, atomic::AtomicU32};

enum ParticleProgram {
    Script(Program),
    Builtin(particle_plugin::Kind),
}

enum Effect {
    Rain(Rain),
    Notice(Notice),
    Anger(Anger),
    Sigh(Sigh),
    Down(Down),
    Sweat(Sweat),
    Dis(Dis),
    Common(Common),
    Note(Note),
    Angry(Angry),
    FlowerBurst(FlowerBurst),
    Exclamation(Exclamation),
    FilmNoise(FilmNoise),
    Asease(Asease),
    Surprise(Surprise),
    Silent(Silent),
    Question(Question),
    Sparkle(Sparkle),
    Concentration(Concentration),
    Glint(Glint),
    Energy(Energy),
    Digi(Digi),
    DigiRing(DigiRing),
    Rising(Rising),
    Double(Double),
    Shy(Box<Shy>),
    Warukira(Warukira),
    Hadou(Hadou),
    Protect(Box<Protect>),
    Dark(Dark),
    Sakura(Sakura),
    Ice(Ice),
    SceneSparkle(SceneSparkle),
    Spark(Box<Spark>),
    Thunder(Thunder),
    Paper(Paper),
    Dash(Dash),
    Explosion(Explosion),
    Round(Round),
    Steam(Box<Steam>),
    Mirror(Mirror),
    Ring(Ring),
    Radiate(Radiate),
    SwordSpark(SwordSpark),
    Emitter(Box<Emitter>),
    Rubble(Rubble),
}

impl Effect {
    fn new(
        kind: particle_plugin::Kind,
        host: &ParticleHost,
        image: [f32; 2],
        random: &mut ScriptRandom,
        module_state: Arc<AtomicU32>,
    ) -> Result<Self> {
        Ok(match kind {
            particle_plugin::Kind::Emitter => {
                return Err(Error::new(0, "通用粒子需要配置资源初始化"));
            }
            particle_plugin::Kind::SwordSpark => {
                Self::SwordSpark(SwordSpark::new(host, image, random)?)
            }
            particle_plugin::Kind::Rain => Self::Rain(Rain::new(host, image, random)),
            particle_plugin::Kind::Notice => Self::Notice(Notice::new(host, image)),
            particle_plugin::Kind::Anger => Self::Anger(Anger::new(host, image, random)),
            particle_plugin::Kind::Sigh => Self::Sigh(Sigh::new(host, image)),
            particle_plugin::Kind::Down => Self::Down(Down::new(host, image, random)),
            particle_plugin::Kind::Sweat => Self::Sweat(Sweat::new(host, image)),
            particle_plugin::Kind::Dis => Self::Dis(Dis::new(host, image)),
            particle_plugin::Kind::Common => Self::Common(Common::new(host, image)),
            particle_plugin::Kind::Note => Self::Note(Note::new(host, image, random)),
            particle_plugin::Kind::Angry => Self::Angry(Angry::new(host, image)),
            particle_plugin::Kind::FlowerBurst => {
                Self::FlowerBurst(FlowerBurst::new(host, image, random))
            }
            particle_plugin::Kind::Exclamation => Self::Exclamation(Exclamation::new(host, image)),
            particle_plugin::Kind::FilmNoise => {
                Self::FilmNoise(FilmNoise::new(host, image, random))
            }
            particle_plugin::Kind::Asease => Self::Asease(Asease::new(host, image, random)),
            particle_plugin::Kind::Surprise => Self::Surprise(Surprise::new(host, image)),
            particle_plugin::Kind::Silent => Self::Silent(Silent::new(host, image)),
            particle_plugin::Kind::Question => Self::Question(Question::new(host, image)),
            particle_plugin::Kind::Sparkle => Self::Sparkle(Sparkle::new(host, image, random)),
            particle_plugin::Kind::Concentration => {
                Self::Concentration(Concentration::new(host, image, random, module_state)?)
            }
            particle_plugin::Kind::Glint => Self::Glint(Glint::new(host, image)),
            particle_plugin::Kind::Energy => Self::Energy(Energy::new(host, image, random)),
            particle_plugin::Kind::Digi => Self::Digi(Digi::new(host, image, random)),
            particle_plugin::Kind::DigiRing => Self::DigiRing(DigiRing::new(host, image, random)),
            particle_plugin::Kind::Rising => Self::Rising(Rising::new(host, image, random)),
            particle_plugin::Kind::Rising3 => {
                Self::Rising(Rising::new_rising3(host, image, random))
            }
            particle_plugin::Kind::Double => Self::Double(Double::new(host, image)),
            particle_plugin::Kind::Shy => Self::Shy(Box::new(Shy::new(host, image, random))),
            particle_plugin::Kind::Warukira => Self::Warukira(Warukira::new(host, image, random)),
            particle_plugin::Kind::Hadou => Self::Hadou(Hadou::new(host, image)?),
            particle_plugin::Kind::Ice => Self::Ice(Ice::new(host, image, random)),
            particle_plugin::Kind::Star => Self::Ice(Ice::new_star(host, image, random)),
            particle_plugin::Kind::Spark => Self::Spark(Box::new(Spark::new(host, image, random))),
            particle_plugin::Kind::Thunder => {
                Self::Thunder(Thunder::new(host, image, random, &module_state))
            }
            particle_plugin::Kind::Paper => {
                Self::Paper(Paper::new(host, image, random, module_state))
            }
            particle_plugin::Kind::Dash => Self::Dash(Dash::new(host, image, random, module_state)),
            particle_plugin::Kind::Explosion => {
                Self::Explosion(Explosion::new(host, image, random, module_state))
            }
            particle_plugin::Kind::Roundy => Self::Round(Round::new(host, random, RoundVariant::Y)),
            particle_plugin::Kind::Roundz => Self::Round(Round::new(host, random, RoundVariant::Z)),
            particle_plugin::Kind::Steam => Self::Steam(Box::new(Steam::new(host, image, random))),
            particle_plugin::Kind::Mirror => Self::Mirror(Mirror::new(host, image, random)),
            particle_plugin::Kind::Ring => Self::Ring(Ring::new(host, image)?),
            particle_plugin::Kind::Radiate => Self::Radiate(Radiate::new(host, image, random)),
            particle_plugin::Kind::Rubble => Self::Rubble(Rubble::new(host, image, random)),
            particle_plugin::Kind::SceneSparkle => {
                Self::SceneSparkle(SceneSparkle::new(host, image, random))
            }
            particle_plugin::Kind::Sakura => Self::Sakura(Sakura::new(host, image, random)),
            particle_plugin::Kind::Dark => Self::Dark(Dark::new(host, image, random)),
            particle_plugin::Kind::DarkTriangle => {
                Self::Dark(Dark::new_triangle(host, image, random))
            }
            particle_plugin::Kind::Protect => Self::Protect(Box::new(Protect::new(host))),
        })
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
        module_state: Arc<AtomicU32>,
    },
}

fn particle_texture(mut frame: crate::hg3::DecodedFrame) -> Result<Arc<Surface>> {
    let [width, height] = frame.info.size.map(|n| n as usize);
    let stride = width * 4;
    // 粒子装载器直接上传底端起始的位图内存，UV 保留该行方向。
    for row in 0..height / 2 {
        let (before, after) = frame.pixels.split_at_mut((height - 1 - row) * stride);
        before[row * stride..(row + 1) * stride].swap_with_slice(&mut after[..stride]);
    }
    Surface::from_bgra(frame.info.size, frame.pixels).map(Arc::new)
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
        particles.active = false;
        match self {
            Self::Script(task) => task.origin = 0,
            Self::Builtin {
                kind,
                host,
                effect,
                origin,
                module_state,
                ..
            } => {
                *origin = 0;
                if matches!(kind, particle_plugin::Kind::Emitter) {
                    let name = Emitter::filename(&host.parameters);
                    let resources = services
                        .resource_services()
                        .ok_or_else(|| Error::new(0, "宿主未提供粒子配置读取服务"))?;
                    let name = resources.resolve(b"ptcldat", &name)?;
                    let bytes = resources.read(&name, ResourceMode::LooseFirst)?;
                    let mut emitter = match effect.take() {
                        Some(Effect::Emitter(emitter)) => emitter,
                        _ => Box::default(),
                    };
                    let mut random = std::mem::take(&mut host.random);
                    let result = emitter.restart(
                        host,
                        image,
                        bytes.as_deref(),
                        services.random().unwrap_or(&mut random),
                    );
                    host.random = random;
                    result?;
                    emitter.write_mesh(host);
                    *effect = Some(Effect::Emitter(emitter));
                    host.active = true;
                    return Ok(());
                }
                *effect = None;
                let next = if let Some(random) = services.random() {
                    Effect::new(*kind, host, image, random, module_state.clone())
                } else {
                    let mut random = std::mem::take(&mut host.random);
                    let effect = Effect::new(*kind, host, image, &mut random, module_state.clone());
                    host.random = random;
                    effect
                };
                *effect = Some(next?);
                match effect {
                    Some(Effect::SwordSpark(effect)) => effect.write_mesh(host),
                    Some(Effect::Protect(effect)) => effect.write_mesh(host),
                    Some(Effect::Dark(effect)) => effect.write_mesh(host),
                    Some(Effect::Sakura(effect)) => effect.write_mesh(host),
                    Some(Effect::Ice(effect)) => effect.write_mesh(host),
                    Some(Effect::SceneSparkle(effect)) => effect.write_mesh(host),
                    Some(Effect::Spark(effect)) => effect.write_mesh(host),
                    Some(Effect::Thunder(effect)) => effect.write_mesh(host),
                    Some(Effect::Paper(effect)) => effect.write_mesh(host),
                    Some(Effect::Dash(effect)) => effect.write_mesh(host),
                    Some(Effect::Explosion(effect)) => effect.write_mesh(host),
                    Some(Effect::Round(effect)) => effect.write_mesh(host),
                    Some(Effect::Steam(effect)) => effect.write_mesh(host),
                    Some(Effect::Mirror(effect)) => effect.write_mesh(host),
                    Some(Effect::Ring(effect)) => effect.write_mesh(host),
                    Some(Effect::Radiate(effect)) => effect.write_mesh(host),
                    Some(Effect::Rubble(effect)) => effect.write_mesh(host),
                    _ => {}
                }
            }
        }
        self.host_mut().active = true;
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
                        Effect::Emitter(effect) => {
                            effect.update(
                                delta,
                                host.source.unwrap_or_default().position_2d,
                                services.random().unwrap_or(&mut host.random),
                            );
                            effect.write_mesh(host);
                        }
                        Effect::Radiate(effect) => {
                            effect.update(delta, services.random().unwrap_or(&mut host.random));
                            effect.write_mesh(host);
                        }
                        Effect::Rubble(effect) => {
                            effect.update(delta);
                            effect.write_mesh(host);
                        }
                        Effect::Ring(effect) => {
                            effect.update(delta, host.source.unwrap_or_default().position_2d);
                            effect.write_mesh(host);
                        }
                        Effect::Mirror(effect) => {
                            effect.update(
                                delta,
                                host.source.unwrap_or_default().position_2d,
                                services.random().unwrap_or(&mut host.random),
                            );
                            effect.write_mesh(host);
                        }
                        Effect::Steam(effect) => {
                            effect.update(delta, services.random().unwrap_or(&mut host.random));
                            effect.write_mesh(host);
                        }
                        Effect::Round(effect) => {
                            effect.update(delta, services.random().unwrap_or(&mut host.random));
                            effect.write_mesh(host);
                        }
                        Effect::Explosion(effect) => {
                            effect.update(delta, services.random().unwrap_or(&mut host.random));
                            effect.write_mesh(host);
                        }
                        Effect::Dash(effect) => {
                            effect.update(delta, services.random().unwrap_or(&mut host.random));
                            effect.write_mesh(host);
                        }
                        Effect::Paper(effect) => {
                            effect.update(delta, services.random().unwrap_or(&mut host.random));
                            effect.write_mesh(host);
                        }
                        Effect::Thunder(effect) => {
                            effect.update(delta, host.source.unwrap_or_default().position_2d);
                            effect.write_mesh(host);
                        }
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
                        Effect::Sigh(effect) => {
                            effect.update(delta, host.source.unwrap_or_default().position_2d);
                            effect.write_mesh(host);
                        }
                        Effect::Down(effect) => {
                            effect.update(delta, host.source.unwrap_or_default().position_2d);
                            effect.write_mesh(host);
                        }
                        Effect::Sweat(effect) => {
                            effect.update(delta, host.source.unwrap_or_default().position_2d);
                            effect.write_mesh(host);
                        }
                        Effect::Dis(effect) => {
                            effect.update(
                                delta,
                                host.source.unwrap_or_default().position_2d,
                                services.random().unwrap_or(&mut host.random),
                            );
                            effect.write_mesh(host);
                        }
                        Effect::Common(effect) => {
                            effect.update(delta, host.source.unwrap_or_default().position_2d);
                            effect.write_mesh(host);
                        }
                        Effect::Angry(effect) => {
                            effect.update(delta, host.source.unwrap_or_default().position_2d);
                            effect.write_mesh(host);
                        }
                        Effect::Double(effect) => {
                            effect.update(delta, host.source.unwrap_or_default().position_2d);
                            effect.write_mesh(host);
                        }
                        Effect::Shy(effect) => {
                            effect.update(
                                delta,
                                host.source.unwrap_or_default().position_2d,
                                services.random().unwrap_or(&mut host.random),
                            );
                            effect.write_mesh(host);
                        }
                        Effect::Exclamation(effect) => {
                            effect.update(delta, host.source.unwrap_or_default().position_2d);
                            effect.write_mesh(host);
                        }
                        Effect::Note(effect) => {
                            effect.update(
                                delta,
                                host.source.unwrap_or_default().position_2d,
                                services.random().unwrap_or(&mut host.random),
                            );
                            effect.write_mesh(host);
                        }
                        Effect::FlowerBurst(effect) => {
                            effect.update(
                                delta,
                                host.source.unwrap_or_default().position_2d,
                                services.random().unwrap_or(&mut host.random),
                            );
                            effect.write_mesh(host);
                        }
                        Effect::FilmNoise(effect) => {
                            effect.update(delta, services.random().unwrap_or(&mut host.random));
                            effect.write_mesh(host);
                        }
                        Effect::Asease(effect) => {
                            effect.update(
                                delta,
                                host.source.unwrap_or_default().position_2d,
                                services.random().unwrap_or(&mut host.random),
                            );
                            effect.write_mesh(host);
                        }
                        Effect::Surprise(effect) => {
                            effect.update(delta, host.source.unwrap_or_default().position_2d);
                            effect.write_mesh(host);
                        }
                        Effect::Sparkle(effect) => {
                            effect.update(
                                delta,
                                host.source.unwrap_or_default().position_2d,
                                services.random().unwrap_or(&mut host.random),
                            );
                            effect.write_mesh(host);
                        }
                        Effect::Warukira(effect) => {
                            effect.update(
                                delta,
                                host.source.unwrap_or_default().position_2d,
                                services.random().unwrap_or(&mut host.random),
                            );
                            effect.write_mesh(host);
                        }
                        Effect::Dark(effect) => {
                            effect.refresh_parameters(&host.parameters);
                            effect.update(
                                delta,
                                host.source.unwrap_or_default().position_2d,
                                services.random().unwrap_or(&mut host.random),
                            );
                            effect.write_mesh(host);
                        }
                        Effect::Spark(effect) => {
                            effect.update(delta, services.random().unwrap_or(&mut host.random));
                            effect.write_mesh(host);
                        }
                        Effect::SwordSpark(effect) => {
                            effect.update(delta, &host.parameters);
                            effect.write_mesh(host);
                        }
                        Effect::SceneSparkle(effect) => {
                            effect.update(delta);
                            effect.write_mesh(host);
                        }
                        Effect::Ice(effect) => {
                            effect.update(delta, services.random().unwrap_or(&mut host.random));
                            effect.write_mesh(host);
                        }
                        Effect::Sakura(effect) => {
                            effect.update(delta, services.random().unwrap_or(&mut host.random));
                            effect.write_mesh(host);
                        }
                        Effect::Protect(effect) => {
                            effect.update(delta);
                            effect.write_mesh(host);
                        }
                        Effect::Hadou(effect) => {
                            effect.update(delta, host.source.unwrap_or_default().position_2d);
                            effect.write_mesh(host);
                        }
                        Effect::Concentration(effect) => {
                            effect.update(delta, services.random().unwrap_or(&mut host.random));
                            effect.write_mesh(host);
                        }
                        Effect::Glint(effect) => {
                            effect.update(
                                delta,
                                host.source.unwrap_or_default().position_2d,
                                services.random().unwrap_or(&mut host.random),
                            );
                            effect.write_mesh(host);
                        }
                        Effect::Energy(effect) => {
                            effect.update(delta, services.random().unwrap_or(&mut host.random));
                            effect.write_mesh(host);
                        }
                        Effect::Rising(effect) => {
                            effect.update(
                                delta,
                                &host.parameters,
                                services.random().unwrap_or(&mut host.random),
                            );
                            effect.write_mesh(host);
                        }
                        Effect::Digi(effect) => {
                            effect.update(
                                delta,
                                host.source.unwrap_or_default().position_2d,
                                services.random().unwrap_or(&mut host.random),
                            );
                            effect.write_mesh(host);
                        }
                        Effect::DigiRing(effect) => {
                            effect.update(
                                delta,
                                host.source.unwrap_or_default().position_2d,
                                services.random().unwrap_or(&mut host.random),
                            );
                            effect.write_mesh(host);
                        }
                        Effect::Question(effect) => {
                            effect.update(delta, host.source.unwrap_or_default().position_2d);
                            effect.write_mesh(host);
                        }
                        Effect::Silent(effect) => {
                            effect.update(delta, host.source.unwrap_or_default().position_2d);
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
    camera_depth: f32,
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
            camera_depth: 0.,
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
        let without_draw = read::u32_at(record, 0x18c)? != 0;
        if mode == 1 && without_draw {
            return Err(Error::new(0x18c, "粒子插件的无绘制模式尚未接入"));
        }
        self.clear(host)?;
        let count = read::u32_at(record, 0x104)? as usize;
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
            let program_name = name(4)?;
            let Some(bytes) = services.read(program_name, read_mode)? else {
                return Ok(());
            };
            let program = if mode == 1 {
                ParticleProgram::Builtin(particle_plugin::identify(&bytes).ok_or_else(|| {
                    let filename = program_name
                        .rsplit(|&b| matches!(b, b'/' | b'\\'))
                        .next()
                        .unwrap_or_default();
                    Error::new(
                        0,
                        format!(
                            "粒子插件类型或版本尚未接入：{}",
                            String::from_utf8_lossy(filename)
                        ),
                    )
                })?)
            } else {
                ParticleProgram::Script(Program::parse(&bytes)?)
            };
            if matches!(
                &program,
                ParticleProgram::Builtin(particle_plugin::Kind::Silent)
            ) && !(1..=4).contains(&count)
            {
                return Err(Error::new(0, "静默标记需要一个主体及至多三个附加点"));
            }
            let image = if without_draw || name(0x84)?.is_empty() {
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
        let depth = f32::from_bits(read::u32_at(record, 0x108)?);
        if !without_draw {
            self.image_size = image
                .as_ref()
                .map_or([256.; 2], |frame| frame.info.size.map(|n| n as f32));
            self.camera_depth = (65000. - depth) - 1000.;
        }
        let mut particles = ParticleHost {
            active: false,
            surface_size: host.surface_size(),
            depth_offset: depth,
            camera_depth: self.camera_depth,
            object_id: self.source,
            parameters: self.parameters,
            ..ParticleHost::default()
        };
        particles.reset_quads(count, self.image_size)?;
        let mesh = if without_draw {
            None
        } else {
            Some(ParticleMesh::new(
                &particles,
                image.map(particle_texture).transpose()?,
                self.camera_depth,
            )?)
        };
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
                module_state: host
                    .drawables()
                    .unwrap()
                    .particle_module_states
                    .load(name(4)?)?,
            },
        });
        if let Some(mesh) = mesh {
            let drawings = host.drawables().unwrap();
            drawings.update(self.id, |drawable| {
                drawable.particle_mesh = Some(mesh);
            });
            drawings.set_base_point(self.id, [0., 0., depth]);
        }
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
            drawable.display = u32::from(particles.active);
            drawable.blend_mode = particles.render_mode;
        }
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
                let result = self
                    .task
                    .as_mut()
                    .map_or(Ok(()), |task| task.start(self.image_size, host));
                self.sync(host)?;
                result?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particle_texture_keeps_bitmap_row_order_and_transparent_rgb() {
        let frame = crate::hg3::DecodedFrame {
            id: 0,
            info: crate::hg3::StandardInfo {
                size: [2, 3],
                bit_depth: 32,
                offset: [0; 2],
                canvas_size: [2, 3],
                reserved: 1,
                base: [0; 2],
            },
            attributes: Default::default(),
            pixels: [
                0x0012_3456u32,
                0xff34_5678,
                0x8078_9abc,
                0x409a_bcde,
                0x20bc_def0,
                0x10de_f012,
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect(),
        };
        let texture = particle_texture(frame).unwrap();
        assert_eq!(texture.size(), [2, 3]);
        assert_eq!(texture.pixel(0, 0), Some(0x20bc_def0));
        assert_eq!(texture.pixel(1, 0), Some(0x10de_f012));
        assert_eq!(texture.pixel(0, 1), Some(0x8078_9abc));
        assert_eq!(texture.pixel(1, 1), Some(0x409a_bcde));
        assert_eq!(texture.pixel(0, 2), Some(0x0012_3456));
        assert_eq!(texture.pixel(1, 2), Some(0xff34_5678));
    }
}
