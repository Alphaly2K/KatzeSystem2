use crate::kcs::{ParticleHost, ScriptRandom};

#[derive(Clone, Copy)]
enum Phase {
    Rising,
    Fading,
    Respawn,
}

struct Light {
    position: [f32; 3],
    green: u8,
    alpha: u8,
    age: i32,
    phase_start: i32,
    phase: Phase,
}

pub(super) struct SceneLight {
    lights: Vec<Light>,
    active: usize,
    center: [f32; 2],
    image: [f32; 2],
    scale: f32,
}

fn interpolate(from: f32, to: f32, duration: f32, time: f32) -> u8 {
    let value = if time <= 0. {
        from
    } else if time >= duration {
        to
    } else {
        (time / duration) * (to - from) + from
    };
    value as u8
}

impl Light {
    fn spawn(center: [f32; 2], phase_start: i32, random: &mut ScriptRandom) -> Self {
        let next = |random: &mut ScriptRandom| f64::from(random.next_float());
        let [x, y] = std::array::from_fn(|_| (next(random) * 100. - 50.) as f32);
        let length = (f64::from(x) * f64::from(x) + f64::from(y) * f64::from(y)) as f32;
        let length = length.sqrt();
        let direction = if length > 0. {
            [x / length, y / length]
        } else {
            [x, y]
        };
        let radius = (next(random) * 100.) as f32;
        let [x, y] = std::array::from_fn(|i| direction[i] * radius + center[i]);
        // Color uses the distance after the coordinates have been translated and stored.
        let [dx, dy] = [x - center[0], y - center[1]];
        let distance = (dx * dx + dy * dy).sqrt();
        Self {
            position: [x, y, (next(random) * 50.) as f32],
            green: interpolate(150., 40., 80., distance),
            alpha: 0,
            age: 0,
            phase_start,
            phase: Phase::Rising,
        }
    }

    fn update(&mut self, delta: i32, center: [f32; 2], random: &mut ScriptRandom) {
        match self.phase {
            Phase::Rising => {
                self.alpha = interpolate(0., 180., 20., self.age as f32);
                self.position[1] -= 0.5;
                if self.age > 120 {
                    self.phase = Phase::Fading;
                    self.phase_start = self.age;
                }
            }
            Phase::Fading => {
                let elapsed = self.age.wrapping_sub(self.phase_start);
                self.alpha = interpolate(180., 0., 30., elapsed as f32);
                self.position[1] -= 0.2;
                if elapsed > 30 {
                    self.phase = Phase::Respawn;
                    self.phase_start = self.age;
                }
            }
            Phase::Respawn => *self = Self::spawn(center, self.phase_start, random),
        }
        self.age = self.age.wrapping_add(delta);
    }
}

impl SceneLight {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let [x, y, scale, ..] = host.parameters.map(f32::from_bits);
        let center = [x, y];
        Self {
            lights: (0..host.quads.len())
                .map(|_| Light::spawn(center, 0, random))
                .collect(),
            active: 1,
            center,
            image,
            scale: (f64::from(scale) / 100.) as f32,
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        self.active = (self.active + 2).min(self.lights.len());
        let delta = delta as f32;
        let delta = if delta < 2147483648. {
            delta as i32
        } else {
            i32::MIN
        };
        for light in &mut self.lights[..self.active] {
            light.update(delta, self.center, random);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        host.render_mode = 1;
        for (light, quad) in self.lights.iter().zip(&mut host.quads) {
            let color =
                (u32::from(light.alpha) << 24) | 0x0096_0014 | (u32::from(light.green) << 8);
            for (i, vertex) in quad.iter_mut().enumerate() {
                let [x, y] = [(i & 1) as f64, (i >> 1) as f64];
                vertex.position = [
                    ((x - 0.5) * f64::from(self.image[0]) * f64::from(self.scale)
                        + f64::from(light.position[0])) as f32,
                    ((y - 0.5) * f64::from(self.image[1]) * f64::from(self.scale)
                        + f64::from(light.position[1])) as f32,
                    light.position[2],
                ];
                vertex.color = color;
            }
        }
    }
}
