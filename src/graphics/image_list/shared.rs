use super::{Entry, EntryRef, ImageList, ImageListProperties, list_frame_image};
use crate::anm::ImageAnimation;
use crate::graphics::{Image, Images, Rect};
use crate::hg3::StandardInfo;
use crate::{Error, Result, read};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

#[derive(Default)]
pub(in crate::graphics) struct SharedEntries(BTreeMap<u32, ScriptEntry>);

struct ScriptEntry {
    entry: Weak<RefCell<Entry>>,
    owned: Option<(u32, EntryRef)>,
}

impl SharedEntries {
    pub(super) fn remove(&mut self, handle: u32) {
        self.0.remove(&handle);
    }
}

pub(super) fn insert_frames(
    images: &mut Images,
    owner: Option<u32>,
    decoded: impl IntoIterator<Item = (u32, Option<Image>)>,
) -> Result<(BTreeMap<u32, u32>, u32)> {
    let mut frames = BTreeMap::new();
    for (id, image) in decoded {
        let Some(image) = image else {
            frames.insert(id, 0);
            continue;
        };
        let alpha = Some(image.info().reserved != 0);
        let inserted = match owner {
            Some(owner) => images.insert(owner, image, alpha),
            None => images.insert_unowned(image, alpha),
        };
        match inserted {
            Ok(image) => {
                frames.insert(id, image);
            }
            Err(error) => {
                for &image in frames.values() {
                    images.release(image)?;
                }
                return Err(error);
            }
        }
    }
    let image = *frames
        .first_key_value()
        .ok_or_else(|| Error::new(0, "图像列表原始来源没有可用帧"))?
        .1;
    Ok((frames, image))
}

impl Images {
    pub(crate) fn create_shared_image_entry(&mut self, owner: u32, bytes: &[u8]) -> Result<u32> {
        if self.shared_entries.0.len() == read::MAX_ITEMS {
            return Err(Error::new(0, "共享图像条目超过数量上限"));
        }
        let decoded = if bytes.starts_with(b"HG-2") {
            let image = crate::hg2::decode_shared_frame(bytes)?
                .map(Image::from_frame)
                .transpose()?;
            BTreeMap::from([(0, image)])
        } else {
            ImageList::decode_frame_set(bytes, |frame, length| {
                frame
                    .decode_shared(bytes)?
                    .map(|image| list_frame_image(image, frame.metadata.offset, length))
                    .transpose()
            })?
        };
        let handle = self.handles.allocate()?;
        let (frames, image) = insert_frames(self, None, decoded)?;
        let (base, clip) = if image == 0 {
            ([0.; 2], Rect::default())
        } else {
            let info = self.get(image)?.info();
            (
                info.base.map(|v| v as f32),
                Rect::from_xywh(0, 0, info.size[0] as i32, info.size[1] as i32),
            )
        };
        let entry = EntryRef::new(Entry {
            shared: Some(handle),
            image,
            detached_image: false,
            frames,
            source_identity: 0,
            capture_revision: None,
            resource: None,
            resource_geometry: None,
            animation: ImageAnimation::default(),
            priority: 0,
            properties: ImageListProperties {
                base_point: base,
                ..Default::default()
            },
            history: None,
            clip,
        });
        self.shared_entries.0.insert(
            handle,
            ScriptEntry {
                entry: Rc::downgrade(&entry.0),
                owned: Some((owner, entry)),
            },
        );
        Ok(handle)
    }

    fn shared_image_entry(&self, handle: u32) -> Result<EntryRef> {
        self.shared_entries
            .0
            .get(&handle)
            .and_then(|record| record.entry.upgrade())
            .map(EntryRef)
            .ok_or_else(|| Error::new(0, "共享图像条目句柄无效"))
    }

    pub(crate) fn shared_image_info(&self, handle: u32) -> Result<Option<StandardInfo>> {
        let reference = self.shared_image_entry(handle)?;
        let entry = reference.read();
        let image = entry
            .frames
            .first_key_value()
            .map_or(entry.image, |(_, &image)| image);
        if image == 0 || self.pixel_handle(image)? == 0 {
            Ok(None)
        } else {
            Ok(Some(*self.get(image)?.info()))
        }
    }

    pub(crate) fn shared_image_frame(&self, handle: u32, index: i32) -> Result<u32> {
        let reference = self.shared_image_entry(handle)?;
        let entry = reference.read();
        Ok(entry
            .frames
            .values()
            .nth(index.max(0) as usize)
            .copied()
            .unwrap_or(0))
    }

    pub(crate) fn release_shared_image_entry(&mut self, handle: u32) -> Result<()> {
        let (_, entry) = self
            .shared_entries
            .0
            .get_mut(&handle)
            .and_then(|record| record.owned.take())
            .ok_or_else(|| Error::new(0, "共享图像条目没有脚本引用"))?;
        entry.release(self)
    }

    pub(crate) fn release_shared_image_owner(&mut self, owner: u32) -> Result<()> {
        let entries: Vec<_> = self
            .shared_entries
            .0
            .iter()
            .filter_map(|(&handle, record)| {
                record
                    .owned
                    .as_ref()
                    .filter(|(id, _)| *id == owner)
                    .map(|_| handle)
            })
            .collect();
        for handle in entries {
            self.release_shared_image_entry(handle)?;
        }
        Ok(())
    }
}

impl ImageList {
    pub(crate) fn set_shared(
        &mut self,
        key: u32,
        handle: u32,
        properties: ImageListProperties,
        images: &Images,
    ) -> Result<()> {
        if self.update_properties(key, properties, images)? {
            return Ok(());
        }
        let (_, clip, priority) = self.source_history(key, images)?;
        let reference = images.shared_image_entry(handle)?;
        let mut entry = reference.write()?;
        let base = entry.properties.base_point;
        entry.properties = properties;
        entry.clip = clip;
        entry.priority = priority;
        drop(entry);
        self.finish_source(key, reference, base, images)
    }
}
