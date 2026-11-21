use super::{ObjectFactory, ObjectHost, Resources};
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::resource::ResourceMode;
use crate::task::CommandBuffers;
use crate::text::delimited::{char_width, field};
use crate::{Error, Result, read};
use std::collections::BTreeMap;

pub(super) type AnimationAliases = BTreeMap<Vec<u8>, Vec<u8>>;

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn image_animation_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &Interpreter,
    ) -> Result<NativeReturn> {
        let query = index == 430;
        if arguments.len() != if query { 4 } else { 12 } {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let mut name = format_string(vm, read::u32_at(arguments, if query { 0 } else { 8 })?, 0)?;
        name.truncate(terminated(&name).len());
        if query {
            name = self
                .resources
                .services
                .as_deref_mut()
                .ok_or_else(|| Error::new(vm.pc, "运行时未配置动画资源服务"))?
                .resolve(b"image", &name)?;
        }
        let found = self.resources.find_image_animation(&name)?;
        if query {
            return Ok(NativeReturn::Value(u32::from(found.is_some())));
        }
        let mut name = found.unwrap_or_default();
        name.push(0);
        self.command_target_with_buffers(
            read::u32_at(arguments, 0)?,
            299,
            [read::u32_at(arguments, 4)?, 0, 0],
            &mut CommandBuffers {
                data1: Some(&mut name),
                ..CommandBuffers::default()
            },
        )?;
        Ok(NativeReturn::Continue)
    }
}

impl Resources {
    pub(super) fn find_image_animation(&mut self, name: &[u8]) -> Result<Option<Vec<u8>>> {
        let services = self
            .services
            .as_deref_mut()
            .ok_or_else(|| Error::new(0, "运行时未配置动画资源服务"))?;
        let name = terminated(name);
        let (basename, extension) = name_parts(name);
        let path = [&name[..extension], b".anm"].concat();
        if services.open(&path, ResourceMode::LooseFirst)?.is_some() {
            return Ok(Some(path));
        }
        if self.animation_aliases.is_none() {
            let table = services.resolve(b"image", b"anmtable.csv")?;
            let bytes = services.read(terminated(&table), ResourceMode::LooseFirst)?;
            self.animation_aliases = Some(parse_aliases(bytes.as_deref().unwrap_or_default())?);
        }
        let key = lowercase(&name[basename..extension]);
        let Some(alias) = self.animation_aliases.as_ref().unwrap().get(&key) else {
            return Ok(None);
        };
        let alias = services.resolve(b"image", alias)?;
        let alias = terminated(&alias);
        let path = [&alias[..name_parts(alias).1], b".anm"].concat();
        Ok(services
            .open(&path, ResourceMode::LooseFirst)?
            .map(|_| path))
    }
}

fn terminated(bytes: &[u8]) -> &[u8] {
    bytes.split(|&b| b == 0).next().unwrap()
}

pub(super) fn name_parts(name: &[u8]) -> (usize, usize) {
    let mut basename = 0;
    let mut extension = name.len();
    let mut pos = 0;
    while pos < name.len() {
        let width = match &name[pos..] {
            [0xf0..=0xfc, 0x40..=0x7e | 0x80..=0xfc, ..] => 2,
            bytes => char_width(bytes),
        };
        if width == 1 {
            match name[pos] {
                b'/' | b'\\' => {
                    basename = pos + usize::from(pos != 0);
                    extension = name.len();
                }
                b'.' => extension = pos,
                _ => (),
            }
        }
        pos += width;
    }
    (basename, extension)
}

fn lowercase(bytes: &[u8]) -> Vec<u8> {
    let mut key = bytes.to_vec();
    let mut pos = 0;
    while pos < key.len() {
        let width = char_width(&key[pos..]);
        if width == 1 {
            key[pos].make_ascii_lowercase();
        }
        pos += width;
    }
    key
}

fn parse_aliases(bytes: &[u8]) -> Result<AnimationAliases> {
    let mut bytes = terminated(bytes);
    let mut aliases = BTreeMap::new();
    while !bytes.is_empty() {
        if !matches!(bytes[0], b'\r' | b'\n') {
            let key = field(&mut bytes, b'\t')?;
            if !bytes.is_empty() && !matches!(bytes[0], b'\r' | b'\n') {
                let value = field(&mut bytes, b'\t')?;
                if !key.is_empty() {
                    aliases.entry(key).or_insert(value);
                }
                while !bytes.is_empty() && !matches!(bytes[0], b'\r' | b'\n') {
                    field(&mut bytes, b'\t')?;
                }
            }
        }
        if matches!(bytes.first(), Some(b'\r' | b'\n')) {
            bytes = &bytes[1..];
        }
    }
    Ok(aliases)
}
