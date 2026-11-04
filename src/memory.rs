use crate::{Error, Result, read};
use std::cell::{Ref, RefCell};
use std::ops::Deref;
use std::rc::Rc;

/// 单线程共享字节区的受检视图；切片与克隆保留同一份存储。
#[derive(Clone, Default)]
pub struct SharedMemory {
    bytes: Rc<RefCell<Vec<u8>>>,
    offset: usize,
    length: usize,
}

impl SharedMemory {
    pub fn new(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() > read::MAX_BYTES {
            return Err(Error::new(0, "共享存储超过上限"));
        }
        let length = bytes.len();
        Ok(Self {
            bytes: Rc::new(RefCell::new(bytes)),
            offset: 0,
            length,
        })
    }

    pub fn len(&self) -> usize {
        self.length
    }

    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub fn slice(&self, offset: usize, length: usize) -> Result<Self> {
        self.range(offset, length)?;
        Ok(Self {
            bytes: self.bytes.clone(),
            offset: self.offset + offset,
            length,
        })
    }

    fn range(&self, offset: usize, length: usize) -> Result<std::ops::Range<usize>> {
        let end = offset
            .checked_add(length)
            .filter(|&end| end <= self.length)
            .ok_or_else(|| Error::new(offset, "共享存储访问越界"))?;
        Ok(self.offset + offset..self.offset + end)
    }

    pub fn read(&self, offset: usize, length: usize) -> Result<Ref<'_, [u8]>> {
        let range = self.range(offset, length)?;
        let bytes = self
            .bytes
            .try_borrow()
            .map_err(|_| Error::new(offset, "共享存储正在写入"))?;
        Ok(Ref::map(bytes, |bytes| &bytes[range]))
    }

    pub fn write(&self, offset: usize, value: &[u8]) -> Result<()> {
        let range = self.range(offset, value.len())?;
        let mut bytes = self
            .bytes
            .try_borrow_mut()
            .map_err(|_| Error::new(offset, "共享存储仍有活动读取视图"))?;
        bytes[range].copy_from_slice(value);
        Ok(())
    }

    pub(crate) fn into_vec(self) -> Result<Vec<u8>> {
        match Rc::try_unwrap(self.bytes) {
            Ok(bytes) => {
                let mut bytes = bytes.into_inner();
                if self.offset == 0 && self.length == bytes.len() {
                    Ok(bytes)
                } else {
                    Ok(bytes
                        .drain(self.offset..self.offset + self.length)
                        .collect())
                }
            }
            Err(bytes) => Ok(bytes
                .try_borrow()
                .map_err(|_| Error::new(0, "共享存储正在写入"))?
                [self.offset..self.offset + self.length]
                .to_vec()),
        }
    }
}

/// VM 读取视图；共享区的读取在视图释放前保持只读借用。
pub enum MemoryRead<'a> {
    Local(&'a [u8]),
    Shared(Ref<'a, [u8]>),
}

impl Deref for MemoryRead<'_> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            Self::Local(bytes) => bytes,
            Self::Shared(bytes) => bytes,
        }
    }
}

impl AsRef<[u8]> for MemoryRead<'_> {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

impl std::fmt::Debug for MemoryRead<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.deref().fmt(f)
    }
}

impl<T: AsRef<[u8]> + ?Sized> PartialEq<T> for MemoryRead<'_> {
    fn eq(&self, other: &T) -> bool {
        self.deref() == other.as_ref()
    }
}

impl Eq for MemoryRead<'_> {}

impl<const N: usize> TryInto<[u8; N]> for MemoryRead<'_> {
    type Error = std::array::TryFromSliceError;

    fn try_into(self) -> std::result::Result<[u8; N], Self::Error> {
        self.deref().try_into()
    }
}
