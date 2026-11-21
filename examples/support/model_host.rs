use katze_system2::{
    Error, Result,
    host::{
        graphics::Surface,
        model::{ModelFrame, ModelPlayer, ModelSelection, ModelServices},
        resources::{ResourceMode, ResourceServices},
        storage::SharedSource,
    },
};
use std::{
    ffi::OsString,
    io::{self, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::Arc,
};

const MODEL_LIMIT: usize = 128 * 1024 * 1024;
const TABLE_LIMIT: usize = 4 * 1024 * 1024;
const FRAME_LIMIT: usize = 64 * 1024 * 1024;

pub(super) struct ModelHost(OsString);

impl ModelHost {
    pub(super) fn configured() -> Option<Self> {
        std::env::var_os("KATZE_MODEL_HOST").map(Self)
    }
}

impl ModelServices for ModelHost {
    fn open_with_resources(
        &mut self,
        spec: &[u8],
        resources: &mut dyn ResourceServices,
    ) -> Result<Option<Box<dyn ModelPlayer>>> {
        let Some(selection) = ModelSelection::parse(spec) else {
            return Ok(None);
        };
        let Some(source) = selection.open(resources)? else {
            return Ok(None);
        };
        let packed = read_bounded(source.psz, MODEL_LIMIT)?;
        let mut read_table = |name: Option<Vec<u8>>| -> Result<Vec<u8>> {
            let Some(name) = name else {
                return Ok(Vec::new());
            };
            let name = resources.resolve(b"m2config", &name)?;
            resources
                .open(&name, ResourceMode::LooseFirst)?
                .map(|source| read_bounded(source, TABLE_LIMIT))
                .transpose()
                .map(Option::unwrap_or_default)
        };
        let expression = read_table(source.expression_config)?;
        let body = read_table(source.body_config)?;
        let mut player = Player::spawn(&self.0)?;
        player.input.write_all(b"KMOD\x01").map_err(host_error)?;
        for bytes in [
            packed.as_slice(),
            source.initial_motion.as_deref().unwrap_or_default(),
            expression.as_slice(),
            body.as_slice(),
            selection.options,
        ] {
            write_field(&mut player.input, bytes).map_err(host_error)?;
        }
        match read_tag(&mut player.output)? {
            b'N' => Ok(None),
            tag @ (b'Y' | b'B') => {
                player.body_commands = tag == b'B';
                Ok(Some(Box::new(player)))
            }
            _ => Err(Error::new(0, "Invalid model host acknowledgement")),
        }
    }
}

struct Player {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
    last_frame: Option<ModelFrame>,
    body_commands: bool,
}

impl Player {
    fn spawn(executable: &OsString) -> Result<Self> {
        let mut child = Command::new(executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(host_error)?;
        Ok(Self {
            input: child.stdin.take().unwrap(),
            output: child.stdout.take().unwrap(),
            child,
            last_frame: None,
            body_commands: false,
        })
    }

    fn accepted(&mut self) -> Result<bool> {
        match read_tag(&mut self.output)? {
            b'Y' => Ok(true),
            b'N' => Ok(false),
            _ => Err(Error::new(0, "Invalid model host acknowledgement")),
        }
    }
}

impl ModelPlayer for Player {
    fn set_options(&mut self, options: &[u8]) -> Result<bool> {
        self.input.write_all(b"O").map_err(host_error)?;
        write_field(&mut self.input, options).map_err(host_error)?;
        self.accepted()
    }

    fn set_body_options(&mut self, options: &[u8]) -> Result<()> {
        if self.body_commands {
            self.input.write_all(b"B").map_err(host_error)?;
            write_field(&mut self.input, options).map_err(host_error)?;
            if !self.accepted()? {
                return Err(Error::new(0, "Model host rejected body options"));
            }
        }
        Ok(())
    }

    fn frame(&mut self, system_time_ms: u32) -> Result<Option<ModelFrame>> {
        let mut request = [b'F', 0, 0, 0, 0];
        request[1..].copy_from_slice(&system_time_ms.to_le_bytes());
        self.input.write_all(&request).map_err(host_error)?;
        match read_tag(&mut self.output)? {
            b'F' => self.last_frame = Some(read_frame(&mut self.output)?),
            b'N' => self.last_frame = None,
            b'R' if self.last_frame.is_some() => (),
            _ => return Err(Error::new(0, "Invalid model host frame response")),
        }
        Ok(self.last_frame.as_ref().map(|frame| ModelFrame {
            pixels: frame.pixels.clone(),
            origin: frame.origin,
        }))
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_bounded(source: SharedSource, limit: usize) -> Result<Vec<u8>> {
    let len = usize::try_from(source.len()?)
        .ok()
        .filter(|&len| len <= limit)
        .ok_or_else(|| Error::new(0, "Model host resource exceeds its size limit"))?;
    let mut bytes = vec![0; len];
    source.read_exact_at(0, &mut bytes)?;
    Ok(bytes)
}

fn write_field(output: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    let len = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Model host field too large"))?;
    output.write_all(&len.to_le_bytes())?;
    output.write_all(bytes)
}

fn read_tag(input: &mut impl Read) -> Result<u8> {
    let mut tag = [0];
    input.read_exact(&mut tag).map_err(host_error)?;
    Ok(tag[0])
}

fn read_frame(input: &mut impl Read) -> Result<ModelFrame> {
    let mut header = [0; 16];
    input.read_exact(&mut header).map_err(host_error)?;
    let word = |at| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
    let size = [word(0), word(4)];
    let len = (size[0] as usize)
        .checked_mul(size[1] as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|&len| len > 0 && len <= FRAME_LIMIT)
        .ok_or_else(|| Error::new(0, "Invalid model host frame size"))?;
    let mut pixels = vec![0; len];
    input.read_exact(&mut pixels).map_err(host_error)?;
    Ok(ModelFrame {
        pixels: Arc::new(Surface::from_bgra(size, pixels)?),
        origin: [word(8) as i32, word(12) as i32],
    })
}

fn host_error(error: io::Error) -> Error {
    Error::new(0, format!("Model host: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_frame_rejects_invalid_size_and_truncation() {
        let mut bytes = Vec::new();
        for word in [1u32, 2, (-3i32) as u32, (-4i32) as u32] {
            bytes.extend(word.to_le_bytes());
        }
        bytes.extend([1, 2, 3, 255, 4, 5, 6, 127]);
        let frame = read_frame(&mut bytes.as_slice()).unwrap();
        assert_eq!(frame.origin, [-3, -4]);
        assert_eq!(frame.pixels.pixel(0, 1), Some(0x7f060504));
        assert!(read_frame(&mut &bytes[..23]).is_err());
        bytes[..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_frame(&mut bytes.as_slice()).is_err());
    }
}
