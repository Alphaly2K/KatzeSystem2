use crate::{Error, Result, read};

/// 新声音配置中的原始编码名称、编号、当前值与有序子分类。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundPart {
    /// 定义 XML 中的节点键名；与可继承的分类名称分别保存。
    pub xml_key: Vec<u8>,
    pub name: Vec<u8>,
    pub type_name: Vec<u8>,
    pub file_prefix: Vec<u8>,
    /// 配置的 `<int>` 归档类别名；前缀命中后决定资源目录。
    pub archive: Vec<u8>,
    pub id: i32,
    /// 配置的 sameid 原值；子分类不沿用父级值。
    pub same_id: i32,
    /// 配置的 mastervol 倍率；与应用设置后的当前音量分别保存。
    pub master_volume: i32,
    pub volume: i32,
    pub voice_volume: i32,
    pub skip_volume: i32,
    pub system_voice: i32,
    pub play_enabled: i32,
    pub mute: i32,
    pub voice_mute: i32,
    pub children: Vec<SoundPart>,
}

impl Default for SoundPart {
    fn default() -> Self {
        Self {
            xml_key: Vec::new(),
            name: Vec::new(),
            type_name: Vec::new(),
            file_prefix: Vec::new(),
            archive: Vec::new(),
            id: -1,
            same_id: 0,
            master_volume: -1,
            volume: -1,
            voice_volume: -1,
            skip_volume: -1,
            system_voice: -1,
            play_enabled: -1,
            mute: 0,
            voice_mute: 0,
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
    pub master_play_enabled: Option<i32>,
    pub master_system_voice: Option<i32>,
    pub master_voice_volume: Option<i32>,
    pub master_skip_volume: Option<i32>,
    pub master_mute: i32,
    pub master_voice_mute: i32,
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

    /// 查询显式编号路径的播放开关；空路径及缺失分量使用一。
    pub fn play_enabled(&self, path: &[i32]) -> Result<i32> {
        self.path_value(path, 1, |part| part.play_enabled)
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
        self.group_volume(name)
    }

    /// GROUP 按分类名称查当前音量；master 在此处也是普通分类名称。
    pub fn group_volume(&self, name: &[u8]) -> Result<i32> {
        Ok(self.group_part(name)?.map_or(100, |part| part.volume))
    }

    /// 分类名称查询的语音播放音量；未匹配时返回一百，master 也按普通名称查询。
    pub fn group_voice_volume(&self, name: &[u8]) -> Result<i32> {
        Ok(self.group_part(name)?.map_or(100, |part| part.voice_volume))
    }

    pub fn group_part(&self, name: &[u8]) -> Result<Option<&SoundPart>> {
        self.find_part(name, Match::Name)
    }

    pub fn type_part(&self, name: &[u8]) -> Result<Option<&SoundPart>> {
        self.find_part(name, Match::Type)
    }

    /// 按非空文件前缀查首个分类；父级匹配时不再查找后代。
    pub fn file_part(&self, name: &[u8]) -> Result<Option<&SoundPart>> {
        self.find_part(name, Match::Prefix)
    }

    pub fn file_volume(&self, name: &[u8]) -> Result<i32> {
        Ok(self.file_part(name)?.map_or(100, |part| part.volume))
    }

    pub fn file_play_enabled(&self, name: &[u8]) -> Result<i32> {
        Ok(self.file_part(name)?.map_or(1, |part| part.play_enabled))
    }

    pub fn file_system_voice(&self, name: &[u8]) -> Result<i32> {
        Ok(self.file_part(name)?.map_or(1, |part| part.system_voice))
    }

    /// 未匹配时返回一百；匹配节点的 −1 静音值转换为零。
    pub fn file_mute(&self, name: &[u8]) -> Result<i32> {
        Ok(self
            .file_part(name)?
            .map_or(100, |part| if part.mute == -1 { 0 } else { part.mute }))
    }

    pub fn master_mute(&self) -> i32 {
        if self.master_mute == -1 {
            0
        } else {
            self.master_mute
        }
    }

    /// 按文件前缀命中分支，自内向外应用分类音量与倍率，再应用独立主音量。
    pub fn effective_file_volume(&self, name: &[u8]) -> Result<i32> {
        self.effective_volume(name, false)
    }

    /// 按文件前缀应用语音期间的分类音量、两种静音开关及主音量。
    pub fn effective_file_voice_volume(&self, name: &[u8]) -> Result<i32> {
        self.effective_volume(name, true)
    }

    fn effective_volume(&self, name: &[u8], voice: bool) -> Result<i32> {
        if self.master_mute() != 0 {
            return Ok(0);
        }
        let mut volume = 100i32;
        for part in self.find_path(name, Match::Prefix)?.iter().rev() {
            volume = if part.mute == 0 && (!voice || part.voice_mute == 0) {
                let value = if voice {
                    part.voice_volume
                } else {
                    part.volume
                };
                value.wrapping_mul(volume) / 100
            } else {
                0
            };
            volume = part.master_volume.wrapping_mul(volume) / 100;
        }
        let master = self
            .master_volume
            .ok_or_else(|| Error::new(0, "运行时未提供新声音配置的当前主音量"))?;
        Ok(master.wrapping_mul(volume) / 100)
    }

    fn find_part(&self, name: &[u8], mode: Match) -> Result<Option<&SoundPart>> {
        Ok(self.find_path(name, mode)?.last().copied())
    }

    fn find_path(&self, name: &[u8], mode: Match) -> Result<Vec<&SoundPart>> {
        let name = &name[..name
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(name.len())];
        if name.len() >= 512 {
            return Err(Error::new(0, "声音分类名称超过查询缓冲区容量"));
        }
        let name = normalized_name(name);
        let mut stack: Vec<_> = self.parts.iter().rev().map(|part| (part, 0)).collect();
        let mut path = Vec::new();
        let mut visited = 0;
        while let Some((part, depth)) = stack.pop() {
            visited += 1;
            if visited > read::MAX_ITEMS {
                return Err(Error::new(0, "声音配置节点数量超过上限"));
            }
            path.truncate(depth);
            path.push(part);
            let key = normalized_name(match mode {
                Match::Name => &part.name,
                Match::Type => &part.type_name,
                Match::Prefix => &part.file_prefix,
            });
            if if matches!(mode, Match::Prefix) {
                !key.is_empty() && name.starts_with(&key)
            } else {
                name == key
            } {
                return Ok(path);
            }
            stack.extend(part.children.iter().rev().map(|part| (part, depth + 1)));
        }
        Ok(Vec::new())
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

#[derive(Clone, Copy)]
enum Match {
    Name,
    Type,
    Prefix,
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
