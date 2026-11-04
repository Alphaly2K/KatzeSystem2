use crate::{Error, Result, read, resource::XmlDocument};
use std::collections::VecDeque;

mod lines;

/// 回看日志的构造配置；姓名前后缀保留启动 XML 的原始 UTF-8 字节。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub columns: u32,
    pub profile: u32,
    pub name_line: u32,
    pub name_prefix: Vec<u8>,
    pub name_suffix: Vec<u8>,
    pub indent: u32,
    pub name_indent: u32,
    pub display_message: u32,
    pub display_name: u32,
    pub space: u32,
    pub escape: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            columns: 40,
            profile: 4,
            name_line: 1,
            name_prefix: Vec::new(),
            name_suffix: Vec::new(),
            indent: 1,
            name_indent: 0,
            display_message: 1,
            display_name: 1,
            space: 0,
            escape: 1,
        }
    }
}

impl Settings {
    pub(crate) fn apply(&mut self, xml: &XmlDocument) -> Result<()> {
        for (name, value) in [
            ("col", &mut self.columns),
            ("prof", &mut self.profile),
            ("nameline", &mut self.name_line),
            ("indent", &mut self.indent),
            ("nameindent", &mut self.name_indent),
            ("disp/mes", &mut self.display_message),
            ("disp/name", &mut self.display_name),
            ("space", &mut self.space),
            ("escape", &mut self.escape),
        ] {
            *value = xml.integer(format!("GAME/log/{name}").as_bytes(), *value)?;
        }
        for (name, value) in [
            ("first", &mut self.name_prefix),
            ("last", &mut self.name_suffix),
        ] {
            if let Some(bytes) = xml.text(format!("GAME/log/nameformat/{name}").as_bytes())? {
                if bytes.len() >= 32 {
                    return Err(Error::new(0, "回看姓名格式超过配置容量"));
                }
                *value = bytes.to_vec();
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Group {
    words: [u32; 7],
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    group: u32,
    text: Vec<u8>,
}

/// 独立回看记录；来源、段、显示行和姓名分别保存。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backlog {
    settings: Settings,
    files: Vec<Vec<u8>>,
    groups: VecDeque<Group>,
    rows: VecDeque<Row>,
    names: Vec<Vec<u8>>,
    limit: u32,
    first_group: u32,
    first_row: u32,
}

/// 一次追加的来源位置、姓名、正文及原始标记。
#[derive(Clone, Copy, Debug)]
pub struct Record<'a> {
    pub file: &'a [u8],
    pub input: u32,
    pub name: &'a [u8],
    pub text: &'a [u8],
    pub flag: u32,
    pub space: u32,
    pub extra: u32,
}

impl Backlog {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            files: Vec::new(),
            groups: VecDeque::new(),
            rows: VecDeque::new(),
            names: Vec::new(),
            limit: 100,
            first_group: 0,
            first_row: 0,
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn limit(&self) -> u32 {
        self.limit
    }

    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn first_group(&self) -> u32 {
        self.first_group
    }

    pub fn first_row(&self) -> u32 {
        self.first_row
    }

    /// 清空四张表，保留构造配置、数量限制和累计编号起点。
    pub fn clear(&mut self) {
        self.files.clear();
        self.groups.clear();
        self.rows.clear();
        self.names.clear();
    }

    /// 相邻同来源、同输入段的记录不重复追加。空记录仍占一个段。
    pub fn push(&mut self, record: Record<'_>) -> Result<bool> {
        let source = c_string(record.file);
        let file = match self.files.iter().rposition(|value| value == source) {
            Some(index) => index as u32,
            None => register(&mut self.files, source)?,
        };
        if self
            .groups
            .back()
            .is_some_and(|last| last.words[1] == file && last.words[2] == record.input)
        {
            return Ok(false);
        }
        if self.groups.len() >= read::MAX_ITEMS {
            return Err(Error::new(0, "回看段数超过上限"));
        }
        let group = self.first_group.wrapping_add(self.groups.len() as u32);
        let name = lines::preprocess(c_string(record.name), self.settings.escape != 0);
        let text = lines::preprocess(c_string(record.text), self.settings.escape != 0);
        let mut words = [0, file, record.input, 0, 0, u32::MAX, record.extra];
        if record.flag != 0 {
            words[0] |= 4;
        }
        let mut formatted_name = Vec::new();
        if !name.is_empty() {
            formatted_name.extend_from_slice(c_string(&self.settings.name_prefix));
            formatted_name.extend_from_slice(&name);
            formatted_name.extend_from_slice(c_string(&self.settings.name_suffix));
            words[0] |= 2;
            words[5] = register(&mut self.names, &formatted_name)?;
        }
        let mut pending = Vec::new();
        if self.settings.space != 0 && record.space != 0 && !self.rows.is_empty() {
            pending.push(Row {
                group: u32::MAX,
                text: Vec::new(),
            });
        }
        let mut name_row = None;
        if !name.is_empty() && self.settings.name_line != 0 {
            name_row = Some(
                self.first_row
                    .wrapping_add((self.rows.len() + pending.len()) as u32),
            );
            pending.push(Row {
                group,
                text: formatted_name.clone(),
            });
        }
        if !text.is_empty() {
            words[0] |= 1;
            let start = self
                .first_row
                .wrapping_add((self.rows.len() + pending.len()) as u32);
            let rows = lines::split(&self.settings, &text, &formatted_name)?;
            words[3] = start;
            words[4] = start.wrapping_add(rows.len() as u32).wrapping_sub(1);
            pending.extend(rows.into_iter().map(|text| Row { group, text }));
        }
        if let Some(row) = name_row {
            words[3] = row;
        }
        if self.rows.len().saturating_add(pending.len()) > read::MAX_ITEMS {
            return Err(Error::new(0, "回看行数超过上限"));
        }
        self.rows.extend(pending);
        self.groups.push_back(Group { words });
        self.trim();
        Ok(true)
    }

    pub fn set_limit(&mut self, limit: u32) {
        self.limit = limit;
        self.trim();
    }

    pub fn row(&self, index: u32) -> Option<&[u8]> {
        self.rows.get(index as usize).map(|row| row.text.as_slice())
    }

    /// 行编号以当前保留区为起点；空行和无姓名记录返回 None。
    pub fn name_for_row(&self, index: u32) -> Option<&[u8]> {
        let group = self
            .rows
            .get(index as usize)?
            .group
            .wrapping_sub(self.first_group);
        self.names
            .get(self.groups.get(group as usize)?.words[5] as usize)
            .map(Vec::as_slice)
    }

    /// 装载四张表并应用当前保留数量；配置由原对象保留。
    pub fn restore(&mut self, data: &[u8]) -> Result<()> {
        if data.len() > read::MAX_BYTES {
            return Err(Error::new(0, "回看存档超过读取上限"));
        }
        let mut next = Self::new(self.settings.clone());
        next.limit = self.limit;
        next.first_group = read::u32_at(data, 16)?;
        next.first_row = read::u32_at(data, 20)?;
        for part in 0..4 {
            let mut at = read::u32_at(data, part * 4)? as usize;
            let count = take_word(data, &mut at)? as usize;
            if count > read::MAX_ITEMS {
                return Err(Error::new(at, "回看存档表数量超过上限"));
            }
            for _ in 0..count {
                match part {
                    0 => next.files.push(take_string(data, &mut at)?),
                    1 => {
                        let mut words = [0; 7];
                        for word in &mut words {
                            *word = take_word(data, &mut at)?;
                        }
                        next.groups.push_back(Group { words });
                    }
                    2 => {
                        let group = take_word(data, &mut at)?;
                        let text = take_string(data, &mut at)?;
                        next.rows.push_back(Row { group, text });
                    }
                    3 => next.names.push(take_string(data, &mut at)?),
                    _ => unreachable!(),
                }
            }
        }
        next.trim();
        *self = next;
        Ok(())
    }

    fn trim(&mut self) {
        while self.groups.len() > self.limit as usize {
            let group = self.groups.pop_front().unwrap();
            // 空段也保留原行端点；累计编号随裁剪推进。
            let start = group.words[3] as i32;
            let end = group.words[4] as i32;
            if start <= end && end >= self.first_row as i32 {
                let count = (end as i64 - self.first_row as i32 as i64 + 1) as u64;
                self.rows.drain(..self.rows.len().min(count as usize));
                self.first_row = self.first_row.wrapping_add(count as u32);
            }
            while self.rows.front().is_some_and(|row| row.group == u32::MAX) {
                self.rows.pop_front();
                self.first_row = self.first_row.wrapping_add(1);
            }
            self.first_group = self.first_group.wrapping_add(1);
        }
    }

    /// 存档字节不包含构造配置和保留数量，四张表通过相对偏移寻址。
    pub fn serialize(&self) -> Result<Vec<u8>> {
        let mut out = vec![0; 24];
        out[16..20].copy_from_slice(&self.first_group.to_le_bytes());
        out[20..24].copy_from_slice(&self.first_row.to_le_bytes());
        for (index, count) in [
            self.files.len(),
            self.groups.len(),
            self.rows.len(),
            self.names.len(),
        ]
        .into_iter()
        .enumerate()
        {
            let offset = out.len() as u32;
            out[index * 4..index * 4 + 4].copy_from_slice(&offset.to_le_bytes());
            append(&mut out, &(count as u32).to_le_bytes())?;
            match index {
                0 | 3 => {
                    let values = if index == 0 { &self.files } else { &self.names };
                    for value in values {
                        append(&mut out, value)?;
                        append(&mut out, &[0])?;
                    }
                }
                1 => {
                    for group in &self.groups {
                        for word in group.words {
                            append(&mut out, &word.to_le_bytes())?;
                        }
                    }
                }
                2 => {
                    for row in &self.rows {
                        append(&mut out, &row.group.to_le_bytes())?;
                        append(&mut out, &row.text)?;
                        append(&mut out, &[0])?;
                    }
                }
                _ => unreachable!(),
            }
        }
        Ok(out)
    }
}

fn append(out: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    if bytes.len() > read::MAX_BYTES - out.len() {
        return Err(Error::new(0, "回看日志序列化超过上限"));
    }
    out.extend_from_slice(bytes);
    Ok(())
}

fn c_string(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
}

fn register(table: &mut Vec<Vec<u8>>, value: &[u8]) -> Result<u32> {
    if let Some(index) = table.iter().position(|v| v == value) {
        return Ok(index as u32);
    }
    if table.len() >= read::MAX_ITEMS {
        return Err(Error::new(0, "回看字串表数量超过上限"));
    }
    table.push(value.to_vec());
    Ok(table.len() as u32 - 1)
}

fn take_word(data: &[u8], at: &mut usize) -> Result<u32> {
    let value = read::u32_at(data, *at)?;
    *at += 4;
    Ok(value)
}

fn take_string(data: &[u8], at: &mut usize) -> Result<Vec<u8>> {
    let remaining = data
        .get(*at..)
        .ok_or_else(|| Error::new(*at, "回看字串偏移越界"))?;
    let length = remaining
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| Error::new(*at, "回看字串缺少结束符"))?;
    *at += length + 1;
    Ok(remaining[..length].to_vec())
}
