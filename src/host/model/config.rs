use crate::{Error, Result, read};

/// Character and motion records from a model configuration table.
/// Fields retain the original byte encoding and table order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelConfig<'a> {
    pub bodies: Vec<ModelBody<'a>>,
    pub motions: Vec<ModelMotion<'a>>,
}

/// One `#charaname` record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelBody<'a> {
    /// The selector may be a number or a named alias.
    pub key: &'a [u8],
    pub name: &'a [u8],
    /// Name to resolve in the `m2psb` resource category.
    pub resource: &'a [u8],
}

/// One `#motname` record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelMotion<'a> {
    pub body: &'a [u8],
    pub key: &'a [u8],
    pub label: &'a [u8],
    names: &'a [u8],
}

impl<'a> ModelMotion<'a> {
    /// Motion names in column order. Empty fields retain their positions.
    pub fn names(&self) -> impl Iterator<Item = &'a [u8]> {
        self.names.split(|&byte| byte == b'\t')
    }
}

#[derive(Clone, Copy)]
enum Section {
    Other,
    Body,
    Motion,
}

impl<'a> ModelConfig<'a> {
    /// Read tab-delimited records with LF or CRLF line endings. Text after a
    /// NUL terminator and records in unknown sections are ignored.
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let bytes = bytes.split(|&byte| byte == 0).next().unwrap_or_default();
        if bytes.len() > read::MAX_BYTES {
            return Err(Error::new(0, "模型配置超过读取上限"));
        }
        let mut config = Self::default();
        let mut section = Section::Other;
        let mut offset = 0;
        for line in bytes.split_inclusive(|&byte| byte == b'\n') {
            config.read_line(line, offset, &mut section)?;
            if config.bodies.len() + config.motions.len() > read::MAX_ITEMS {
                return Err(Error::new(offset, "模型配置记录数超过上限"));
            }
            offset += line.len();
        }
        Ok(config)
    }

    fn read_line(&mut self, line: &'a [u8], offset: usize, section: &mut Section) -> Result<()> {
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let trimmed = line.trim_ascii();
        if trimmed.is_empty() || trimmed.starts_with(b"//") {
            return Ok(());
        }
        if trimmed.starts_with(b"#") {
            *section = if trimmed.eq_ignore_ascii_case(b"#charaname") {
                Section::Body
            } else if trimmed.eq_ignore_ascii_case(b"#motname") {
                Section::Motion
            } else {
                Section::Other
            };
            return Ok(());
        }
        let malformed = || Error::new(offset, "模型配置记录缺少字段");
        match section {
            Section::Body => {
                let mut fields = line.split(|&byte| byte == b'\t');
                let key = fields.next().unwrap();
                let name = fields.next().ok_or_else(malformed)?;
                let resource = fields.next().ok_or_else(malformed)?;
                if key.is_empty() {
                    return Err(Error::new(offset, "模型配置缺少主体选择键"));
                }
                self.bodies.push(ModelBody {
                    key,
                    name,
                    resource,
                });
            }
            Section::Motion => {
                let mut fields = line.splitn(4, |&byte| byte == b'\t');
                let body = fields.next().unwrap();
                let key = fields.next().ok_or_else(malformed)?;
                let label = fields.next().ok_or_else(malformed)?;
                let names = fields.next().ok_or_else(malformed)?;
                if body.is_empty() || key.is_empty() {
                    return Err(Error::new(offset, "模型配置缺少动作选择键"));
                }
                self.motions.push(ModelMotion {
                    body,
                    key,
                    label,
                    names,
                });
            }
            Section::Other => (),
        }
        Ok(())
    }

    /// Look up the first body with this selector, using ASCII case folding.
    pub fn body(&self, key: &[u8]) -> Option<ModelBody<'a>> {
        self.bodies
            .iter()
            .find(|body| body.key.eq_ignore_ascii_case(key))
            .copied()
    }

    /// Look up the first motion with this body and motion selector.
    pub fn motion(&self, body: &[u8], key: &[u8]) -> Option<ModelMotion<'a>> {
        self.motions
            .iter()
            .find(|motion| {
                motion.body.eq_ignore_ascii_case(body) && motion.key.eq_ignore_ascii_case(key)
            })
            .copied()
    }
}
