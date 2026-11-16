use super::Transform;
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};
use crate::{Error, Result};

fn integer(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn positive(value: f32) -> f32 {
    if value > 0. { value } else { 0. }
}

fn normalize(mut value: [f32; 3]) -> [f32; 3] {
    let square = value.map(|v| v * v);
    let length = (((square[0] + square[1]) + square[2]) as f64).sqrt() as f32;
    if length > 0. {
        value = value.map(|v| v / length);
    }
    value
}

fn radians(value: f32) -> f32 {
    (value as f64 * std::f32::consts::PI as f64 / 180.) as f32
}

#[derive(Default)]
struct Config {
    angle: [f32; 2],
    duration: f32,
    radial_mode: i32,
    additive: bool,
    start: [[f32; 2]; 4],
    finish: [[f32; 2]; 4],
    size: [[f32; 2]; 2],
    gravity: [f32; 3],
    life: [f32; 2],
    source: [f32; 3],
    spread: [f32; 2],
    speed: [f32; 2],
    radial: [f32; 2],
    tangential: [f32; 2],
    radius: [f32; 2],
    end_radius: f32,
    spin: [f32; 2],
    rotation: [[f32; 2]; 2],
}

impl Config {
    fn apply(&mut self, bytes: &[u8], height: f32) -> Result<()> {
        let bytes = bytes.split(|&b| b == 0).next().unwrap_or_default();
        let text =
            std::str::from_utf8(bytes).map_err(|_| Error::new(0, "粒子配置需要 UTF-8 XML"))?;
        let document = roxmltree::Document::parse_with_options(
            text,
            roxmltree::ParsingOptions {
                allow_dtd: true,
                nodes_limit: crate::read::MAX_ITEMS as u32,
            },
        )
        .map_err(|e| Error::new(0, format!("粒子配置 XML 无效: {e}")))?;
        let root = document.root_element();
        let dict = root
            .children()
            .find(|node| node.has_tag_name("dict"))
            .filter(|_| root.has_tag_name("plist"))
            .ok_or_else(|| Error::new(0, "粒子配置缺少 plist/dict"))?;
        let mut children = dict.children().filter(roxmltree::Node::is_element);
        while let Some(key) = children.next() {
            let Some(value) = children.next() else { break };
            let key = key.text().unwrap_or_default();
            let field = match key {
                "angle" => &mut self.angle[0],
                "angleVariance" => &mut self.angle[1],
                "duration" => &mut self.duration,
                "emitterType" | "blendFuncDestination" => {
                    let value = parse_number(value.text().unwrap_or_default())?;
                    if key == "emitterType" {
                        self.radial_mode = integer(value);
                    } else {
                        self.additive = integer(value) == 1;
                    }
                    continue;
                }
                "startColorBlue" => &mut self.start[0][0],
                "startColorGreen" => &mut self.start[1][0],
                "startColorRed" => &mut self.start[2][0],
                "startColorAlpha" => &mut self.start[3][0],
                "startColorVarianceBlue" => &mut self.start[0][1],
                "startColorVarianceGreen" => &mut self.start[1][1],
                "startColorVarianceRed" => &mut self.start[2][1],
                "startColorVarianceAlpha" => &mut self.start[3][1],
                "finishColorBlue" => &mut self.finish[0][0],
                "finishColorGreen" => &mut self.finish[1][0],
                "finishColorRed" => &mut self.finish[2][0],
                "finishColorAlpha" => &mut self.finish[3][0],
                "finishColorVarianceBlue" => &mut self.finish[0][1],
                "finishColorVarianceGreen" => &mut self.finish[1][1],
                "finishColorVarianceRed" => &mut self.finish[2][1],
                "finishColorVarianceAlpha" => &mut self.finish[3][1],
                "startParticleSize" => &mut self.size[0][0],
                "startParticleSizeVariance" => &mut self.size[0][1],
                "finishParticleSize" => &mut self.size[1][0],
                "finishParticleSizeVariance" => &mut self.size[1][1],
                "gravityx" => &mut self.gravity[0],
                "gravityy" => &mut self.gravity[1],
                "particleLifespan" => &mut self.life[0],
                "particleLifespanVariance" => &mut self.life[1],
                "sourcePositionx" => &mut self.source[0],
                "sourcePositiony" => &mut self.source[1],
                "sourcePositionVariancex" => &mut self.spread[0],
                "sourcePositionVariancey" => &mut self.spread[1],
                "speed" => &mut self.speed[0],
                "speedVariance" => &mut self.speed[1],
                "radialAcceleration" => &mut self.radial[0],
                "radialAccelVariance" => &mut self.radial[1],
                "tangentialAcceleration" => &mut self.tangential[0],
                "tangentialAccelVariance" => &mut self.tangential[1],
                "maxRadius" => &mut self.radius[0],
                "maxRadiusVariance" => &mut self.radius[1],
                "minRadius" => &mut self.end_radius,
                "rotatePerSecond" => &mut self.spin[0],
                "rotatePerSecondVariance" => &mut self.spin[1],
                "rotationStart" => &mut self.rotation[0][0],
                "rotationStartVariance" => &mut self.rotation[0][1],
                "rotationEnd" => &mut self.rotation[1][0],
                "rotationEndVariance" => &mut self.rotation[1][1],
                _ => continue,
            };
            let value = parse_number(value.text().unwrap_or_default())?;
            *field = match key {
                "angle" => -value,
                "duration" if value > 0. => value * 60.,
                "gravityx" => value / 3600.,
                "gravityy" => -value / 3600.,
                "sourcePositiony" => height - value,
                "rotatePerSecond" => -value / 60.,
                "rotatePerSecondVariance" | "speed" | "speedVariance" => value / 60.,
                "particleLifespan" | "particleLifespanVariance" => value * 60.,
                "radialAcceleration" | "radialAccelVariance" => value / 360000.,
                "tangentialAcceleration" | "tangentialAccelVariance" => value / 3600.,
                _ if key.contains("Color") => value * 255.,
                _ if key.contains("ParticleSize") => value / 30.,
                _ => value,
            };
        }
        Ok(())
    }
}

fn parse_number(value: &str) -> Result<f32> {
    value
        .trim()
        .parse()
        .map_err(|_| Error::new(0, "粒子配置数值无效"))
}

#[derive(Clone)]
struct Sprite {
    pivot: [f32; 2],
    position: [f32; 3],
    rotation: f32,
    size: f32,
    color: u32,
    visible: bool,
}

impl Default for Sprite {
    fn default() -> Self {
        Self {
            pivot: [0.; 2],
            position: [0.; 3],
            rotation: 0.,
            size: 1.,
            color: u32::MAX,
            visible: false,
        }
    }
}

#[derive(Clone, Default)]
struct Particle {
    position: [f32; 3],
    initial: [f32; 3],
    unit: [f32; 3],
    velocity: [f32; 3],
    color: [u8; 4],
    color_step: [u8; 4],
    size: f32,
    size_step: f32,
    rotation: f32,
    rotation_step: f32,
    life: f32,
    initial_life: f32,
    radial: f32,
    tangential: f32,
    angle: f32,
    angle_step: f32,
    radius: f32,
    radius_step: f32,
    done: bool,
}

#[derive(Default)]
pub(super) struct Emitter {
    config: Config,
    valid: bool,
    stopped: bool,
    burst: i32,
    rate: f32,
    emit_clock: f32,
    elapsed: f32,
    active: usize,
    finished: usize,
    center: [f32; 2],
    offset: [f32; 2],
    image: [f32; 2],
    mode: u32,
    particles: Vec<Particle>,
    sprites: Vec<Sprite>,
}

impl Emitter {
    pub(super) fn filename(parameters: &[u32; 32]) -> Vec<u8> {
        let a = integer(f32::from_bits(parameters[0]));
        let b = integer(f32::from_bits(parameters[1]));
        format!("ptcl{a}{b:03}.plist").into_bytes()
    }

    pub(super) fn restart(
        &mut self,
        host: &ParticleHost,
        image: [f32; 2],
        bytes: Option<&[u8]>,
        random: &mut ScriptRandom,
    ) -> Result<()> {
        self.image = image;
        self.burst = integer(f32::from_bits(host.parameters[2]));
        self.particles = vec![Particle::default(); host.quads.len()];
        self.sprites = vec![Sprite::default(); host.quads.len()];
        let Some(bytes) = bytes.filter(|b| !b.is_empty()) else {
            return Ok(());
        };
        self.config.apply(bytes, host.surface_size[1] as f32)?;
        self.rate = host.quads.len() as f32 / self.config.life[0];
        self.emit_clock = 0.;
        self.elapsed = 0.;
        self.active = 0;
        self.finished = 0;
        self.center = host.surface_size.map(|n| n as f32 * 0.5);
        if self.config.additive {
            self.mode = 1;
        }
        for index in 0..self.particles.len() {
            self.spawn(index, random);
        }
        self.valid = true;
        Ok(())
    }

    fn spawn(&mut self, index: usize, random: &mut ScriptRandom) {
        let config = &self.config;
        let mut draw = |[base, spread]: [f32; 2]| ((random.next_float() * 2. - 1.) * spread) + base;
        let p = &mut self.particles[index];
        let s = &mut self.sprites[index];
        s.pivot = self.image.map(|v| v * 0.5);
        s.visible = false;
        p.done = false;
        p.life = positive(draw(config.life));
        p.initial_life = p.life;
        p.position[0] = draw([config.source[0], config.spread[0]]);
        p.position[1] = draw([config.source[1], config.spread[1]]);
        p.initial = p.position;
        p.unit = normalize(std::array::from_fn(|i| p.position[i] - config.source[i]));
        let mut start = [0.; 4];
        let mut finish = [0.; 4];
        for i in [2, 1, 0, 3] {
            start[i] = draw(config.start[i]).clamp(0., 255.);
        }
        for i in [2, 1, 0, 3] {
            finish[i] = draw(config.finish[i]).clamp(0., 255.);
        }
        p.color = start.map(|v| integer(v) as u8);
        // 颜色及增量均为无符号字节，负增量按低八位回绕。
        p.color_step = std::array::from_fn(|i| integer((finish[i] - start[i]) / p.life) as u8);
        p.size = positive(draw(config.size[0]));
        p.size_step = (positive(draw(config.size[1])) - p.size) / p.life;
        p.rotation = radians(draw(config.rotation[0]));
        p.rotation_step = (radians(draw(config.rotation[1])) - p.rotation) / p.life;
        let angle = radians(draw(config.angle));
        if config.radial_mode == 1 {
            p.radius = draw(config.radius);
            p.radius_step = (draw([config.end_radius, 0.]) - p.radius) / p.life;
            p.angle = angle;
            p.angle_step = radians(draw(config.spin));
        } else {
            let speed = draw(config.speed);
            p.velocity = [
                (angle as f64).cos() as f32 * speed,
                (angle as f64).sin() as f32 * speed,
                0. * speed,
            ];
            p.radial = draw(config.radial);
            p.tangential = draw(config.tangential);
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2], random: &mut ScriptRandom) {
        if !self.valid {
            return;
        }
        let dt = delta as f32;
        self.offset = std::array::from_fn(|i| source[i] - self.center[i]);
        if !self.stopped {
            let period = 1. / self.rate;
            if self.active < self.particles.len() {
                self.emit_clock += dt;
                while self.emit_clock > period && self.active < self.particles.len() {
                    self.active = if self.burst != 0 {
                        self.particles.len()
                    } else {
                        self.active + 1
                    };
                    self.emit_clock -= period;
                }
            }
            self.elapsed += dt;
            if self.config.duration != -1. && self.elapsed > self.config.duration {
                self.stopped = true;
            }
        }
        let mut index = 0;
        while index < self.active {
            let slot = index;
            let p = &mut self.particles[index];
            p.life -= dt;
            if p.life > 0. {
                let c = &self.config;
                if c.radial_mode == 1 {
                    p.angle += p.angle_step * dt;
                    p.radius += p.radius_step * dt;
                    p.position[0] = -((p.angle as f64).cos() as f32 * p.radius) + c.source[0];
                    p.position[1] = -((p.angle as f64).sin() as f32 * p.radius) + c.source[1];
                    p.position[2] += 0.;
                } else {
                    let away: [f32; 3] = std::array::from_fn(|i| p.position[i] - c.source[i]);
                    let basis = [-p.unit[1], p.unit[0], p.unit[2]];
                    let from = if p.tangential > 0. { p.velocity } else { basis };
                    let to = if p.tangential > 0. { basis } else { p.velocity };
                    let d: [f32; 3] = std::array::from_fn(|i| from[i] - to[i]);
                    let perpendicular = [-d[1], d[0], d[2]];
                    let tangent = normalize(std::array::from_fn(|i| {
                        if p.tangential > 0. {
                            p.velocity[i] - perpendicular[i]
                        } else {
                            perpendicular[i] - p.velocity[i]
                        }
                    }));
                    for i in 0..3 {
                        let force = (away[i] * p.radial + tangent[i] * p.tangential) + c.gravity[i];
                        p.velocity[i] += force * dt;
                        p.position[i] += (p.velocity[i] * dt) * 1.;
                    }
                }
                p.color = std::array::from_fn(|i| {
                    integer(p.color_step[i] as f32 * dt + p.color[i] as f32) as u8
                });
                p.size = positive(p.size + p.size_step * dt);
                p.rotation += p.rotation_step * dt;
                let s = &mut self.sprites[index];
                s.visible = true;
                s.position = p.position;
                s.size = p.size;
                s.rotation = p.rotation;
                s.color = u32::from_le_bytes(p.color);
                index += 1;
            } else if self.stopped {
                if !p.done {
                    p.done = true;
                    self.sprites[index].visible = false;
                    self.finished += 1;
                }
                if self.finished == self.active {
                    self.valid = false;
                    self.elapsed = self.config.duration;
                    self.emit_clock = 0.;
                    return;
                }
                index += 1;
            } else {
                self.spawn(index, random);
                let last = self.active - 1;
                if index != last {
                    self.sprites[index] = self.sprites[last].clone();
                    self.particles[index] = self.particles[last].clone();
                    self.spawn(last, random);
                }
                self.active -= 1;
            }
            // 被移入当前槽的网格仍执行本轮偏移，下一次循环再更新其寿命。
            for axis in 0..2 {
                self.sprites[slot].position[axis] += self.offset[axis];
            }
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        host.render_mode = self.mode;
        for (i, sprite) in self.sprites.iter().enumerate() {
            let mut transform = Transform::default();
            transform.set_scale([sprite.size, sprite.size, 1.]);
            transform.set_rotation([0., 0., sprite.rotation]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&sprite.position);
            for (corner, vertex) in host.quads[i].iter_mut().enumerate() {
                let point = transform_vector(
                    [
                        (corner & 1) as f32 * self.image[0] - sprite.pivot[0],
                        (corner >> 1) as f32 * self.image[1] - sprite.pivot[1],
                        0.,
                        1.,
                    ],
                    matrix,
                );
                vertex.position.copy_from_slice(&point[..3]);
                vertex.rhw = point[3];
                vertex.color = sprite.color;
            }
            host.quad_flags[i] = u32::from(sprite.visible);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &[u8] = b"<plist><dict>\
        <key>duration</key><real>-1</real>\
        <key>particleLifespan</key><real>0.1</real>\
        <key>startColorAlpha</key><real>1</real>\
        <key>finishColorAlpha</key><real>0</real>\
        <key>startParticleSize</key><real>30</real>\
        <key>finishParticleSize</key><real>30</real>\
        </dict></plist>";

    #[test]
    fn strict_emission_boundary_and_byte_color_steps_keep_rebirth_random_order() -> Result<()> {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(2, [30., 30.])?;
        host.parameters[2] = 1f32.to_bits();
        let mut random = ScriptRandom::seeded(17);
        let mut emitter = Emitter::default();
        emitter.restart(&host, [30., 30.], Some(CONFIG), &mut random)?;
        emitter.update(3, [640., 360.], &mut random);
        assert_eq!(emitter.active, 0);
        emitter.update(1, [640., 360.], &mut random);
        emitter.write_mesh(&mut host);
        assert_eq!(host.quad_flags, [1, 1]);
        assert_eq!(host.quads[0][0].color, 213 << 24);
        assert_eq!(emitter.particles[0].life, 5.);
        emitter.update(5, [640., 360.], &mut random);
        emitter.write_mesh(&mut host);
        assert_eq!(emitter.active, 0);
        assert_eq!(host.quad_flags, [0, 0]);
        let mut expected = ScriptRandom::seeded(17);
        for _ in 0..95 {
            expected.next_float();
        }
        assert_eq!(random.next_float(), expected.next_float());
        assert_eq!(random.next_integer(), expected.next_integer());
        Ok(())
    }

    #[test]
    fn restarting_retains_config_and_stop_state_and_missing_files_clear_particles() -> Result<()> {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(2, [30., 30.])?;
        host.parameters[2] = 1f32.to_bits();
        let mut random = ScriptRandom::seeded(17);
        let mut emitter = Emitter::default();
        let config = String::from_utf8(CONFIG.to_vec())
            .unwrap()
            .replace("<real>-1</real>", "<real>0</real>");
        emitter.restart(&host, [30., 30.], Some(config.as_bytes()), &mut random)?;
        emitter.update(4, [640., 360.], &mut random);
        emitter.update(2, [640., 360.], &mut random);
        assert!(emitter.stopped);
        assert!(!emitter.valid);
        let next = random.clone().next_float();
        emitter.restart(&host, [30., 30.], None, &mut random)?;
        assert_eq!(random.clone().next_float(), next);
        assert_eq!(emitter.particles[0].life, 0.);
        assert_eq!(emitter.config.life, [6., 0.]);
        emitter.restart(
            &host,
            [30., 30.],
            Some(b"<plist><dict><key>duration</key><real>-1</real></dict></plist>"),
            &mut random,
        )?;
        assert!(emitter.valid && emitter.stopped);
        emitter.update(100, [640., 360.], &mut random);
        assert_eq!(emitter.active, 0);
        assert_eq!(emitter.config.life, [6., 0.]);
        assert_eq!(emitter.config.source[1], 0.);
        Ok(())
    }
}
