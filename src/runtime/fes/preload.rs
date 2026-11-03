use super::{Objects, Scene};
use crate::kcs::ResourceServices;
use crate::resource::ResourceMode;
use crate::runtime::{ObjectFactory, Store};
use crate::{Error, Result, fes, read};
use std::{
    collections::{BTreeMap, VecDeque},
    rc::Rc,
};

pub(super) type Cache = Rc<BTreeMap<Vec<u8>, Vec<u8>>>;

pub(super) struct Preload {
    lines: VecDeque<fes::Line>,
    name: Option<Vec<u8>>,
    active: bool,
}

impl Preload {
    pub(super) fn new(program: &fes::Program) -> Self {
        Self {
            lines: program
                .declarations
                .iter()
                .filter(|declaration| {
                    declaration.header.split(u8::is_ascii_whitespace).next() == Some(b"PRELOAD")
                })
                .flat_map(|declaration| declaration.lines.iter().cloned())
                .collect(),
            name: None,
            active: false,
        }
    }

    pub(super) fn is_pending(&self) -> bool {
        self.active && !self.lines.is_empty()
    }
}

impl Objects {
    /// 查询当前 FES 缓存的完整图像名称；名称按原始字节区分大小写。
    pub fn preloaded_image(&self, name: &[u8]) -> Option<&[u8]> {
        self.preloaded_images
            .get(terminated(name))
            .map(Vec::as_slice)
    }
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn preload_fes_images(&mut self, scene: &mut Scene) -> Result<bool> {
        for _ in 0..self.budget.max(1) {
            let Some(line) = scene.preload.lines.front() else {
                return Ok(true);
            };
            scene.preload.active = true;
            let offset = line.offset;
            let result = (|| {
                let services = self
                    .resources
                    .services
                    .as_deref_mut()
                    .ok_or_else(|| Error::new(0, "运行时未配置 FES 预加载资源服务"))?;
                if scene.preload.name.is_none() {
                    let input = terminated(&line.bytes);
                    if input.len() >= 260 {
                        return Err(Error::new(0, "FES 预加载名称超过缓冲区容量"));
                    }
                    scene.preload.name = Some(services.resolve(b"image", input)?);
                }
                let name = terminated(scene.preload.name.as_ref().unwrap());
                if name.len() >= 260 {
                    return Err(Error::new(0, "FES 预加载完整名称超过缓冲区容量"));
                }
                if let Some(bytes) = services.read(name, ResourceMode::LooseFirst)? {
                    if bytes.len() > read::MAX_BYTES {
                        return Err(Error::new(0, "FES 预加载资源超过读取上限"));
                    }
                    let cache = Rc::make_mut(&mut scene.objects.preloaded_images);
                    if !cache.contains_key(name) && cache.len() >= read::MAX_ITEMS {
                        return Err(Error::new(0, "FES 预加载资源数量超过上限"));
                    }
                    cache.entry(name.to_vec()).or_insert(bytes);
                }
                Ok(())
            })();
            result.map_err(|error: Error| Error::new(offset + error.offset, error.message))?;
            scene.preload.lines.pop_front();
            scene.preload.name = None;
        }
        Ok(scene.preload.lines.is_empty())
    }
}

pub(super) struct ImageServices<'a> {
    inner: &'a mut dyn ResourceServices,
    cache: Cache,
}

impl<'a> ImageServices<'a> {
    pub(super) fn new(cache: &Cache, inner: &'a mut dyn ResourceServices) -> Self {
        Self {
            inner,
            cache: cache.clone(),
        }
    }
}

impl ResourceServices for ImageServices<'_> {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        self.inner.resolve(category, name)
    }

    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        match self.cache.get(terminated(name)) {
            Some(bytes) => Ok(Some(bytes.clone())),
            None => self.inner.read(name, mode),
        }
    }
}

fn terminated(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len())]
}
