/// 基线使用独立的整数和浮点 MT19937 序列，初始化采用旧版 69069 递推。
#[derive(Clone, Debug)]
pub struct ScriptRandom {
    integer: Stream,
    floating: Stream,
}

impl Default for ScriptRandom {
    fn default() -> Self {
        Self::seeded(4357)
    }
}

impl ScriptRandom {
    pub fn seeded(seed: u32) -> Self {
        Self {
            integer: Stream::seeded(seed),
            floating: Stream::seeded(seed),
        }
    }

    pub fn next_integer(&mut self) -> u32 {
        self.integer.next()
    }

    /// 结果在闭区间 [0, 1]，按 double 除法后舍入至 f32。
    pub fn next_float(&mut self) -> f32 {
        (self.floating.next() as f64 * (1.0 / 4294967295.0)) as f32
    }
}

#[derive(Clone, Debug)]
struct Stream {
    values: [u32; 624],
    index: usize,
}

impl Stream {
    fn seeded(mut seed: u32) -> Self {
        let mut values = [0; 624];
        for value in &mut values {
            let high = seed & 0xffff_0000;
            seed = seed.wrapping_mul(69069).wrapping_add(1);
            *value = high | (seed >> 16);
            seed = seed.wrapping_mul(69069).wrapping_add(1);
        }
        Self { values, index: 624 }
    }

    fn next(&mut self) -> u32 {
        if self.index == 624 {
            for i in 0..624 {
                let y = (self.values[i] & 0x8000_0000) | (self.values[(i + 1) % 624] & 0x7fff_ffff);
                self.values[i] = self.values[(i + 397) % 624]
                    ^ (y >> 1)
                    ^ if y & 1 == 1 { 0x9908_b0df } else { 0 };
            }
            self.index = 0;
        }
        let mut value = self.values[self.index];
        self.index += 1;
        value ^= value >> 11;
        value ^= (value << 7) & 0x9d2c_5680;
        value ^= (value << 15) & 0xefc6_0000;
        value ^ (value >> 18)
    }
}
