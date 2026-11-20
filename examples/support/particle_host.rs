use katze_system2::{
    Error, Result,
    graphics::Transform,
    host::effect::{EffectServices, ParticleContext, ParticleFrame, ParticlePlayer},
};
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

const MESH_BYTES: usize = 148;

pub(super) struct ParticleHost {
    executable: OsString,
    root: PathBuf,
}

impl ParticleHost {
    pub(super) fn configured(root: &str) -> Option<Self> {
        Some(Self {
            executable: std::env::var_os("KATZE_PARTICLE_HOST")?,
            root: root.into(),
        })
    }
}

impl EffectServices for ParticleHost {
    fn open_particle(
        &mut self,
        _: u32,
        name: &[u8],
        bytes: &[u8],
    ) -> Result<Option<Box<dyn ParticlePlayer>>> {
        let name = std::str::from_utf8(name).map_err(|error| Error::new(0, error.to_string()))?;
        let path = self.root.join(name.replace('\\', "/"));
        let path = std::fs::canonicalize(path).map_err(host_error)?;
        if std::fs::read(&path).map_err(host_error)? != bytes {
            return Ok(None);
        }
        let mut child = Command::new(&self.executable)
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(host_error)?;
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let mut player = Player {
            child,
            input,
            output,
            mesh: Vec::new(),
        };
        let mut status = [0];
        player.output.read_exact(&mut status).map_err(host_error)?;
        match status[0] {
            0 => Ok(None),
            1 => Ok(Some(Box::new(player))),
            _ => Err(Error::new(0, "Invalid particle host response")),
        }
    }
}

struct Player {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
    mesh: Vec<u8>,
}

impl Player {
    fn update(
        &mut self,
        operation: u8,
        context: ParticleContext<'_>,
        frame: ParticleFrame<'_>,
    ) -> Result<()> {
        let mut packet = [0; 1 + 39 * 4];
        packet[0] = operation;
        let count = frame.quads.len();
        let setting_words = [
            context.elapsed,
            context.delta,
            count as u32,
            (context.surface_size[0] as f32).to_bits(),
            (context.surface_size[1] as f32).to_bits(),
            context.texture_size[0].to_bits(),
            context.texture_size[1].to_bits(),
        ];
        for (bytes, word) in packet[1..]
            .chunks_exact_mut(4)
            .zip(setting_words.into_iter().chain(*context.parameters))
        {
            bytes.copy_from_slice(&word.to_le_bytes());
        }
        self.input.write_all(&packet).map_err(host_error)?;
        self.mesh.resize(count * MESH_BYTES, 0);
        self.output.read_exact(&mut self.mesh).map_err(host_error)?;
        for (quad, bytes) in frame
            .quads
            .iter_mut()
            .zip(self.mesh.chunks_exact(MESH_BYTES))
        {
            let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
            let float = |at| f32::from_bits(word(at));
            let mut transform = Transform::default();
            transform.set_scale([float(0x84), float(0x88), float(0x8c)]);
            transform.set_rotation([float(0x78), float(0x7c), float(0x80)]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&[float(0x6c), float(0x70), float(0x74)]);
            for (index, vertex) in quad.iter_mut().enumerate() {
                let at = index * 24;
                let point: [f32; 4] = std::array::from_fn(|axis| {
                    float(at) * matrix[axis]
                        + float(at + 4) * matrix[axis + 4]
                        + float(at + 8) * matrix[axis + 8]
                        + matrix[axis + 12]
                });
                vertex.position.copy_from_slice(&point[..3]);
                vertex.rhw = point[3];
                vertex.color = word(at + 12);
                vertex.uv = [float(at + 16), float(at + 20)];
            }
        }
        Ok(())
    }
}

impl ParticlePlayer for Player {
    fn start(&mut self, context: ParticleContext<'_>, frame: ParticleFrame<'_>) -> Result<()> {
        self.update(b'S', context, frame)
    }

    fn advance(&mut self, context: ParticleContext<'_>, frame: ParticleFrame<'_>) -> Result<()> {
        self.update(b'A', context, frame)
    }

    fn stop(&mut self) {
        let _ = self.input.write_all(b"X");
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.input.write_all(b"Q");
        let _ = self.child.wait();
    }
}

fn host_error(error: std::io::Error) -> Error {
    Error::new(0, format!("Particle host: {error}"))
}
