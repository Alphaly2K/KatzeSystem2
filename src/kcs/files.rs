use super::{HostState, Interpreter, NativeReturn, ResourceServices, strings};
use crate::io::SharedSource;
use crate::resource::ResourceMode;
use crate::{Error, Result, read};

pub(super) enum OpenedFile {
    Read { source: SharedSource, position: u64 },
    Write(Box<dyn std::io::Write>),
}

pub(super) struct OpenRequest {
    pub name: Vec<u8>,
    pub mode: Option<ResourceMode>,
}

pub(super) fn request(arguments: &[u8], vm: &Interpreter) -> Result<OpenRequest> {
    let args = super::native::words(arguments, 2, vm.pc)?;
    let mut text = Vec::with_capacity(2);
    for address in args {
        let mut bytes = strings::format(vm, address, 0)?;
        bytes.truncate(bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len()));
        if bytes.len() >= 1024 {
            return Err(Error::new(vm.pc, "KCS 文件名称或模式超过容量"));
        }
        text.push(bytes);
    }
    let mode = match text[1].iter().find(|&&b| matches!(b, b'r' | b'w')) {
        Some(b'r') => Some(ResourceMode::File),
        Some(b'w') => None,
        _ => Some(ResourceMode::LooseFirst),
    };
    Ok(OpenRequest {
        name: text.remove(0),
        mode,
    })
}

pub(crate) fn open_native(
    host: &mut HostState,
    arguments: &[u8],
    vm: &Interpreter,
    services: Option<&mut dyn ResourceServices>,
) -> Result<NativeReturn> {
    let request = request(arguments, vm)?;
    let file = if let Some(mode) = request.mode {
        services
            .ok_or_else(|| Error::new(vm.pc, "宿主未配置文件读取服务"))?
            .open(&request.name, mode)?
            .map(|source| OpenedFile::Read {
                source,
                position: 0,
            })
    } else {
        host.file_services
            .as_mut()
            .ok_or_else(|| Error::new(vm.pc, "宿主未配置文件写入服务"))?
            .create(host.task_id, &request.name)?
            .map(OpenedFile::Write)
    };
    Ok(NativeReturn::Value(match file {
        Some(file) => host.register_file(file)?,
        None => 0,
    }))
}

impl HostState {
    fn register_file(&mut self, file: OpenedFile) -> Result<u32> {
        if self.files.len() >= read::MAX_ITEMS {
            return Err(Error::new(0, "KCS 打开文件数量超过上限"));
        }
        let handle = self.handle_allocator().allocate()?;
        self.files.insert(handle, file);
        Ok(handle)
    }

    pub(crate) fn register_file_source(&mut self, source: SharedSource) -> Result<u32> {
        self.register_file(OpenedFile::Read {
            source,
            position: 0,
        })
    }
}

pub(super) fn call(
    host: &mut HostState,
    index: u32,
    arguments: &[u8],
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    if index == 29 {
        return open_native(host, arguments, vm, None);
    }
    if index == 32 {
        let args = super::native::words(arguments, 1, vm.pc)?;
        if args[0] != 0 && host.files.remove(&args[0]).is_none() {
            return Err(Error::new(vm.pc, "KCS 文件句柄无效"));
        }
        return Ok(NativeReturn::Continue);
    }
    let args = super::native::words(arguments, 4, vm.pc)?;
    if args[3] == 0 {
        return Ok(NativeReturn::Value(0));
    }
    let file = host
        .files
        .get_mut(&args[3])
        .ok_or_else(|| Error::new(vm.pc, "KCS 文件句柄无效"))?;
    let size = args[1].wrapping_mul(args[2]) as usize;
    if size > read::MAX_BYTES {
        return Err(Error::new(vm.pc, "KCS 文件传输超过容量"));
    }
    drop(vm.read(args[0], size)?);
    let count = match (index, file) {
        (30, OpenedFile::Read { source, position }) => {
            let mut output = vec![0; size];
            let count = match source.read_at(*position, &mut output) {
                Ok(count) if count <= size => count,
                Ok(_) => return Err(Error::new(vm.pc, "文件读取服务返回量超过缓冲区")),
                Err(_) => return Ok(NativeReturn::Value(0)),
            };
            let next = position
                .checked_add(count as u64)
                .ok_or_else(|| Error::new(vm.pc, "KCS 文件读取位置溢出"))?;
            vm.write(args[0], &output[..count])?;
            *position = next;
            count
        }
        (31, OpenedFile::Write(output)) => match output.write(&vm.read(args[0], size)?) {
            Ok(count) if count <= size => count,
            Ok(_) => return Err(Error::new(vm.pc, "文件写入服务返回量超过缓冲区")),
            Err(_) => 0,
        },
        _ => 0,
    };
    Ok(NativeReturn::Value(count as u32))
}
