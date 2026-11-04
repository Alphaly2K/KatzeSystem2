use crate::{Error, Result, read};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub name: String,
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct Table {
    pub languages: Vec<String>,
    pub messages: Vec<Vec<Message>>,
}

impl Table {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let (table, used) = Self::parse_prefix(data)?;
        if used != data.len() {
            return Err(Error::new(used, "CSTL 存在尾部数据"));
        }
        Ok(table)
    }

    pub(crate) fn parse_prefix(data: &[u8]) -> Result<(Self, usize)> {
        if data.len() > read::MAX_BYTES || data.get(..4) != Some(b"CSTL") {
            return Err(Error::new(0, "无效 CSTL 容器"));
        }
        if read::u32_at(data, 4)? != 0 {
            return Err(Error::new(4, "未支持的 CSTL 头字段"));
        }
        let mut p = 8;
        let languages_count = integer(data, &mut p)?;
        if languages_count == 0
            || languages_count > 4096
            || languages_count > data.len().saturating_sub(p)
        {
            return Err(Error::new(p, "无效语言数量"));
        }
        let mut languages = Vec::new();
        for _ in 0..languages_count {
            let language = string(data, &mut p)?;
            if languages.contains(&language) {
                return Err(Error::new(p, "重复的语言名称"));
            }
            languages.push(language);
        }
        let count = integer(data, &mut p)?;
        if count > data.len().saturating_sub(p) / languages_count / 2
            || count > read::MAX_ITEMS / languages_count
        {
            return Err(Error::new(p, "消息数量超过可用数据或读取上限"));
        }
        let mut messages = Vec::new();
        for _ in 0..count {
            let mut row = Vec::new();
            for _ in 0..languages_count {
                row.push(Message {
                    name: string(data, &mut p)?,
                    content: string(data, &mut p)?,
                });
            }
            messages.push(row);
        }
        Ok((
            Self {
                languages,
                messages,
            },
            p,
        ))
    }

    pub fn message(&self, language: &str, index: usize) -> Option<&Message> {
        let lang = self.languages.iter().position(|s| s == language)?;
        self.messages.get(index)?.get(lang)
    }

    pub fn segment(&self, language: &str, index: usize, segment: i32) -> Option<&str> {
        let content = &self.message(language, index)?.content;
        Some(if segment < 0 {
            content
        } else {
            content.split('\x1f').nth(segment as usize).unwrap_or("")
        })
    }

    pub fn option(&self, language: &str, input_count: usize, index: usize) -> Option<String> {
        let index = input_count.checked_sub(1)?.checked_add(index)?;
        let message = self.message(language, index)?;
        Some(
            message
                .content
                .split('\x1f')
                .take_while(|s| !s.is_empty())
                .collect(),
        )
    }
}

fn integer(data: &[u8], p: &mut usize) -> Result<usize> {
    let mut n = 0usize;
    loop {
        let b = *data
            .get(*p)
            .ok_or_else(|| Error::new(*p, "未结束的 FF 整数"))?;
        *p += 1;
        n = n
            .checked_add(b as usize)
            .filter(|&x| x <= read::MAX_BYTES)
            .ok_or_else(|| Error::new(*p, "FF 整数超过上限"))?;
        if b != 255 {
            return Ok(n);
        }
    }
}

fn string(data: &[u8], p: &mut usize) -> Result<String> {
    let n = integer(data, p)?;
    let bytes = read::range(data, *p, n)?;
    let s = std::str::from_utf8(bytes)
        .map_err(|e| Error::new(*p + e.valid_up_to(), "无效 UTF-8 文本"))?
        .to_owned();
    *p += n;
    Ok(s)
}
