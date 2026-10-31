use crate::{Error, Result, Step, read};
use std::collections::BTreeMap;

mod host;
mod matrix;
mod motion;
mod numeric;
mod particle;
mod queue;
mod random;
mod sse;
mod strings;
pub use host::CoreHost;
pub use motion::{MotionHost, MotionState, MotionTask};
pub use particle::{ParticleHost, ParticleSource, ParticleTask, ParticleVertex};
pub use queue::MotionQueue;
pub use random::ScriptRandom;

#[derive(Clone, Copy, Debug)]
pub struct Operand {
    pub flags: u8,
    pub payload: u32,
}

#[derive(Clone, Debug)]
pub struct Instruction {
    pub offset: usize,
    pub opcode: u8,
    pub operands: Vec<Operand>,
}

#[derive(Clone, Debug)]
pub struct Program {
    instructions: Vec<Instruction>,
    pub control: u32,
    pub reserved: u32,
    code_len: usize,
    data: Vec<u8>,
    stack_size: usize,
}

pub fn operand_count(opcode: u8) -> Option<usize> {
    Some(match opcode {
        0x00 | 0x2d | 0x33 | 0x3a..=0x3c => 0,
        0x08
        | 0x0c
        | 0x16..=0x1a
        | 0x25..=0x29
        | 0x2e..=0x2f
        | 0x31..=0x32
        | 0x36..=0x39
        | 0x3d
        | 0x42
        | 0x44 => 1,
        0x01..=0x07
        | 0x09..=0x0b
        | 0x0d..=0x15
        | 0x1b..=0x24
        | 0x2a..=0x2c
        | 0x30
        | 0x34
        | 0x3e
        | 0x41
        | 0x43 => 2,
        0x35 => 3,
        _ => return None,
    })
}

impl Program {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > read::MAX_BYTES {
            return Err(Error::new(0, "KCS 容器超过读取上限"));
        }
        if bytes.get(..4) != Some(b"KCS\0")
            || read::u32_at(bytes, 4)? != 2
            || read::u32_at(bytes, 12)? != 52
        {
            return Err(Error::new(0, "未支持的 KCS 容器"));
        }
        let a = read::sized(read::u32_at(bytes, 16)?, 16)?;
        let b = read::sized(read::u32_at(bytes, 28)?, 28)?;
        let n = read::u32_at(bytes, 24)? as usize;
        let d = read::sized(read::u32_at(bytes, 32)?, 32)?;
        let s = read::sized(read::u32_at(bytes, 36)?, 36)?;
        let capacity = read::sized(read::u32_at(bytes, 48)?, 48)?;
        if read::u32_at(bytes, 20)? as usize != 52 + a
            || read::u32_at(bytes, 40)? as usize != 52 + a + b
            || a + b > capacity
            || n > b / 8
            || d + s > read::MAX_BYTES
        {
            return Err(Error::new(16, "KCS 区间字段不一致"));
        }
        let body = read::inflate(
            bytes
                .get(52..)
                .ok_or_else(|| Error::new(52, "KCS 头被截断"))?,
            a + b,
        )?;
        let init = &body[a..];
        let mut data = vec![0; d];
        for i in 0..n {
            let source = read::u32_at(init, i * 8)? as usize;
            let target = read::u32_at(init, i * 8 + 4)? as usize;
            if source < n * 8 {
                return Err(Error::new(i * 8, "字符串源指向初始化表"));
            }
            let string = read::zstr(init, source)?;
            let len = string.len() + 1;
            if target.checked_add(len).is_none_or(|end| end > d) {
                return Err(Error::new(i * 8 + 4, "字符串目标超过数据区"));
            }
            data[target..target + string.len()].copy_from_slice(string);
            data[target + string.len()] = 0;
        }
        let mut program = Self::from_code(&body[..a], data, s)?;
        program.control = read::u32_at(bytes, 8)?;
        program.reserved = read::u32_at(bytes, 44)?;
        Ok(program)
    }

    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    pub fn from_code(code: &[u8], data: Vec<u8>, stack_size: usize) -> Result<Self> {
        if code.len() > read::MAX_BYTES
            || data
                .len()
                .checked_add(stack_size)
                .is_none_or(|n| n > read::MAX_BYTES)
        {
            return Err(Error::new(0, "KCS 存储超过上限"));
        }
        let mut instructions = Vec::new();
        let mut p = 0;
        while p < code.len() {
            let offset = p;
            let opcode = code[p];
            p += 1;
            let count = operand_count(opcode)
                .ok_or_else(|| Error::new(offset, format!("无效 KCS opcode {opcode:02x}")))?;
            let mut operands = Vec::new();
            for _ in 0..count {
                operands.push(Operand {
                    flags: read::range(code, p, 1)?[0],
                    payload: read::u32_at(code, p + 1)?,
                });
                p += 5;
            }
            instructions.push(Instruction {
                offset,
                opcode,
                operands,
            });
            if instructions.len() > read::MAX_ITEMS {
                return Err(Error::new(offset, "KCS 指令数量超过上限"));
            }
        }
        for ins in &instructions {
            let target = match ins.opcode {
                0x27..=0x2a => Some(0),
                0x34 => Some(1),
                0x35 => Some(2),
                _ => None,
            };
            if let Some(arg) = target {
                let op = ins.operands[arg];
                if op.flags == 3
                    && op.payload as usize != code.len()
                    && instructions
                        .binary_search_by_key(&(op.payload as usize), |i| i.offset)
                        .is_err()
                {
                    return Err(Error::new(ins.offset, "KCS 静态目标不在指令边界"));
                }
            }
        }
        Ok(Self {
            instructions,
            control: 0,
            reserved: 0,
            code_len: code.len(),
            data,
            stack_size,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeReturn {
    Yield,
    Value(u32),
    Continue,
    YieldValue(u32),
    YieldVoid,
    Retry,
}

pub trait Host {
    fn call(&mut self, index: u32, arguments: &[u8], vm: &mut Interpreter) -> Result<NativeReturn>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub native_index: u32,
    pub status: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DebugState {
    pub line: u32,
    pub source: Vec<u8>,
    pub scopes: Vec<u32>,
    pub variables: Vec<u32>,
}

pub struct Interpreter {
    program: Program,
    boundaries: BTreeMap<usize, usize>,
    pub pc: usize,
    memory: Vec<u8>,
    external: Vec<u8>,
    sp: usize,
    frame_top: usize,
    frame_base: usize,
    stack_start: usize,
    frame_floors: Vec<usize>,
    switch: u32,
    debug: DebugState,
    debug_variable_floors: Vec<usize>,
}

impl Interpreter {
    pub fn new(program: Program, external: Vec<u8>) -> Result<Self> {
        if external.len() > read::MAX_BYTES {
            return Err(Error::new(0, "外部存储超过上限"));
        }
        let stack_start = program.data.len();
        let mut memory = program.data.clone();
        memory.resize(stack_start + program.stack_size, 0);
        let boundaries = program
            .instructions
            .iter()
            .enumerate()
            .map(|(i, ins)| (ins.offset, i))
            .collect();
        Ok(Self {
            program,
            boundaries,
            pc: 0,
            memory,
            external,
            sp: stack_start,
            frame_top: stack_start,
            frame_base: stack_start,
            stack_start,
            frame_floors: vec![stack_start],
            switch: 0,
            debug: DebugState::default(),
            debug_variable_floors: Vec::new(),
        })
    }
    pub fn debug(&self) -> &DebugState {
        &self.debug
    }
    pub fn stack_bytes(&self) -> &[u8] {
        &self.memory[self.stack_start..self.sp]
    }
    /// 从入口重新执行，保留程序数据、外部区及调试元数据。
    pub fn reset_execution(&mut self) {
        self.pc = 0;
        self.sp = self.stack_start;
        self.frame_base = self.stack_start;
        self.frame_top = self.stack_start;
        self.frame_floors.clear();
        self.frame_floors.push(self.stack_start);
        self.debug_variable_floors.clear();
    }
    pub fn read(&self, address: u32, length: usize) -> Result<&[u8]> {
        let storage = if address & 0x8000_0000 != 0 {
            &self.external
        } else {
            &self.memory
        };
        read::range(storage, (address & 0x7fff_ffff) as usize, length)
    }
    pub fn read_string(&self, address: u32) -> Result<&[u8]> {
        let storage = if address & 0x8000_0000 != 0 {
            &self.external
        } else {
            &self.memory
        };
        read::zstr(storage, (address & 0x7fff_ffff) as usize)
    }
    pub fn write(&mut self, address: u32, bytes: &[u8]) -> Result<()> {
        let p = (address & 0x7fff_ffff) as usize;
        let storage = if address & 0x8000_0000 != 0 {
            &mut self.external
        } else {
            &mut self.memory
        };
        let end = p
            .checked_add(bytes.len())
            .filter(|&n| n <= storage.len())
            .ok_or_else(|| Error::new(self.pc, "KCS 写入越界"))?;
        storage[p..end].copy_from_slice(bytes);
        Ok(())
    }
    fn push(&mut self, n: u32) -> Result<()> {
        let end = self
            .sp
            .checked_add(4)
            .filter(|&n| n <= self.memory.len())
            .ok_or_else(|| Error::new(self.pc, "KCS 栈溢出"))?;
        self.memory[self.sp..end].copy_from_slice(&n.to_le_bytes());
        self.sp = end;
        Ok(())
    }
    fn pop(&mut self) -> Result<u32> {
        if self.sp < self.frame_top + 4 {
            return Err(Error::new(self.pc, "KCS 操作栈下溢"));
        }
        self.sp -= 4;
        read::u32_at(&self.memory, self.sp)
    }
    fn raw(&mut self, op: Operand) -> Result<u32> {
        if op.flags & 0x40 != 0 {
            self.pop()
        } else {
            Ok(op.payload)
        }
    }
    fn value(&mut self, op: Operand) -> Result<u32> {
        let raw = self.raw(op)?;
        if op.flags & 0x10 == 0 {
            return Ok(raw);
        }
        let width = width(op.flags)?;
        let bytes = self.read(raw, width)?;
        Ok(match width {
            1 if op.flags & 0x20 != 0 => bytes[0] as i8 as i32 as u32,
            1 => bytes[0] as u32,
            2 if op.flags & 0x20 != 0 => {
                i16::from_le_bytes(bytes.try_into().unwrap()) as i32 as u32
            }
            2 => u16::from_le_bytes(bytes.try_into().unwrap()) as u32,
            _ => u32::from_le_bytes(bytes.try_into().unwrap()),
        })
    }
    fn jump(&mut self, target: u32) -> Result<()> {
        if target as usize != self.program.code_len
            && !self.boundaries.contains_key(&(target as usize))
        {
            return Err(Error::new(self.pc, "KCS 目标不在指令边界"));
        }
        self.pc = target as usize;
        Ok(())
    }
    pub fn run(&mut self, host: &mut impl Host, budget: usize) -> Result<Step<Event>> {
        for _ in 0..budget {
            if self.pc == self.program.code_len {
                return Ok(Step::Halted);
            }
            let i = *self
                .boundaries
                .get(&self.pc)
                .ok_or_else(|| Error::new(self.pc, "PC 不在指令边界"))?;
            let ins = self.program.instructions[i].clone();
            if ins.opcode == 0x3b {
                return Ok(Step::Halted);
            }
            self.pc += 1 + 5 * ins.operands.len();
            let a = &ins.operands;
            match ins.opcode {
                0x00 => (),
                0x01..=0x07 | 0x09..=0x0b | 0x0d..=0x14 => {
                    let left = self.value(a[0])?;
                    let right = self.value(a[1])?;
                    let n = numeric::binary(
                        ins.opcode, left, right, a[0].flags, a[1].flags, ins.offset,
                    )?;
                    self.push(n)?;
                }
                0x08 | 0x0c | 0x1a => {
                    let n = self.value(a[0])?;
                    self.push(match ins.opcode {
                        8 => !n,
                        12 => (n == 0) as u32,
                        _ if a[0].flags & 4 != 0 => n ^ 0x8000_0000,
                        _ => n.wrapping_neg(),
                    })?;
                }
                0x15 => {
                    let target = self.raw(a[0])?;
                    let n = self.value(a[1])?;
                    let width = width(a[0].flags)?;
                    self.write(target, &n.to_le_bytes()[..width])?;
                    self.push(match width {
                        1 => n & 255,
                        2 => n & 65535,
                        _ => n,
                    })?;
                }
                0x16..=0x19 => {
                    let target = self.raw(a[0])?;
                    let old = read::u32_at(self.read(target, 4)?, 0)?;
                    let width = width(a[0].flags & 0x7f)?;
                    let subtract = matches!(ins.opcode, 0x17 | 0x19);
                    let result = if a[0].flags & 15 == 4 {
                        numeric::float(if subtract { 2 } else { 1 }, old, 1f32.to_bits())
                    } else {
                        old.wrapping_add(if subtract { u32::MAX } else { 1 })
                    };
                    self.write(target, &result.to_le_bytes()[..width])?;
                    let result = if ins.opcode >= 0x18 {
                        old
                    } else {
                        read::u32_at(self.read(target, 4)?, 0)?
                    };
                    self.push(result)?;
                }
                0x1b..=0x24 => {
                    let target = self.raw(a[0])?;
                    let right = self.value(a[1])?;
                    let left = read::u32_at(self.read(target, 4)?, 0)?;
                    let n = numeric::binary(
                        ins.opcode, left, right, a[0].flags, a[1].flags, ins.offset,
                    )?;
                    self.write(target, &n.to_le_bytes()[..width(a[0].flags)?])?;
                    self.push(n)?;
                }
                0x25 => {
                    if a[0].flags & 0x10 != 0 {
                        let address = self.raw(a[0])?;
                        let n = read::u32_at(self.read(address, 4)?, 0)?;
                        self.push(n)?;
                    } else if a[0].flags & 0x40 == 0 {
                        self.push(a[0].payload)?;
                    }
                }
                0x26 => {
                    let target = self.raw(a[0])?;
                    let n = self.pop()?;
                    self.write(target, &n.to_le_bytes()[..width(a[0].flags & 0x7f)?])?;
                }
                0x27..=0x29 => {
                    let target = self.value(a[0])?;
                    let jump = if ins.opcode == 0x29 {
                        true
                    } else {
                        let v = self.pop()?;
                        if ins.opcode == 0x27 { v == 0 } else { v != 0 }
                    };
                    self.sp = self.frame_top;
                    if jump {
                        self.jump(target)?;
                    }
                }
                0x2a => {
                    let target = self.value(a[0])?;
                    let bytes = self.value(a[1])? as usize;
                    if bytes > self.sp - self.frame_top {
                        return Err(Error::new(ins.offset, "KCS 调用参数下溢"));
                    }
                    let base = self.sp - bytes;
                    self.push(self.pc as u32)?;
                    self.push(self.frame_base as u32)?;
                    self.push(self.frame_top as u32)?;
                    self.frame_base = base;
                    self.frame_top = self.sp;
                    self.jump(target)?;
                    self.frame_floors.push(self.frame_top);
                    self.debug_variable_floors.push(self.debug.variables.len());
                }
                0x2b => {
                    let index = self.value(a[0])?;
                    let bytes = self.value(a[1])? as usize;
                    if bytes > self.sp - self.frame_top {
                        return Err(Error::new(ins.offset, "KCS 宿主参数下溢"));
                    }
                    let old_sp = self.sp;
                    self.sp -= bytes;
                    let arguments = self.memory[self.sp..old_sp].to_vec();
                    let result = host.call(index, &arguments, self)?;
                    let status = match result {
                        NativeReturn::Yield => 0,
                        NativeReturn::Value(_) => 1,
                        NativeReturn::Continue => 2,
                        NativeReturn::YieldValue(_) => 3,
                        NativeReturn::YieldVoid => 4,
                        NativeReturn::Retry => 5,
                    };
                    if let NativeReturn::Value(n) | NativeReturn::YieldValue(n) = result {
                        self.push(n)?;
                    }
                    if result == NativeReturn::Retry {
                        self.sp = old_sp;
                        self.pc = ins.offset;
                    }
                    if !matches!(result, NativeReturn::Value(_) | NativeReturn::Continue) {
                        return Ok(Step::Event(Event {
                            native_index: index,
                            status,
                        }));
                    }
                }
                0x2c => {
                    if self.frame_floors.len() < 2 {
                        return Err(Error::new(ins.offset, "KCS return 没有调用帧"));
                    }
                    let result_len = self.value(a[0])? as usize;
                    let parameter_len = self.value(a[1])? as usize;
                    if result_len > self.sp - self.frame_top {
                        return Err(Error::new(ins.offset, "KCS 返回值下溢"));
                    }
                    let result = self.memory[self.sp - result_len..self.sp].to_vec();
                    let saved = self
                        .frame_base
                        .checked_add(parameter_len)
                        .filter(|&n| n + 12 == *self.frame_floors.last().unwrap())
                        .ok_or_else(|| Error::new(ins.offset, "KCS 返回帧无效"))?;
                    let pc = read::u32_at(&self.memory, saved)?;
                    let base = read::u32_at(&self.memory, saved + 4)? as usize;
                    let top = read::u32_at(&self.memory, saved + 8)? as usize;
                    let destination = self.frame_base;
                    if base < self.stack_start
                        || base > top
                        || top > destination
                        || destination + result_len > self.memory.len()
                    {
                        return Err(Error::new(ins.offset, "KCS 保存帧越界"));
                    }
                    self.frame_base = base;
                    self.frame_top = top;
                    self.sp = destination + result_len;
                    self.frame_floors.pop();
                    self.debug.scopes.pop();
                    if let Some(floor) = self.debug_variable_floors.pop() {
                        self.debug.variables.truncate(floor);
                    }
                    self.memory[destination..self.sp].copy_from_slice(&result);
                    self.jump(pc)?;
                }
                0x2d => {
                    self.pop()?;
                }
                0x2e => {
                    let n = self.raw(a[0])?;
                    self.push(n)?;
                }
                0x2f => {
                    let address = self.raw(a[0])?;
                    let n = u32::from_le_bytes(self.read(address, 4)?.try_into().unwrap());
                    self.push(n)?;
                }
                0x30 => {
                    let target = self.raw(a[0])?;
                    let bytes = self.value(a[1])? as usize;
                    if bytes > self.sp - self.frame_top {
                        return Err(Error::new(ins.offset, "KCS 块复制源下溢"));
                    }
                    let source = self.memory[self.sp - bytes..self.sp].to_vec();
                    self.write(target, &source)?;
                }
                0x31 | 0x32 => {
                    let bytes = self.value(a[0])? as usize;
                    let top = if ins.opcode == 0x31 {
                        self.frame_top.checked_add(bytes)
                    } else {
                        self.frame_top.checked_sub(bytes)
                    }
                    .filter(|&n| n >= self.stack_start && n <= self.memory.len())
                    .ok_or_else(|| Error::new(ins.offset, "KCS 局部帧越界"))?;
                    if top < *self.frame_floors.last().unwrap() {
                        return Err(Error::new(ins.offset, "释放超过当前帧"));
                    }
                    self.frame_top = top;
                    self.sp = top;
                }
                0x33 => self.switch = self.pop()?,
                0x34 => {
                    let value = self.value(a[0])?;
                    let target = self.value(a[1])?;
                    if value == self.switch {
                        self.jump(target)?;
                    }
                }
                0x35 => {
                    let left = self.value(a[0])?;
                    let right = self.value(a[1])?;
                    let target = self.value(a[2])?;
                    if (left.min(right)..=left.max(right)).contains(&self.switch) {
                        self.jump(target)?;
                    }
                }
                0x36..=0x39 => {
                    let n = self.value(a[0])?;
                    self.push(numeric::convert(ins.opcode, n))?;
                }
                0x3a => {
                    let n = self.pop()?;
                    self.push(n)?;
                    self.push(n)?;
                }
                0x3c => self.sp = self.frame_top,
                0x3d => {
                    let n = self.value(a[0])?;
                    self.push(n.wrapping_add(self.frame_base as u32))?;
                }
                0x3e => {
                    let mut n = self.raw(a[0])?;
                    if a[0].flags & 0x10 != 0 {
                        n = u32::from_le_bytes(self.read(n, 4)?.try_into().unwrap());
                    }
                    let displacement = self.value(a[1])?;
                    self.push(n.wrapping_add(displacement))?;
                }
                0x41 => {
                    let line = self.value(a[0])?;
                    let source = self.value(a[1])?;
                    self.debug.source = self.read_string(source)?.to_vec();
                    self.debug.line = line;
                }
                0x42 => {
                    let address = self.value(a[0])?;
                    self.read(address, 0)?;
                    if self.debug.scopes.len() >= read::MAX_ITEMS {
                        return Err(Error::new(ins.offset, "KCS 调试作用域超过上限"));
                    }
                    self.debug.scopes.push(address);
                }
                0x43 => {
                    let first = self.value(a[0])?;
                    let count = self.value(a[1])?;
                    if count as usize > read::MAX_ITEMS - self.debug.variables.len() {
                        return Err(Error::new(ins.offset, "KCS 调试变量超过上限"));
                    }
                    self.debug
                        .variables
                        .extend((0..count).map(|i| first.wrapping_add(i)));
                }
                0x44 => {
                    let count = self.value(a[0])? as usize;
                    let len = self
                        .debug
                        .variables
                        .len()
                        .checked_sub(count)
                        .ok_or_else(|| Error::new(ins.offset, "KCS 调试变量下溢"))?;
                    self.debug.variables.truncate(len);
                }
                _ => {
                    return Err(Error::new(
                        ins.offset,
                        format!("KCS 指令 {:02x} 的执行契约尚未实现", ins.opcode),
                    ));
                }
            }
        }
        Ok(Step::Yield)
    }
}

fn width(flags: u8) -> Result<usize> {
    if flags & 0x80 != 0 {
        return Ok(4);
    }
    match flags & 15 {
        1 => Ok(1),
        2 => Ok(2),
        3 | 4 => Ok(4),
        _ => Err(Error::new(0, "未支持的 KCS 存储类型")),
    }
}
