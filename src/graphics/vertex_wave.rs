use crate::{Result, read};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct VertexWave {
    pub axis: u32,
    pub amplitude: f32,
    pub wavelength: f32,
    pub period: f32,
    pub phase: u32,
    pub normalized: bool,
}

impl Default for VertexWave {
    fn default() -> Self {
        Self {
            axis: 0,
            amplitude: 0.,
            wavelength: 0.,
            period: 1.,
            phase: 0,
            normalized: false,
        }
    }
}

impl VertexWave {
    pub fn configure(&mut self, bytes: &[u8]) -> Result<u32> {
        let target = read::u32_at(bytes, 0)?;
        let axis = read::u32_at(bytes, 4)?;
        self.axis = axis & 0x7fff_ffff;
        self.normalized = axis >> 31 != 0;
        self.change(bytes)?;
        Ok(target)
    }

    pub fn change(&mut self, bytes: &[u8]) -> Result<()> {
        let values = [8, 12, 16].map(|at| read::u32_at(bytes, at).map(f32::from_bits));
        let [amplitude, wavelength, period] = values;
        self.amplitude = amplitude?;
        let wavelength = wavelength?;
        self.wavelength = if self.normalized {
            1. / wavelength
        } else {
            wavelength
        };
        let period = period?;
        self.period = if self.normalized {
            (period / 60.) * 60.
        } else {
            period
        };
        Ok(())
    }

    pub fn advance(&mut self, delta: u32, speed: u32) {
        let speed = match speed {
            1 => 1.5,
            2 => 2.,
            3 => 3.,
            4 => 4.,
            _ => 1.,
        };
        self.phase = self.phase.wrapping_add(unsigned(delta as f32 * speed));
        let length = unsigned(self.wavelength);
        if self.phase >= length {
            self.phase = self.phase.wrapping_sub(length);
        }
    }

    pub fn apply(&self, mut position: [f32; 3]) -> [f32; 3] {
        let axis = usize::from(self.axis != 1);
        let coordinate = position[1 - axis];
        let angle = if self.normalized {
            let time = (self.phase as f32 / self.period).fract();
            let time = (f64::from(time) * f64::from(std::f32::consts::TAU)) as f32;
            let spatial = (coordinate * self.wavelength).fract();
            spatial * std::f32::consts::TAU + time
        } else {
            let distance = f64::from(self.phase as f32 + coordinate);
            let remainder = (distance % f64::from(self.wavelength)) as f32;
            let half = remainder * std::f32::consts::PI;
            (half + half) / self.wavelength
        };
        let offset = (f64::from(angle).sin() as f32) * self.amplitude;
        position[axis] += offset;
        position
    }
}

fn unsigned(value: f32) -> u32 {
    if value.is_finite() {
        value.trunc() as i64 as u32
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_modes_keep_single_subtraction_and_independent_time_parameters() {
        let parameters = |axis| {
            [5, axis, 4f32.to_bits(), 40f32.to_bits(), 60f32.to_bits()]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
        };
        let mut wave = VertexWave::default();
        assert_eq!(wave.configure(&parameters(1)).unwrap(), 5);
        wave.advance(100, 0);
        assert_eq!(wave.phase, 60);
        wave.advance(0, 4);
        assert_eq!(wave.phase, 20);
        let point = wave.apply([2., 10., 7.]);
        assert!((point[0] + 2.).abs() < 0.000001);
        assert_eq!(&point[1..], &[10., 7.]);

        wave.configure(&parameters(0x8000_0001)).unwrap();
        wave.phase = 0;
        wave.advance(15, 0);
        assert_eq!(wave.phase, 15);
        assert!((wave.apply([0.; 3])[0] - 4.).abs() < 0.000001);
        let mut changed = parameters(2);
        changed[8..12].copy_from_slice(&(-2f32).to_le_bytes());
        wave.change(&changed).unwrap();
        assert_eq!((wave.axis, wave.normalized, wave.phase), (1, true, 15));
        assert!((wave.apply([0.; 3])[0] + 2.).abs() < 0.000001);
    }
}
