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

const BODY_OPTIONS: u8 = 1;
const EFFECTS_MODE: u8 = 2;
const TONE: u8 = 4;
const EFFECTS_ACTIVE: u8 = 8;

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
        let expression = read_model_table(resources, source.expression_config.as_deref())?;
        let body = read_model_table(resources, source.body_config.as_deref())?;
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
        let Some(capabilities) = read_capabilities(&mut player.output)? else {
            return Ok(None);
        };
        player.capabilities = capabilities;
        Ok(Some(Box::new(player)))
    }
}

struct Player {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
    last_frame: Option<ModelFrame>,
    capabilities: u8,
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
            capabilities: 0,
        })
    }

    fn request_words(&mut self, tag: u8, words: &[u32]) -> Result<bool> {
        self.input.write_all(&[tag]).map_err(host_error)?;
        for word in words {
            self.input
                .write_all(&word.to_le_bytes())
                .map_err(host_error)?;
        }
        self.accepted()
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
        if self.capabilities & BODY_OPTIONS != 0 {
            self.input.write_all(b"B").map_err(host_error)?;
            write_field(&mut self.input, options).map_err(host_error)?;
            if !self.accepted()? {
                return Err(Error::new(0, "Model host rejected body options"));
            }
        }
        Ok(())
    }

    fn set_effects_mode(&mut self, mode: u32) -> Result<()> {
        if self.capabilities & EFFECTS_MODE != 0 && !self.request_words(b'E', &[mode])? {
            return Err(Error::new(0, "Model host rejected effect mode"));
        }
        Ok(())
    }

    fn set_tone(&mut self, rgb: [u32; 3]) -> Result<bool> {
        if self.capabilities & TONE == 0 {
            return Ok(false);
        }
        let applied = self.request_words(b'T', &rgb)?;
        self.last_frame = None;
        Ok(applied)
    }

    fn effects_active(&mut self, system_time_ms: u32) -> Result<bool> {
        if self.capabilities & EFFECTS_ACTIVE == 0 {
            return Ok(false);
        }
        self.request_words(b'A', &[system_time_ms])
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

fn read_model_table(resources: &mut dyn ResourceServices, name: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut table = Vec::new();
    if let Some(name) = name {
        let mut remaining = TABLE_LIMIT;
        append_model_table(resources, name, &mut Vec::new(), &mut remaining, &mut table)?;
    }
    Ok(table)
}

fn append_model_table(
    resources: &mut dyn ResourceServices,
    name: &[u8],
    active: &mut Vec<Vec<u8>>,
    remaining: &mut usize,
    table: &mut Vec<u8>,
) -> Result<()> {
    if active.len() == 16 {
        return Err(Error::new(0, "Model table include depth exceeds its limit"));
    }
    let path = resources.resolve(b"m2config", name)?;
    if active
        .iter()
        .any(|parent| parent.eq_ignore_ascii_case(&path))
    {
        return Err(Error::new(0, "Model table includes itself"));
    }
    let Some(source) = resources.open(&path, ResourceMode::LooseFirst)? else {
        if active.is_empty() {
            return Ok(());
        }
        return Err(Error::new(0, "Model table include is missing"));
    };
    let bytes = read_bounded(source, *remaining)?;
    *remaining -= bytes.len();
    active.push(path);

    let start = table.len();
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    let mut includes = false;
    for line in bytes[..end].split_inclusive(|&byte| byte == b'\n') {
        let comment = line.windows(2).position(|pair| pair == b"//");
        let entry = line[..comment.unwrap_or(line.len())].trim_ascii();
        if entry.starts_with(b"#") {
            includes = entry.eq_ignore_ascii_case(b"#include");
            if includes {
                continue;
            }
        }
        if !includes {
            table.extend_from_slice(line);
        } else if !entry.is_empty() {
            let file = entry
                .strip_prefix(b"\"")
                .and_then(|entry| entry.strip_suffix(b"\""))
                .unwrap_or(entry);
            append_model_table(resources, file, active, remaining, table)?;
        }
    }
    if active.len() == 1 {
        table.extend_from_slice(&bytes[end..]);
    } else if table.len() > start && table.last() != Some(&b'\n') {
        table.push(b'\n');
    }
    active.pop();
    Ok(())
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

fn read_capabilities(input: &mut impl Read) -> Result<Option<u8>> {
    Ok(match read_tag(input)? {
        b'N' => None,
        b'Y' => Some(0),
        b'B' => Some(BODY_OPTIONS),
        b'E' => Some(BODY_OPTIONS | EFFECTS_MODE),
        b'C' => Some(read_tag(input)?),
        _ => return Err(Error::new(0, "Invalid model host acknowledgement")),
    })
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

    struct Tables<'a>(&'a [(&'a [u8], &'a [u8])]);

    impl ResourceServices for Tables<'_> {
        fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            assert_eq!(category, b"m2config");
            Ok(name.to_ascii_lowercase())
        }

        fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
            assert_eq!(mode, ResourceMode::LooseFirst);
            Ok(self
                .0
                .iter()
                .find(|(path, _)| *path == name)
                .map(|(_, bytes)| bytes.to_vec()))
        }
    }

    #[test]
    fn model_tables_supply_included_sections_in_place() {
        let mut resources = Tables(&[
            (b"face.txt", b"#scale\r\n1\r\n#include\r\nbase.inc // comment\r\n#pattern\r\n%1 2\r\nsmile 1\0suffix"),
            (b"base.inc", b"#include\n\"common.inc\"\n#dif\nsmile 0"),
            (b"common.inc", b"#pattern\n%1 1\nsmile -1\0ignored"),
        ]);
        assert_eq!(
            read_model_table(&mut resources, Some(b"face.txt")).unwrap(),
            b"#scale\r\n1\r\n#pattern\n%1 1\nsmile -1\n#dif\nsmile 0\n#pattern\r\n%1 2\r\nsmile 1\0suffix",
        );
        assert!(
            read_model_table(&mut resources, Some(b"missing.txt"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn model_tables_reject_include_cycles_and_missing_include_data() {
        let mut resources = Tables(&[
            (b"face.txt", b"#include\nbase.inc"),
            (b"base.inc", b"#include\nFACE.TXT"),
            (b"body.txt", b"#include\nunavailable.inc"),
        ]);
        assert!(
            read_model_table(&mut resources, Some(b"face.txt"))
                .unwrap_err()
                .message
                .contains("includes itself")
        );
        assert!(
            read_model_table(&mut resources, Some(b"body.txt"))
                .unwrap_err()
                .message
                .contains("is missing")
        );
    }

    #[test]
    fn model_open_negotiates_capabilities_without_reading_past_legacy_responses() {
        for (message, expected) in [
            (b"NY".as_slice(), None),
            (b"YY", Some(0)),
            (b"BY", Some(BODY_OPTIONS)),
            (b"EY", Some(BODY_OPTIONS | EFFECTS_MODE)),
            (b"C\x0cY", Some(TONE | EFFECTS_ACTIVE)),
        ] {
            let mut input = message;
            assert_eq!(read_capabilities(&mut input).unwrap(), expected);
            assert_eq!(input, b"Y");
        }
        assert!(read_capabilities(&mut b"C".as_slice()).is_err());
    }

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
