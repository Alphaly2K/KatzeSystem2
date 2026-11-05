use super::{ObjectFactory, ObjectHost};
use crate::kcs::{Interpreter, NativeReturn, ResourceServices, format_string};
use crate::resource::{ResourceMode, XmlDocument};
use crate::{Error, Result, read, text};

#[derive(Default)]
pub(super) struct EmoconState {
    texture_scale: i32,
    sizes: Vec<(u32, i32)>,
    channels: Vec<(u32, Vec<Pose>)>,
    marks: Vec<Mark>,
    stop: String,
}

struct Pose {
    id: u32,
    offsets: Vec<(u32, [u32; 2])>,
}

struct Mark {
    name: String,
    label: String,
    position: [i32; 2],
    scale: i32,
    half: bool,
    textures: [String; 2],
}

impl EmoconState {
    fn reload(&mut self, services: &mut dyn ResourceServices) -> Result<()> {
        self.sizes.clear();
        self.channels.clear();
        self.marks.clear();
        let Some(xml) = load_xml(services, b"config.int/emodef.xml")? else {
            return Ok(());
        };
        for i in 1..=read::MAX_ITEMS {
            let path = format!("size/s{i:02}");
            if xml.text(format!("{path}/prefix").as_bytes())?.is_none() {
                break;
            }
            self.sizes.push((
                integer(&xml, &path, "id", 100)?,
                integer(&xml, &path, "scale", 100)? as i32,
            ));
        }
        self.texture_scale = xml.integer(b"size/texscale", 100)? as i32;
        for i in 1..=read::MAX_ITEMS {
            let path = format!("ch/c{i:02}");
            let id = integer(&xml, &path, "id", u32::MAX)?;
            if id == u32::MAX {
                break;
            }
            let mut poses = Vec::new();
            for j in 1..=read::MAX_ITEMS {
                let path = format!("{path}/p{j:02}");
                let id = integer(&xml, &path, "id", u32::MAX)?;
                if id == u32::MAX {
                    break;
                }
                let mut offsets = Vec::new();
                if let Some(table) = xml.text(format!("{path}/table").as_bytes())? {
                    let mut name = b"config.int/".to_vec();
                    name.extend(cp932(std::str::from_utf8(table).unwrap())?);
                    if let Some(table) = load_xml(services, &name)? {
                        for k in 1..=read::MAX_ITEMS {
                            let path = format!("em{k:02}");
                            let id = integer(&table, &path, "id", u32::MAX)?;
                            if id == u32::MAX {
                                break;
                            }
                            offsets.push((
                                id,
                                [
                                    integer(&table, &path, "offset/x", 0)?,
                                    integer(&table, &path, "offset/y", 0)?,
                                ],
                            ));
                        }
                    }
                }
                poses.push(Pose { id, offsets });
            }
            self.channels.push((id, poses));
        }
        self.stop = string(&xml, "emocon/stop", "label")?;
        for i in 1..=read::MAX_ITEMS {
            let path = format!("emocon/em{i:02}");
            if integer(&xml, &path, "id", u32::MAX)? == u32::MAX {
                break;
            }
            self.marks.push(Mark {
                name: string(&xml, &path, "name")?,
                label: string(&xml, &path, "label")?,
                position: [
                    integer(&xml, &path, "pos/x", 100)? as i32,
                    integer(&xml, &path, "pos/y", 100)? as i32,
                ],
                scale: integer(&xml, &path, "scale", u32::MAX)? as i32,
                half: integer(&xml, &path, "tex/half", 0)? != 0,
                textures: [string(&xml, &path, "tex/l")?, string(&xml, &path, "tex/s")?],
            });
        }
        Ok(())
    }
}

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn emocon_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let state = self.resources.emocon.entry(self.host.task_id).or_default();
        if index == 1055 {
            let services = self
                .resources
                .services
                .as_deref_mut()
                .ok_or_else(|| Error::new(vm.pc, "运行时未配置图形标记资源服务"))?;
            state.reload(services)?;
            return Ok(NativeReturn::Continue);
        }
        if index == 1057 {
            return Ok(NativeReturn::Value(state.marks.len() as u32));
        }
        let length = match index {
            1056 => 32,
            1058 | 1059 => 8,
            1060 => 4,
            1061 => 20,
            _ => unreachable!(),
        };
        if arguments.len() != length {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let arg = |at| read::u32_at(arguments, at);
        match index {
            1056 => {
                let source = format_string(vm, arg(0)?, 0)?;
                let source = text::cp932(terminated(&source))?;
                let x = arg(16)?;
                let y = arg(20)?;
                let scale = arg(24)?;
                let output = arg(28)?;
                vm.write(x, &[0; 4])?;
                vm.write(y, &[0; 4])?;
                let size_id = arg(4)?;
                let size = state.sizes.iter().find(|(id, _)| *id == size_id);
                let mark = state.marks.iter().find(|mark| mark.name == source);
                let texture = if let (Some((_, size)), Some(mark)) = (size, mark) {
                    let percent = |value: i32| value.wrapping_mul(*size) / 100;
                    vm.write(
                        x,
                        &percent(mark.position[0].wrapping_add(arg(8)? as i32)).to_le_bytes(),
                    )?;
                    vm.write(
                        y,
                        &percent(mark.position[1].wrapping_add(arg(12)? as i32)).to_le_bytes(),
                    )?;
                    let mut factor = percent(state.texture_scale).wrapping_mul(mark.scale) / 100;
                    let small = factor < 50 && mark.half;
                    if small {
                        factor = factor.wrapping_mul(2);
                    }
                    vm.write(scale, &factor.to_le_bytes())?;
                    &mark.textures[usize::from(small)]
                } else {
                    "dummy"
                };
                write_string(vm, output, texture, false)?;
            }
            1058 | 1059 => {
                let value = state
                    .marks
                    .get(arg(0)? as usize)
                    .map(|mark| {
                        if index == 1058 {
                            mark.name.as_str()
                        } else {
                            mark.label.as_str()
                        }
                    })
                    .unwrap_or_default();
                write_string(vm, arg(4)?, value, true)?;
            }
            1060 => write_string(vm, arg(0)?, &state.stop, true)?,
            1061 => {
                let x = arg(12)?;
                let y = arg(16)?;
                vm.write(x, &[0; 4])?;
                vm.write(y, &[0; 4])?;
                let channel = arg(0)?;
                let pose = arg(4)?;
                let mark = arg(8)?;
                if let Some((_, value)) = state
                    .channels
                    .iter()
                    .find(|(id, _)| *id == channel)
                    .and_then(|(_, poses)| poses.iter().find(|p| p.id == pose))
                    .and_then(|p| p.offsets.iter().find(|(id, _)| *id == mark))
                {
                    vm.write(x, &value[0].to_le_bytes())?;
                    vm.write(y, &value[1].to_le_bytes())?;
                }
            }
            _ => unreachable!(),
        }
        Ok(NativeReturn::Continue)
    }
}

fn terminated(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
}

fn integer(xml: &XmlDocument, path: &str, field: &str, default: u32) -> Result<u32> {
    xml.integer(format!("{path}/{field}").as_bytes(), default)
}

fn string(xml: &XmlDocument, path: &str, field: &str) -> Result<String> {
    Ok(std::str::from_utf8(terminated(
        xml.text(format!("{path}/{field}").as_bytes())?
            .unwrap_or_default(),
    ))
    .unwrap()
    .to_owned())
}

fn cp932(value: &str) -> Result<Vec<u8>> {
    let (bytes, _, errors) = encoding_rs::SHIFT_JIS.encode(value);
    if errors {
        return Err(Error::new(0, "图形标记文本无法编码为 CP932"));
    }
    Ok(bytes.into_owned())
}

fn write_string(vm: &mut Interpreter, address: u32, value: &str, optional: bool) -> Result<()> {
    if optional && vm.read(address, 1).is_err() {
        return Ok(());
    }
    let mut bytes = cp932(value)?;
    bytes.push(0);
    vm.write(address, &bytes)
}

fn load_xml(services: &mut dyn ResourceServices, name: &[u8]) -> Result<Option<XmlDocument>> {
    Ok(services
        .read(name, ResourceMode::LooseFirst)?
        .and_then(|bytes| XmlDocument::parse(&bytes).ok()))
}
