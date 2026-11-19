use super::{Interpreter, NativeReturn, ResourceServices, format_string};
use crate::resource::ResourceMode;
use crate::{Error, Result, read, text};
use std::{cell::RefCell, rc::Rc, sync::Arc};

/// 可由资源服务及程序装载器共享的预读队列和原始字节缓存。
#[derive(Clone, Default)]
pub struct ResourceCache(Rc<RefCell<State>>);

struct Entry {
    name: Vec<u8>,
    key: String,
    life: i32,
    priority: i32,
    attempted: bool,
    bytes: Option<Arc<Vec<u8>>>,
}

struct State {
    enabled: bool,
    entries: Vec<Entry>,
    progress: i32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            enabled: true,
            entries: Vec::new(),
            progress: 0,
        }
    }
}

impl ResourceCache {
    pub fn with_enabled(enabled: bool) -> Self {
        Self(Rc::new(RefCell::new(State {
            enabled,
            ..Default::default()
        })))
    }

    pub fn len(&self) -> usize {
        self.0.borrow().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_ready(&self, priority: i32) -> bool {
        let state = self.0.borrow();
        !state.enabled || priority <= state.progress
    }

    pub fn clear(&self) {
        let mut state = self.0.borrow_mut();
        if state.enabled {
            state.entries.clear();
            state.progress = i32::MAX;
        }
    }

    /// 同名条目仅刷新寿命，保留首次名称、排序等级及已有读取结果。
    pub fn enqueue(&self, name: &[u8], life: i32) -> Result<()> {
        let mut state = self.0.borrow_mut();
        if !state.enabled {
            return Ok(());
        }
        let name = terminated(name);
        let decoded = text::cp932(name)?;
        if decoded.encode_utf16().count() >= 260 {
            return Err(Error::new(0, "资源预读名称超过缓冲区容量"));
        }
        let key = decoded.to_uppercase();
        if let Some(entry) = state.entries.iter_mut().find(|entry| entry.key == key) {
            entry.life = life;
        } else {
            if state.entries.len() >= read::MAX_ITEMS {
                return Err(Error::new(0, "资源预读条目超过上限"));
            }
            state.entries.push(Entry {
                name: name.to_vec(),
                key,
                life,
                priority: life,
                attempted: false,
                bytes: None,
            });
        }
        Ok(())
    }

    /// 按登记等级稳定排序并把完成等级归零，不重复装载已有结果。
    pub fn start(&self) {
        let mut state = self.0.borrow_mut();
        if state.enabled {
            state.entries.sort_by_key(|entry| entry.priority);
            state.progress = 0;
        }
    }

    /// 推进原工作循环的一组读取；true 表示本轮已遍历至队列末尾。
    pub fn advance(&self, services: &mut dyn ResourceServices) -> Result<bool> {
        if !self.0.borrow().enabled {
            return Ok(true);
        }
        let mut group = -1;
        let mut index = 0;
        loop {
            let state = self.0.borrow();
            let Some(entry) = state.entries.get(index) else {
                drop(state);
                self.0.borrow_mut().progress = i32::MAX;
                return Ok(true);
            };
            if group != -1 && group != entry.life {
                drop(state);
                self.0.borrow_mut().progress = group;
                return Ok(false);
            }
            if entry.attempted {
                index += 1;
                continue;
            }
            let name = entry.name.clone();
            let key = entry.key.clone();
            drop(state);
            let bytes = services.read(&name, ResourceMode::LooseFirst)?;
            if bytes
                .as_ref()
                .is_some_and(|bytes| bytes.len() > read::MAX_BYTES)
            {
                return Err(Error::new(0, "资源预读正文超过读取上限"));
            }
            let mut state = self.0.borrow_mut();
            if let Some(entry) = state
                .entries
                .get_mut(index)
                .filter(|entry| entry.key == key)
            {
                entry.bytes = bytes.map(Arc::new);
                entry.attempted = true;
                group = entry.life;
            }
            index += 1;
        }
    }

    pub fn age(&self) {
        let mut state = self.0.borrow_mut();
        if state.enabled {
            state.entries.retain_mut(|entry| {
                entry.life = entry.life.wrapping_sub(1);
                if entry.life < 0 {
                    return false;
                }
                entry.priority = entry.priority.wrapping_sub(1);
                true
            });
            state.progress = state.progress.wrapping_sub(1);
        }
    }

    /// 返回独立副本；清空或老化预读队列不改变已交出的正文。
    pub fn read_cached(&self, name: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(self.source_cached(name)?.map(|bytes| (*bytes).clone()))
    }

    fn source_cached(&self, name: &[u8]) -> Result<Option<Arc<Vec<u8>>>> {
        let state = self.0.borrow();
        if !state.enabled || state.entries.is_empty() {
            return Ok(None);
        }
        let key = text::cp932(terminated(name))?.to_uppercase();
        Ok(state
            .entries
            .iter()
            .find(|entry| entry.key == key && entry.attempted)
            .and_then(|entry| entry.bytes.clone()))
    }
}

/// 为同步资源服务增加预读缓存；克隆服务时沿用同一份缓存。
#[derive(Clone)]
pub struct CachedResources<S> {
    pub inner: S,
    cache: ResourceCache,
    sound_names: Option<SoundNames>,
}

#[derive(Clone)]
struct SoundNames {
    // Routing uses the definition; user sound settings only change playback properties.
    definition: Option<crate::audio::SoundConfig>,
}

impl<S> std::ops::Deref for CachedResources<S> {
    type Target = S;
    fn deref(&self) -> &S {
        &self.inner
    }
}

impl<S> std::ops::DerefMut for CachedResources<S> {
    fn deref_mut(&mut self) -> &mut S {
        &mut self.inner
    }
}

impl<S: ResourceServices> CachedResources<S> {
    pub fn new(inner: S) -> Self {
        let cache = inner.resource_cache().unwrap_or_default();
        Self::with_cache(inner, cache)
    }

    pub fn with_cache(inner: S, cache: ResourceCache) -> Self {
        Self {
            inner,
            cache,
            sound_names: None,
        }
    }

    pub(crate) fn with_sound_names(
        mut self,
        definition: Option<crate::audio::SoundConfig>,
    ) -> Self {
        self.sound_names = Some(SoundNames { definition });
        self
    }
}

impl<S: ResourceServices> ResourceServices for CachedResources<S> {
    fn script_encoding(&self) -> &'static encoding_rs::Encoding {
        self.inner.script_encoding()
    }
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        if category == b"sound" {
            if let Some(sound) = &self.sound_names {
                let archive = sound
                    .definition
                    .as_ref()
                    .and_then(|config| config.file_part(name).ok().flatten())
                    .map(|part| part.archive.as_slice())
                    .filter(|archive| !archive.is_empty());
                return match archive {
                    Some(archive) => self.inner.resolve(archive, name),
                    None => crate::resource::Category::default().resolve(name),
                };
            }
        }
        self.inner.resolve(category, name)
    }

    fn resolve_utf16(&mut self, category: &[u8], name: &[u16]) -> Result<Vec<u16>> {
        self.inner.resolve_utf16(category, name)
    }

    fn resolve_file(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        self.inner.resolve_file(category, name)
    }

    fn resolve_slot(&mut self, slot: u32, name: &[u8]) -> Result<Vec<u8>> {
        self.inner.resolve_slot(slot, name)
    }

    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        if let Some(bytes) = self.cache.read_cached(name)? {
            return Ok(Some(bytes));
        }
        self.inner.read(name, mode)
    }

    fn open(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<crate::io::SharedSource>> {
        if let Some(bytes) = self.cache.source_cached(name)? {
            return Ok(Some(bytes));
        }
        self.inner.open(name, mode)
    }

    fn resource_cache(&self) -> Option<ResourceCache> {
        Some(self.cache.clone())
    }

    fn read_utf16(&mut self, name: &[u16], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        self.inner.read_utf16(name, mode)
    }

    fn open_utf16(
        &mut self,
        name: &[u16],
        mode: ResourceMode,
    ) -> Result<Option<crate::io::SharedSource>> {
        self.inner.open_utf16(name, mode)
    }
}

pub(crate) fn call(
    index: u32,
    arguments: &[u8],
    vm: &Interpreter,
    services: Option<&mut dyn ResourceServices>,
) -> Result<NativeReturn> {
    let count = match index {
        435 => 3,
        437 => 1,
        434 | 436 | 438 => 0,
        _ => unreachable!(),
    };
    if arguments.len() != count * 4 {
        return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
    }
    let services = services.ok_or_else(|| Error::new(vm.pc, "运行时未配置资源服务"))?;
    let cache = services
        .resource_cache()
        .ok_or_else(|| Error::new(vm.pc, "资源服务未提供预读缓存"))?;
    let word = |n: usize| read::u32_at(arguments, n * 4);
    match index {
        434 => cache.clear(),
        435 => {
            let category = format_string(vm, word(1)?, 0)?;
            let name = format_string(vm, word(0)?, 0)?;
            if terminated(&category).len() >= 260 || terminated(&name).len() >= 260 {
                return Err(Error::new(vm.pc, "资源预读参数超过缓冲区容量"));
            }
            let name = services.resolve(terminated(&category), terminated(&name))?;
            cache.enqueue(&name, word(2)? as i32)?;
        }
        436 => {
            cache.start();
            while !cache.advance(services)? {}
        }
        437 => return Ok(NativeReturn::Value(cache.is_ready(word(0)? as i32) as u32)),
        438 => cache.age(),
        _ => unreachable!(),
    }
    Ok(NativeReturn::Continue)
}

fn terminated(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len())]
}
