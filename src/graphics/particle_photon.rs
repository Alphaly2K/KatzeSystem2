use super::particle_curve::TwoPointPath;
use crate::kcs::{ParticleHost, ScriptRandom};

#[derive(Clone, Copy)]
struct Settings {
    center: [f32; 3],
    spread: [f32; 3],
    scale_range: f32,
    fade_in: f32,
    travel: f32,
    fade_out: f32,
    emit_interval: i32,
    emit_count: i32,
    repeat: bool,
}

#[derive(Clone, Copy)]
enum Phase {
    FadeIn,
    Travel,
    FadeOut,
    Inactive,
}

struct Particle {
    path: TwoPointPath,
    position: [f32; 3],
    scale: f32,
    alpha: u8,
    age: i32,
    phase_start: i32,
    phase: Phase,
}

impl Particle {
    fn spawn(settings: Settings, random: &mut ScriptRandom) -> Self {
        let scale =
            (random.next_float() as f64 * settings.scale_range as f64 + 0.1f32 as f64) as f32;
        let direction = settings
            .spread
            .map(|spread| ((random.next_float() as f64 - 0.5) * spread as f64) as f32);
        let [x, y, z] = direction.map(f64::from);
        let length = ((x * x + y * y + z * z) as f32).sqrt();
        let direction = if length > 0. {
            direction.map(|axis| axis / length)
        } else {
            direction
        };
        let position = std::array::from_fn(|i| {
            (direction[i] as f64 * settings.spread[i] as f64 + settings.center[i] as f64) as f32
        });
        let path = TwoPointPath::new_extended(
            [position, settings.center],
            settings.travel + settings.fade_out,
            100.,
        );
        // The original consumes a final draw for an unused per-particle value.
        random.next_float();
        Self {
            path,
            position,
            scale,
            alpha: 0,
            age: 0,
            phase_start: 0,
            phase: Phase::FadeIn,
        }
    }

    fn enter(&mut self, phase: Phase) {
        self.phase = phase;
        self.phase_start = self.age;
    }

    fn update(&mut self, settings: Settings, delta: i32, random: &mut ScriptRandom) {
        let phase_age = self.age.wrapping_sub(self.phase_start) as f32;
        match self.phase {
            Phase::FadeIn => {
                let age = self.age as f32;
                self.alpha = opacity(0., 255., settings.fade_in, age);
                if age > settings.fade_in {
                    self.enter(Phase::Travel);
                }
            }
            Phase::Travel => {
                self.position = self.path.at(phase_age);
                if phase_age > settings.travel {
                    self.enter(Phase::FadeOut);
                }
            }
            Phase::FadeOut => {
                self.position = self.path.at(settings.travel + phase_age);
                self.alpha = opacity(255., 0., settings.fade_out, phase_age);
                if phase_age > settings.fade_out {
                    self.alpha = 0;
                    self.enter(Phase::Inactive);
                }
            }
            Phase::Inactive if settings.repeat => {
                let phase_start = self.phase_start;
                *self = Self::spawn(settings, random);
                self.phase_start = phase_start;
            }
            Phase::Inactive => {}
        }
        self.age = self.age.wrapping_add(delta);
    }
}

fn opacity(from: f32, to: f32, duration: f32, age: f32) -> u8 {
    let value = if age <= 0. {
        from
    } else if age >= duration {
        to
    } else {
        (age / duration) * (to - from) + from
    };
    value as u8
}

pub(super) struct Photon {
    settings: Settings,
    heads: Vec<Particle>,
    active: i32,
    image: [f32; 2],
}

impl Photon {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let settings = Settings {
            center: [p[1], p[2], 0.],
            spread: [p[3], p[4], p[5]],
            scale_range: (p[6] as f64 / 100.) as f32,
            fade_in: p[7],
            travel: p[8],
            fade_out: p[9],
            emit_interval: p[10] as i32,
            emit_count: p[11] as i32,
            repeat: p[12] as i32 != 0,
        };
        let group = (p[0] as i32 as u32).max(1) as usize;
        let count = host.quads.len() / group;
        let heads = (0..count)
            .map(|_| Particle::spawn(settings, random))
            .collect();
        let active = if settings.emit_interval > 0 {
            settings.emit_count
        } else {
            count as i32
        };
        Self {
            settings,
            heads,
            active,
            image,
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        let delta = delta as f32 as i64 as i32;
        let count = self.heads.len() as i32;
        if self.active >= count {
            self.active = count;
        } else if self.settings.emit_interval > 0
            && random
                .next_integer()
                .is_multiple_of(self.settings.emit_interval as u32)
        {
            self.active = self.active.wrapping_add(self.settings.emit_count);
        }
        for head in self.heads.iter_mut().take(self.active.max(0) as usize) {
            head.update(self.settings, delta, random);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (head, quad) in self.heads.iter().zip(&mut host.quads) {
            for (i, vertex) in quad.iter_mut().enumerate() {
                let [x, y] = [(i & 1) as f64, (i >> 1) as f64];
                vertex.position = [
                    (((x - 0.5) * self.image[0] as f64) * head.scale as f64
                        + head.position[0] as f64) as f32,
                    (((y - 0.5) * self.image[1] as f64) * head.scale as f64
                        + head.position[1] as f64) as f32,
                    head.position[2],
                ];
                vertex.color = 0xff_ffff | (u32::from(head.alpha) << 24);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouped_photons_fade_travel_and_respawn_without_touching_other_quads() {
        let image = [32., 24.];
        let mut host = ParticleHost::default();
        host.reset_quads(3, image).unwrap();
        let unused = host.quads[1..].to_vec();
        let params = [
            2f32, 280., 230., 900., 620., 20., 70., 9., 13., 11., 0., 0., 1.,
        ];
        host.parameters[..params.len()].copy_from_slice(&params.map(f32::to_bits));
        let mut random = ScriptRandom::seeded(1);
        let mut effect = Photon::new(&host, image, &mut random);
        effect.write_mesh(&mut host);
        assert_eq!(host.quads[0][0].color >> 24, 0);
        assert_eq!(host.quads[1..], unused);

        let checkpoints = [
            (9, 0, Phase::FadeIn),
            (1, 255, Phase::FadeIn),
            (1, 255, Phase::Travel),
            (13, 255, Phase::Travel),
            (0, 255, Phase::FadeOut),
            (10, 255, Phase::FadeOut),
            (2, 23, Phase::FadeOut),
            (0, 0, Phase::Inactive),
        ];
        for (delta, alpha, phase) in checkpoints {
            effect.update(delta, &mut random);
            assert_eq!(effect.heads[0].alpha, alpha);
            assert_eq!(effect.heads[0].phase as u8, phase as u8);
        }
        assert_eq!(effect.heads[0].position, [280., 230., 0.]);
        assert_eq!(random.clone().next_float().to_bits(), 0x3eaf_2e9d);

        effect.update(1, &mut random);
        effect.write_mesh(&mut host);
        assert_eq!(effect.heads[0].age, 1);
        assert_ne!(effect.heads[0].position, [280., 230., 0.]);
        assert_eq!(host.quads[0][0].color >> 24, 0);
        assert_eq!(host.quads[1..], unused);
    }
}
