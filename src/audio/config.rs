use crate::{Error, Result, read};

/// 新声音配置中的原始编码名称、编号、当前值与有序子分类。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundPart {
    pub name: Vec<u8>,
    pub id: i32,
    pub volume: i32,
    pub system_voice: i32,
    pub children: Vec<SoundPart>,
}

impl Default for SoundPart {
    fn default() -> Self {
        Self {
            name: Vec::new(),
            id: -1,
            volume: -1,
            system_voice: -1,
            children: Vec::new(),
        }
    }
}

/// 宿主装载并应用设置后的新声音分类树。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SoundConfig {
    pub parts: Vec<SoundPart>,
    /// 应用设置后的独立主音量；未提供时对主音量查询返回诊断。
    pub master_volume: Option<i32>,
}

impl SoundConfig {
    /// 空路径列表按先父后子顺序收集开关为一的编号；显式路径保留重复候选。
    pub fn system_voice_candidates(&self, paths: &[Vec<i32>]) -> Result<Vec<i32>> {
        let mut candidates = Vec::new();
        if paths.is_empty() {
            let mut stack: Vec<_> = self.parts.iter().rev().collect();
            let mut visited = 0;
            while let Some(part) = stack.pop() {
                visited += 1;
                if visited > read::MAX_ITEMS {
                    return Err(Error::new(0, "声音配置节点数量超过上限"));
                }
                if part.system_voice == 1 {
                    candidates.push(part.id);
                }
                stack.extend(part.children.iter().rev());
            }
        } else {
            if paths.len() > read::MAX_ITEMS {
                return Err(Error::new(0, "系统语音候选数量超过上限"));
            }
            for path in paths {
                if self.system_voice_enabled(path)? != 0 {
                    candidates.push(path.last().copied().unwrap_or(-1));
                }
            }
        }
        Ok(candidates)
    }

    /// 查询显式编号路径。缺失分量使用一，后续分量仍在当前分类中继续查询。
    pub fn system_voice_enabled(&self, path: &[i32]) -> Result<i32> {
        self.path_value(path, 1, |part| part.system_voice)
    }

    /// 查询最后一个分类的当前音量；空路径及缺失分量使用一百。
    pub fn volume(&self, path: &[i32]) -> Result<i32> {
        self.path_value(path, 100, |part| part.volume)
    }

    /// 小写 master 读取显式主音量；其他名称按先父后子的顺序查首个匹配分类。
    pub fn volume_by_name(&self, name: &[u8]) -> Result<i32> {
        let name = &name[..name
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(name.len())];
        if name == b"master" {
            return self
                .master_volume
                .ok_or_else(|| Error::new(0, "运行时未提供新声音配置的当前主音量"));
        }
        if name.len() >= 512 {
            return Err(Error::new(0, "声音分类名称超过查询缓冲区容量"));
        }
        let name = normalized_name(name);
        let mut stack: Vec<_> = self.parts.iter().rev().collect();
        let mut visited = 0;
        while let Some(part) = stack.pop() {
            visited += 1;
            if visited > read::MAX_ITEMS {
                return Err(Error::new(0, "声音配置节点数量超过上限"));
            }
            if normalized_name(&part.name) == name {
                return Ok(part.volume);
            }
            stack.extend(part.children.iter().rev());
        }
        Ok(100)
    }

    fn path_value(&self, path: &[i32], default: i32, value: fn(&SoundPart) -> i32) -> Result<i32> {
        if path.len() > read::MAX_ITEMS {
            return Err(Error::new(0, "声音配置路径超过上限"));
        }
        let mut parts = self.parts.as_slice();
        let mut result = default;
        for &id in path {
            result = default;
            if let Some(part) = parts.iter().find(|part| part.id == id) {
                result = value(part);
                parts = &part.children;
            }
        }
        Ok(result)
    }
}

fn normalized_name(name: &[u8]) -> Vec<u8> {
    let mut name = name[..name
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(name.len())]
        .to_vec();
    let mut position = 0;
    while position < name.len() {
        if matches!(name[position], 0x81..=0x9f | 0xe0..=0xfc) && position + 1 < name.len() {
            position += 2;
        } else {
            name[position].make_ascii_uppercase();
            position += 1;
        }
    }
    name
}
