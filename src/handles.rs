use crate::{Error, Result};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

/// 脚本不透明句柄的共享编号源；克隆共享进度，零保留为空，已分配编号不复用。
#[derive(Clone, Debug)]
pub struct HandleAllocator(Arc<AtomicU32>);

impl Default for HandleAllocator {
    fn default() -> Self {
        Self(Arc::new(AtomicU32::new(1)))
    }
}

impl HandleAllocator {
    /// 分配一个非零编号；各存储仍负责类型检查与对象生命周期。
    pub fn allocate(&self) -> Result<u32> {
        self.allocate_at(0)
    }

    pub(crate) fn allocate_at(&self, offset: usize) -> Result<u32> {
        self.0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .map_err(|_| Error::new(offset, "脚本对象句柄耗尽"))
    }

    pub(crate) fn allocate_buffer_at(&self, length: usize, offset: usize) -> Result<u32> {
        let length =
            u32::try_from(length).map_err(|_| Error::new(offset, "原始缓冲区地址空间耗尽"))?;
        // 从普通 VM 偏移范围之外起分配，为块尾零长度视图保留一个地址。
        let start = |next: u32| {
            next.max(0x4000_0000)
                .checked_add(7)
                .map(|n| n & !7)?
                .checked_add(4)
        };
        self.0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                start(next)?.checked_add(length.max(1))?.checked_add(1)
            })
            .map(|next| start(next).unwrap())
            .map_err(|_| Error::new(offset, "原始缓冲区地址空间耗尽"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_allocator_stops_at_exhaustion_without_reusing_handles() {
        let handles = HandleAllocator(Arc::new(AtomicU32::new(u32::MAX - 2)));
        let other = handles.clone();
        assert_eq!(handles.allocate().unwrap(), u32::MAX - 2);
        assert_eq!(other.allocate().unwrap(), u32::MAX - 1);
        assert_eq!(handles.allocate_at(37).unwrap_err().offset, 37);
        assert!(other.allocate().is_err());
        let handles = HandleAllocator::default();
        let object = handles.allocate().unwrap();
        let buffer = handles.allocate_buffer_at(9, 0).unwrap();
        let next = handles.allocate().unwrap();
        assert!(buffer > object && buffer.is_multiple_of(4));
        assert!(next > buffer + 9);
        assert!(handles.allocate_buffer_at(usize::MAX, 0).is_err());
    }
}
