use super::{Host, Interpreter, NativeReturn, Program, ScriptRandom, matrix, numeric, strings};
use crate::{Error, Result, Step, read};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleVertex {
    pub position: [f32; 3],
    pub rhw: f32,
    pub color: u32,
    pub uv: [f32; 2],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ParticleSource {
    pub color: u32,
    pub position_2d: [f32; 2],
    pub position_3d: [f32; 3],
}

impl Default for ParticleVertex {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            rhw: 1.0,
            color: u32::MAX,
            uv: [0.0; 2],
        }
    }
}

/// KCS 2.0 粒子任务表。宿主在每次推进前提供时间和当前网格上下文。
pub struct ParticleHost {
    pub frame_rate: u32,
    pub elapsed: u32,
    pub previous_elapsed: u32,
    pub active: bool,
    pub current_quad: u32,
    pub quads: Vec<[ParticleVertex; 4]>,
    pub saved_quads: Vec<[ParticleVertex; 4]>,
    pub offsets: Vec<[f32; 4]>,
    pub quad_flags: Vec<u32>,
    pub parameters: [u32; 32],
    pub surface_size: [u32; 2],
    pub depth_offset: f32,
    pub camera_depth: f32,
    pub object_id: i32,
    pub source: Option<ParticleSource>,
    pub render_mode: u32,
    pub random: ScriptRandom,
    pub debug_messages: Vec<Vec<u8>>,
}

impl Default for ParticleHost {
    fn default() -> Self {
        Self {
            frame_rate: 60,
            elapsed: 0,
            previous_elapsed: u32::MAX,
            active: true,
            current_quad: 0,
            quads: vec![[ParticleVertex::default(); 4]],
            saved_quads: vec![[ParticleVertex::default(); 4]],
            offsets: vec![[0.0; 4]],
            quad_flags: vec![1],
            parameters: [0; 32],
            surface_size: [0; 2],
            depth_offset: 0.0,
            camera_depth: 0.0,
            object_id: -1,
            source: None,
            render_mode: 0,
            random: ScriptRandom::default(),
            debug_messages: Vec::new(),
        }
    }
}

/// 每个网格从入口执行一次，共享同一份程序数据和粒子宿主。
pub struct ParticleTask {
    pub interpreter: Interpreter,
    pub host: ParticleHost,
    pub speed: u32,
    pub origin: u32,
    pub previous_time: u32,
    invocation: Option<usize>,
}

impl ParticleTask {
    pub fn new(program: Program, host: ParticleHost) -> Result<Self> {
        Ok(Self {
            interpreter: Interpreter::new(program, Vec::new())?,
            host,
            speed: 1,
            origin: 0,
            previous_time: 0,
            invocation: None,
        })
    }

    /// 预算按单个网格计算；让出后再次调用会继续当前网格和本次更新。
    pub fn update(&mut self, now: u32, budget_per_quad: usize) -> Result<Option<Step<()>>> {
        if self.invocation.is_none() {
            if !self.host.active {
                return Ok(None);
            }
            let count = self.host.quads.len();
            if count > u32::MAX as usize
                || self.host.saved_quads.len() != count
                || self.host.offsets.len() != count
                || self.host.quad_flags.len() != count
            {
                return Err(Error::new(self.interpreter.pc, "粒子网格数组数量不匹配"));
            }
            if self.origin == 0 {
                self.origin = now;
                self.host.elapsed = 0;
                self.host.previous_elapsed = u32::MAX;
            } else {
                self.host.elapsed = self.host.elapsed.wrapping_add(
                    now.wrapping_sub(self.previous_time)
                        .wrapping_mul(self.speed),
                );
            }
            self.previous_time = now;
            self.host.current_quad = 0;
            self.invocation = Some(count);
            if count != 0 {
                self.interpreter.reset_execution();
            }
        }
        let count = self.invocation.unwrap();
        while (self.host.current_quad as usize) < count {
            match self.interpreter.run(&mut self.host, budget_per_quad) {
                Ok(Step::Yield) => return Ok(Some(Step::Yield)),
                Err(error) => {
                    self.invocation = None;
                    return Err(error);
                }
                Ok(_) => {
                    self.host.current_quad += 1;
                    if (self.host.current_quad as usize) < count {
                        self.interpreter.reset_execution();
                    }
                }
            }
        }
        self.host.previous_elapsed = self.host.elapsed;
        self.invocation = None;
        Ok(Some(Step::Halted))
    }
}

impl Host for ParticleHost {
    fn call(&mut self, index: u32, arguments: &[u8], vm: &mut Interpreter) -> Result<NativeReturn> {
        let count = match index {
            1 | 2 | 4 | 32..=34 | 36 | 40 | 44..=47 => 0,
            0 | 3 | 6..=11 | 13..=15 | 17 | 19..=21 | 30 | 31 | 35 | 42 | 50 => 1,
            5 | 12 | 16 | 18 | 22..=24 | 39 | 43 | 48 => 2,
            27..=29 | 49 => 3,
            25 | 26 => 4,
            37 => 6,
            38 | 41 => 5,
            _ => {
                return Err(Error::new(
                    vm.pc,
                    format!("未支持的粒子 KCS native {index}"),
                ));
            }
        };
        if arguments.len() != count * 4 {
            return Err(Error::new(vm.pc, "粒子 KCS native 参数字节数不匹配"));
        }
        let args: Vec<_> = arguments
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let bits = match index {
            0 => {
                self.debug_messages.push(strings::format(vm, args[0], 0)?);
                return Ok(NativeReturn::Continue);
            }
            1 => self.elapsed,
            2 => self.previous_elapsed,
            3 => {
                let frames = args[0];
                if frames as i32 <= 0 {
                    frames
                } else {
                    let scaled = (self.frame_rate as f32 / 60.0) * frames as f32;
                    numeric::convert(0x38, scaled.to_bits()).max(1)
                }
            }
            4 => self.frame_rate,
            5 => {
                for (address, size) in args.iter().zip(self.surface_size) {
                    vm.write(*address, &size.to_le_bytes())?;
                }
                return Ok(NativeReturn::Continue);
            }
            6..=18 => numeric::native_math(index, &args),
            19 => (f32::from_bits(args[0]) * std::f32::consts::PI / 180.0).to_bits(),
            20 => (f32::from_bits(args[0]) * 180.0 / std::f32::consts::PI).to_bits(),
            21 => (args[0] as i32).wrapping_abs() as u32,
            22..=26 => {
                let mut quad = read_quad(vm, args[0])?;
                for vertex in &mut quad {
                    match index {
                        22..=24 => {
                            let s = f32::from_bits(numeric::native_math(6, &args[1..2]));
                            let c = f32::from_bits(numeric::native_math(7, &args[1..2]));
                            let [x, y, z] = vertex.position;
                            vertex.position = match index {
                                22 => [x, z * s + y * c, z * c - y * s],
                                23 => [x * c - z * s, y, z * c + x * s],
                                24 => [x * c + y * s, y * c - x * s, z],
                                _ => unreachable!(),
                            };
                        }
                        25 => {
                            for (n, delta) in vertex.position.iter_mut().zip(&args[1..]) {
                                *n += f32::from_bits(*delta);
                            }
                            vertex.rhw += 0.0;
                        }
                        26 => {
                            for (n, scale) in vertex.position.iter_mut().zip(&args[1..]) {
                                *n *= f32::from_bits(*scale);
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                write_quad(vm, args[0], &quad)?;
                return Ok(NativeReturn::Continue);
            }
            28 => {
                let offset = self
                    .offsets
                    .get(self.current_quad as usize)
                    .ok_or_else(|| Error::new(vm.pc, "粒子网格编号越界"))?;
                for (address, value) in args.iter().zip(offset) {
                    vm.write(*address, &value.to_bits().to_le_bytes())?;
                }
                return Ok(NativeReturn::Continue);
            }
            27 => {
                let offset = self
                    .offsets
                    .get_mut(self.current_quad as usize)
                    .ok_or_else(|| Error::new(vm.pc, "粒子网格编号越界"))?;
                *offset = [
                    f32::from_bits(args[0]),
                    f32::from_bits(args[1]),
                    f32::from_bits(args[2]),
                    0.0,
                ];
                return Ok(NativeReturn::Continue);
            }
            29 => {
                let quad = self
                    .quads
                    .get_mut(self.current_quad as usize)
                    .ok_or_else(|| Error::new(vm.pc, "粒子网格编号越界"))?;
                let first = f32::from_bits(args[0]) - 0.5;
                let second = f32::from_bits(args[2]) - 0.5;
                let [width, height] = self.surface_size.map(|n| n as f32);
                for (vertex, uv) in quad.iter_mut().zip([
                    [first / width, first / height],
                    [second / width, first / height],
                    [first / width, second / height],
                    [second / width, second / height],
                ]) {
                    vertex.uv = uv;
                }
                return Ok(NativeReturn::Continue);
            }
            30 => {
                let quad = self
                    .quads
                    .get(self.current_quad as usize)
                    .ok_or_else(|| Error::new(vm.pc, "粒子网格编号越界"))?;
                write_quad(vm, args[0], quad)?;
                return Ok(NativeReturn::Continue);
            }
            31 => {
                let quad = read_quad(vm, args[0])?;
                *self
                    .quads
                    .get_mut(self.current_quad as usize)
                    .ok_or_else(|| Error::new(vm.pc, "粒子网格编号越界"))? = quad;
                return Ok(NativeReturn::Continue);
            }
            32 => {
                self.active = false;
                return Ok(NativeReturn::YieldVoid);
            }
            33 => self.random.next_integer(),
            34 => self.random.next_float().to_bits(),
            35 => {
                let bytes: Vec<_> = self
                    .parameters
                    .iter()
                    .flat_map(|n| n.to_le_bytes())
                    .collect();
                vm.write(args[0], &bytes)?;
                return Ok(NativeReturn::Continue);
            }
            36 => self.current_quad,
            37 => {
                let mut total = [0.0; 3];
                for i in 0..3 {
                    let d = f32::from_bits(args[i]) - f32::from_bits(args[i + 3]);
                    total[i] = d * d;
                }
                numeric::native_math(17, &[(total[1] + total[0] + total[2]).to_bits()])
            }
            38 => {
                let values = self.projection_bounds(f32::from_bits(args[0]));
                for (address, value) in args[1..].iter().zip(values) {
                    vm.write(*address, &value.to_le_bytes())?;
                }
                return Ok(NativeReturn::Continue);
            }
            39 => {
                if let Some(flag) = self.quad_flags.get_mut(args[0] as usize) {
                    *flag = args[1];
                }
                return Ok(NativeReturn::Continue);
            }
            40 => (self.quads.len() as u32).wrapping_sub(1),
            41 => {
                let v: Vec<_> = args.iter().map(|&n| f32::from_bits(n)).collect();
                if v[2] >= v[4] {
                    args[0]
                } else if v[4] >= v[3] {
                    args[1]
                } else {
                    (((v[4] - v[2]) / (v[3] - v[2])) * (v[1] - v[0]) + v[0]).to_bits()
                }
            }
            42 => {
                let rounded = (f32::from_bits(args[0]) + 0.5).floor();
                numeric::convert(0x39, rounded.to_bits())
            }
            43 => {
                numeric::native_triangle(f32::from_bits(args[0]), f32::from_bits(args[1])).to_bits()
            }
            44 | 45 => {
                if self.saved_quads.len() != self.quads.len() {
                    return Err(Error::new(vm.pc, "粒子网格快照数量不匹配"));
                }
                if index == 44 {
                    self.quads.clone_from(&self.saved_quads);
                } else {
                    self.saved_quads.clone_from(&self.quads);
                }
                return Ok(NativeReturn::Continue);
            }
            46 => self.object_id as u32,
            47..=49 => {
                let source = if self.object_id == -1 {
                    None
                } else {
                    self.source
                };
                let source = source.unwrap_or_default();
                if index == 47 {
                    source.color
                } else {
                    let position: &[f32] = if index == 48 {
                        &source.position_2d
                    } else {
                        &source.position_3d
                    };
                    for (address, value) in args.iter().zip(position) {
                        vm.write(*address, &value.to_le_bytes())?;
                    }
                    return Ok(NativeReturn::Continue);
                }
            }
            50 => {
                self.render_mode = if args[0] <= 5 { args[0] } else { 0 };
                return Ok(NativeReturn::Continue);
            }
            _ => unreachable!(),
        };
        Ok(NativeReturn::Value(bits))
    }
}

impl ParticleHost {
    /// 重建矩形网格、快照、零偏移和启用标记，保留任务时间及参数。
    pub fn reset_quads(&mut self, count: usize, image_size: [f32; 2]) -> Result<()> {
        if count > read::MAX_BYTES / 244 {
            return Err(Error::new(0, "粒子网格存储超过上限"));
        }
        let [width, height] = image_size;
        let positions = [[0.0, 0.0], [width, 0.0], [0.0, height], [width, height]];
        let uv = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
        let quad = std::array::from_fn(|i| ParticleVertex {
            position: [positions[i][0], positions[i][1], 0.0],
            uv: uv[i],
            ..Default::default()
        });
        self.quads = vec![quad; count];
        self.saved_quads.clone_from(&self.quads);
        self.offsets = vec![[0.0; 4]; count];
        self.quad_flags = vec![1; count];
        Ok(())
    }

    fn projection_bounds(&self, depth: f32) -> [f32; 4] {
        let f =
            |op, a: f32, b: f32| f32::from_bits(numeric::float_ieee(op, a.to_bits(), b.to_bits()));
        let depth = f(2, 65000.0, f(1, self.depth_offset, depth));
        let distance = if depth == self.camera_depth {
            1.0
        } else {
            f(2, depth, self.camera_depth)
        };
        let scale = f(4, 1000.0, distance);
        let scale = if scale < 0.0 { 0.0 } else { scale };
        let mut projection = [0.0; 16];
        projection[0] = scale;
        projection[5] = scale;
        projection[10] = f32::from_bits(0x3781_0e36);
        projection[15] = f(4, 1.0, distance);
        let identity = std::array::from_fn(|i| if i % 5 == 0 { 1.0 } else { 0.0 });
        let projection = matrix::multiply(identity, projection);
        let projection = matrix::inverse(projection).unwrap_or(projection);
        let [x, y] = self.surface_size.map(|n| (n >> 1) as f32);
        let point = matrix::transform([x, y, depth, 1.0], projection);
        [
            f(2, x, point[0]),
            f(2, y, point[1]),
            f(1, point[0], x),
            f(1, point[1], y),
        ]
    }
}

fn read_quad(vm: &Interpreter, address: u32) -> Result<[ParticleVertex; 4]> {
    let bytes = vm.read(address, 112)?;
    Ok(std::array::from_fn(|i| {
        let v: [u32; 7] = std::array::from_fn(|j| {
            let start = i * 28 + j * 4;
            u32::from_le_bytes(bytes[start..start + 4].try_into().unwrap())
        });
        ParticleVertex {
            position: [v[0], v[1], v[2]].map(f32::from_bits),
            rhw: f32::from_bits(v[3]),
            color: v[4],
            uv: [v[5], v[6]].map(f32::from_bits),
        }
    }))
}

fn write_quad(vm: &mut Interpreter, address: u32, quad: &[ParticleVertex; 4]) -> Result<()> {
    let bytes: Vec<_> = quad
        .iter()
        .flat_map(|v| {
            [
                v.position[0].to_bits(),
                v.position[1].to_bits(),
                v.position[2].to_bits(),
                v.rhw.to_bits(),
                v.color,
                v.uv[0].to_bits(),
                v.uv[1].to_bits(),
            ]
        })
        .flat_map(u32::to_le_bytes)
        .collect();
    vm.write(address, &bytes)
}
