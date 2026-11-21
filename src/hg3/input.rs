use crate::{Error, Result, host::storage::ReadSource, read};
use std::borrow::Cow;

#[derive(Clone, Copy)]
pub(super) enum Input<'a> {
    Bytes(&'a [u8]),
    Source(&'a dyn ReadSource, usize),
}

impl<'a> Input<'a> {
    pub fn source(source: &'a dyn ReadSource) -> Result<Self> {
        let length = source.len()?;
        if length > read::MAX_BYTES as u64 {
            return Err(Error::new(0, "无效 HG-3 容器"));
        }
        Ok(Self::Source(source, length as usize))
    }

    pub fn len(self) -> usize {
        match self {
            Self::Bytes(bytes) => bytes.len(),
            Self::Source(_, length) => length,
        }
    }

    pub fn bounded(self, end: usize) -> Self {
        match self {
            Self::Bytes(bytes) => Self::Bytes(&bytes[..end]),
            Self::Source(source, _) => Self::Source(source, end),
        }
    }

    pub fn check(self, start: usize, length: usize) -> Result<()> {
        if start.checked_add(length).is_none_or(|end| end > self.len()) {
            return Err(Error::new(start, "输入范围越界"));
        }
        Ok(())
    }

    pub fn array<const N: usize>(self, start: usize) -> Result<[u8; N]> {
        self.check(start, N)?;
        match self {
            Self::Bytes(bytes) => Ok(bytes[start..start + N].try_into().unwrap()),
            Self::Source(source, _) => {
                let mut bytes = [0; N];
                source.read_exact_at(start as u64, &mut bytes)?;
                Ok(bytes)
            }
        }
    }

    pub fn u32_at(self, start: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array(start)?))
    }

    pub fn bytes(self, start: usize, length: usize) -> Result<Cow<'a, [u8]>> {
        self.check(start, length)?;
        match self {
            Self::Bytes(bytes) => Ok(Cow::Borrowed(&bytes[start..start + length])),
            Self::Source(source, _) => {
                let mut bytes = vec![0; length];
                source.read_exact_at(start as u64, &mut bytes)?;
                Ok(Cow::Owned(bytes))
            }
        }
    }
}
