use super::{ObjectFactory, ObjectHost, Resources};
use crate::graphics::ImageReference;
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::resource::ResourceMode;
use crate::{Error, Result, read};

const RECORD_SIZE: usize = 136;

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn image_pack_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        if arguments.len() != if index == 703 { 12 } else { 8 } {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let first = read::u32_at(arguments, 0)?;
        let second = read::u32_at(arguments, 4)?;
        let owner = self.host.task_id;
        let images = &mut self.resources.images;
        match index {
            703 => {
                if first == 0 {
                    return Ok(NativeReturn::Value(0));
                }
                let name = format_string(vm, first, 0)?;
                if name.is_empty() || second == 0 {
                    return Ok(NativeReturn::Value(0));
                }
                let capacity = read::u32_at(arguments, 8)? as i32;
                if capacity <= 0 {
                    return Ok(NativeReturn::Value(0));
                }
                vm.read(second, record_len(capacity as u32)?)?;
                let services = self
                    .resources
                    .services
                    .as_deref_mut()
                    .ok_or_else(|| Error::new(vm.pc, "运行时未配置图像资源服务"))?;
                let name = services.resolve(b"image", &name)?;
                let name = &name[..name
                    .iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(name.len())];
                let count =
                    self.resources
                        .load_image_pack(owner, name, second, capacity as usize, vm)?;
                Ok(NativeReturn::Value(count as u32))
            }
            704 => {
                if first == 0 || second == 0 {
                    return Ok(NativeReturn::Continue);
                }
                vm.read(first, record_len(second)?)?;
                for index in 0..second {
                    let address = first + index * RECORD_SIZE as u32;
                    let (kind, handle) = {
                        let record = vm.read(address, 8)?;
                        (read::u32_at(&record, 0)?, read::u32_at(&record, 4)?)
                    };
                    if handle != 0 {
                        match kind {
                            1 => images.release_for(owner, handle)?,
                            2 => images.release_shared_image_entry(handle)?,
                            _ => (),
                        }
                    }
                    vm.write(
                        address,
                        &[0; 8][..if matches!(kind, 1 | 2) { 8 } else { 4 }],
                    )?;
                }
                Ok(NativeReturn::Continue)
            }
            706 => {
                if first == 0 || second == 0 {
                    return Ok(NativeReturn::Value(0));
                }
                vm.read(second, 36)?;
                let mut words = [0; 9];
                let info = images.shared_image_info(first)?;
                if let Some(info) = info {
                    words[..2].copy_from_slice(&info.offset.map(|value| value as u32));
                    words[2..4].copy_from_slice(&info.size);
                    for axis in 0..2 {
                        words[4 + axis] = if info.canvas_size[axis] == 0 {
                            info.size[axis]
                        } else {
                            info.canvas_size[axis]
                        };
                    }
                    words[6] = info.bit_depth;
                    words[7..].copy_from_slice(&info.base.map(|value| value as u32));
                }
                vm.write(second, words.map(u32::to_le_bytes).as_flattened())?;
                Ok(NativeReturn::Value(u32::from(info.is_some())))
            }
            709 => Ok(NativeReturn::Value(
                images.shared_image_frame(first, second as i32)?,
            )),
            _ => unreachable!(),
        }
    }
}

fn record_len(count: u32) -> Result<usize> {
    (count as usize)
        .checked_mul(RECORD_SIZE)
        .filter(|&len| len <= read::MAX_BYTES)
        .ok_or_else(|| Error::new(0, "图像组合记录超过缓冲区上限"))
}

impl Resources {
    fn load_image_pack(
        &mut self,
        owner: u32,
        name: &[u8],
        target: u32,
        capacity: usize,
        vm: &mut Interpreter,
    ) -> Result<usize> {
        if name.len() >= 256 {
            return Err(Error::new(vm.pc, "图像组合名称超过缓冲区上限"));
        }
        for index in 0..capacity {
            vm.write(target + (index * RECORD_SIZE) as u32, &[0; 8])?;
        }
        let (stem, extension) = split_extension(name);
        let mut parts = stem
            .split(|&byte| byte == b',')
            .filter(|part| !part.is_empty());
        let Some(base) = parts.next() else {
            return Ok(0);
        };
        let parts: Vec<_> = parts.map(|part| part[0]).collect();
        if parts.is_empty() {
            let (kind, handle) = if self.find_image_animation(name)?.is_some() {
                (2, self.load_shared_image(owner, name)?)
            } else {
                (1, self.load_pack_image(owner, name)?)
            };
            if handle != 0 {
                write_pack_record(vm, target, 0, kind, handle, None)?;
                return Ok(1);
            }
            return Ok(0);
        }

        let mut count = 0;
        let mut group = base.to_vec();
        let mut after_animated = false;
        for (index, &part) in parts.iter().enumerate() {
            if part == b'0' {
                continue;
            }
            let mut layer = base.to_vec();
            layer.push(b'_');
            layer.resize(layer.len() + index, b'0');
            let part = (part as i8 as i32).to_le_bytes();
            let part = &part[..if part[1] == 0 { 1 } else { 3 }];
            layer.extend_from_slice(part);
            append_extension(&mut layer, extension);
            let animated = self.find_image_animation(&layer)?.is_some();
            if !animated && !after_animated {
                group.push(b',');
                group.extend_from_slice(part);
                continue;
            }
            if count == capacity {
                return Err(Error::new(vm.pc, "图像组合记录容量不足"));
            }
            if animated {
                // 带动画的分量使用已补全名称，再追加一份原扩展名。
                append_extension(&mut layer, extension);
            }
            let label = pack_label(&layer)?;
            let (kind, handle) = if animated {
                (2, self.load_shared_image(owner, &layer)?)
            } else {
                append_extension(&mut group, extension);
                (1, self.load_pack_image(owner, &group)?)
            };
            if handle != 0 {
                write_pack_record(vm, target, count, kind, handle, Some(&label))?;
                count += 1;
            }
            group.truncate(base.len());
            for _ in 0..index {
                group.extend_from_slice(b",0");
            }
            after_animated = animated;
        }
        Ok(count)
    }

    fn load_shared_image(&mut self, owner: u32, name: &[u8]) -> Result<u32> {
        let bytes = self
            .services
            .as_deref_mut()
            .unwrap()
            .read(name, ResourceMode::LooseFirst)?;
        match bytes.filter(|data| !data.is_empty()) {
            Some(data) => self.images.create_shared_image_entry(owner, &data),
            None => Ok(0),
        }
    }

    fn load_pack_image(&mut self, owner: u32, name: &[u8]) -> Result<u32> {
        if !split_extension(name).0.contains(&b',') {
            let reference = ImageReference::parse(name)?;
            let services = self.services.as_deref_mut().unwrap();
            if let Some(image) =
                self.images
                    .decode(&reference.name, Some(reference.frame), false, services)?
            {
                let alpha = Some(image.info().reserved != 0);
                return self.images.insert(owner, image, alpha);
            }
            return Ok(0);
        }
        let Some(layers) = self.load_image_resource(name, false)? else {
            return Ok(0);
        };
        let resource = self.images.insert_layered_resource(None, layers, None)?;
        let image = self.images.resource_entry(resource)?.image;
        let result = self
            .images
            .share_pixels_for(Some(owner), self.images.pixel_handle(image)?);
        self.images.release_resource_reference(resource)?;
        result
    }
}

fn split_extension(name: &[u8]) -> (&[u8], &[u8]) {
    name.split_at(super::image_animation::name_parts(name).1)
}

fn append_extension(name: &mut Vec<u8>, extension: &[u8]) {
    if extension.is_empty() {
        name.push(b'.');
    } else {
        name.extend_from_slice(extension);
    }
}

fn pack_label(name: &[u8]) -> Result<Vec<u8>> {
    let label: Vec<u8> = crate::text::cp932_replacing(name)
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    if label.len() > RECORD_SIZE - 8 {
        return Err(Error::new(0, "图像组合名称超过记录上限"));
    }
    Ok(label)
}

fn write_pack_record(
    vm: &mut Interpreter,
    target: u32,
    index: usize,
    kind: u32,
    handle: u32,
    label: Option<&[u8]>,
) -> Result<()> {
    let address = target + (index * RECORD_SIZE) as u32;
    vm.write(address, [kind, handle].map(u32::to_le_bytes).as_flattened())?;
    if let Some(label) = label {
        vm.write(address + 8, label)?;
    }
    Ok(())
}
