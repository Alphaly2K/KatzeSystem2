use super::{Image, Images};
use crate::Result;
use crate::host::resources::ResourceServices;
use crate::resource::ResourceMode;
use std::collections::VecDeque;

struct Entry {
    name: Vec<u16>,
    frame: i32,
    image: Image,
}

pub(super) struct DecodedCache {
    entries: VecDeque<Entry>,
    bytes: usize,
    limit: usize,
    automatic: bool,
}

impl Default for DecodedCache {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
            limit: 32 << 20,
            automatic: true,
        }
    }
}

impl Images {
    /// 设置解码图像缓存的字节预算；零禁用缓存，不影响已有图像句柄。
    pub fn set_cache_limit(&mut self, bytes: usize) {
        if self.cache.limit != bytes {
            self.cache.limit = bytes;
            self.cache.trim(0);
        }
    }

    pub(crate) fn set_automatic_cache(&mut self, enabled: bool) {
        self.cache.automatic = enabled;
    }

    pub(crate) fn decode<S: ResourceServices + ?Sized>(
        &mut self,
        name: &[u8],
        frame: Option<i32>,
        preload: bool,
        services: &mut S,
    ) -> Result<Option<Image>> {
        let key = match frame {
            Some(_) if self.cache.limit != 0 => Some(
                crate::text::cp932(name)?
                    .to_uppercase()
                    .encode_utf16()
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        };
        let retain = preload || self.cache.automatic;
        if let Some(key) = &key {
            if let Some(index) = self
                .cache
                .entries
                .iter()
                .position(|entry| entry.name == *key && Some(entry.frame) == frame)
            {
                let image = self.cache.entries[index].image.clone();
                if retain {
                    let entry = self.cache.entries.remove(index).unwrap();
                    self.cache.entries.push_front(entry);
                }
                return Ok(Some(image));
            }
        }
        let Some(source) = services.open(name, ResourceMode::LooseFirst)? else {
            return Ok(None);
        };
        let image = Image::decode_source(source.as_ref(), frame)?;
        if retain {
            if let (Some(name), Some(image)) = (key, &image) {
                let bytes = image.surface().pixels().len();
                self.cache.trim(bytes);
                self.cache.bytes += bytes;
                self.cache.entries.push_front(Entry {
                    name,
                    frame: frame.unwrap(),
                    image: image.clone(),
                });
            }
        }
        Ok(image)
    }
}

impl DecodedCache {
    fn trim(&mut self, incoming: usize) {
        while self.bytes.saturating_add(incoming) >= self.limit {
            let Some(entry) = self.entries.pop_back() else {
                break;
            };
            self.bytes -= entry.image.surface().pixels().len();
        }
    }
}
