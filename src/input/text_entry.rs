use std::sync::Arc;

use crate::{Error, Result, read, text};

pub(crate) const CAPACITY: usize = 256;

pub(super) struct TextEntry {
    bytes: Vec<u8>,
    request: Option<Arc<str>>,
    pub(super) status: u32,
}

impl Default for TextEntry {
    fn default() -> Self {
        Self {
            bytes: Vec::new(),
            request: None,
            status: 2,
        }
    }
}

impl TextEntry {
    pub(super) fn begin(&mut self, input: &[u8]) -> Result<()> {
        let bytes = read::zstr(input, 0)?;
        if bytes.len() >= CAPACITY {
            return Err(Error::new(0, "输入对话框初始文字超出缓冲区"));
        }
        self.bytes.clear();
        self.bytes.extend_from_slice(&input[..=bytes.len()]);
        self.request = Some(Arc::from(text::cp932_replacing(bytes)));
        self.status = 0;
        Ok(())
    }

    pub(super) fn request(&self) -> Option<Arc<str>> {
        self.request.clone()
    }

    pub(super) fn complete(&mut self, request: &Arc<str>, text: Option<&str>) -> Result<bool> {
        if !self
            .request
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, request))
        {
            return Ok(false);
        }
        if let Some(text) = text {
            let wide: Vec<_> = text.encode_utf16().take(CAPACITY).collect();
            if wide.len() >= CAPACITY || wide.contains(&0) {
                return Err(Error::new(0, "输入对话框返回的文字无效或过长"));
            }
            let mut bytes = text::cp932_encoded_replacing(&wide);
            if bytes.len() >= CAPACITY {
                return Err(Error::new(0, "输入对话框返回的文字超出缓冲区"));
            }
            bytes.push(0);
            self.bytes = bytes;
            self.status = 1;
        } else {
            self.status = 2;
        }
        self.request = None;
        Ok(true)
    }

    pub(super) fn bytes(&self) -> &[u8] {
        if self.bytes.is_empty() {
            &[0]
        } else {
            &self.bytes
        }
    }
}
