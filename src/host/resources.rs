use super::SharedSource;
use crate::{
    Error, Result,
    resource::{ResourceCache, ResourceMode},
};

/// 运行时的同步资源读取服务；None 表示未找到或无法打开。
pub trait ResourceServices {
    /// 依类别补全名称；可直接委托 ArchiveConfig::resolve。
    fn resolve(&mut self, _category: &[u8], _name: &[u8]) -> Result<Vec<u8>> {
        Err(Error::new(0, "资源服务未提供类别名称补全"))
    }

    /// 宽字符类别名称补全；默认桥接 CP932，其他字符由宿主覆盖此方法。
    fn resolve_utf16(&mut self, category: &[u8], name: &[u16]) -> Result<Vec<u16>> {
        let name = &name[..name
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(name.len())];
        let name =
            String::from_utf16(name).map_err(|_| Error::new(0, "资源名称包含无效 UTF-16"))?;
        let name = crate::text::cp932_encoded(&name)?;
        let resolved = self.resolve(category, &name)?;
        let resolved = &resolved[..resolved
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(resolved.len())];
        Ok(crate::text::cp932_replacing(resolved)
            .encode_utf16()
            .collect())
    }

    /// 按类别的散装目录补全文件路径；结果由宿主的 File 读取模式解释。
    fn resolve_file(&mut self, _category: &[u8], _name: &[u8]) -> Result<Vec<u8>> {
        Err(Error::new(0, "资源服务未提供文件路径补全"))
    }

    /// 按启动配置的类别槽位补全名称。
    ///
    /// 声音装载携带 INT 槽位而非类别名；只提供名称服务的宿主沿用 ``sound``
    /// 类别，保持既有实现可用。
    fn resolve_slot(&mut self, _slot: u32, name: &[u8]) -> Result<Vec<u8>> {
        self.resolve(b"sound", name)
    }

    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>>;

    /// 打开可独立交给解码线程的读取源；默认持有普通读取返回的字节。
    fn open(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<SharedSource>> {
        Ok(self
            .read(name, mode)?
            .map(|bytes| std::sync::Arc::new(bytes) as SharedSource))
    }

    /// 打开宽字符资源名称，默认转为 CP932，由宿主提供其他字符的支持。
    fn open_utf16(&mut self, name: &[u16], mode: ResourceMode) -> Result<Option<SharedSource>> {
        self.open(&encode_resource_name(name)?, mode)
    }

    /// CST 与名称表原始文字的编码；默认沿用基线 CP932。
    fn script_encoding(&self) -> &'static encoding_rs::Encoding {
        encoding_rs::SHIFT_JIS
    }

    /// 宽字符资源读取；默认转为 CP932，无法无损表示时由宿主覆盖此方法。
    fn read_utf16(&mut self, name: &[u16], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        self.read(&encode_resource_name(name)?, mode)
    }

    /// 返回与普通读取共用的预读缓存；默认由 CachedResources 提供。
    fn resource_cache(&self) -> Option<ResourceCache> {
        None
    }
}

fn encode_resource_name(name: &[u16]) -> Result<Vec<u8>> {
    let name = name.split(|&unit| unit == 0).next().unwrap_or_default();
    let name = String::from_utf16(name).map_err(|_| Error::new(0, "资源名称包含无效 UTF-16"))?;
    let (bytes, _, errors) = encoding_rs::SHIFT_JIS.encode(&name);
    if errors {
        return Err(Error::new(0, "资源服务需要宽字符名称读取能力"));
    }
    Ok(bytes.into_owned())
}

impl<S: ResourceServices + ?Sized> ResourceServices for &mut S {
    fn script_encoding(&self) -> &'static encoding_rs::Encoding {
        (**self).script_encoding()
    }
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        (**self).resolve(category, name)
    }
    fn resolve_utf16(&mut self, category: &[u8], name: &[u16]) -> Result<Vec<u16>> {
        (**self).resolve_utf16(category, name)
    }
    fn resolve_file(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        (**self).resolve_file(category, name)
    }
    fn resolve_slot(&mut self, slot: u32, name: &[u8]) -> Result<Vec<u8>> {
        (**self).resolve_slot(slot, name)
    }
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        (**self).read(name, mode)
    }
    fn open(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<SharedSource>> {
        (**self).open(name, mode)
    }
    fn open_utf16(&mut self, name: &[u16], mode: ResourceMode) -> Result<Option<SharedSource>> {
        (**self).open_utf16(name, mode)
    }
    fn read_utf16(&mut self, name: &[u16], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        (**self).read_utf16(name, mode)
    }
    fn resource_cache(&self) -> Option<ResourceCache> {
        (**self).resource_cache()
    }
}
