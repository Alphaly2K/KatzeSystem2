use katze_system2::{
    Error, Result,
    host::{
        MovieAudio, MovieDecoded, MovieDecoder, MovieDecoderFactory, MovieFrame, MovieMetadata,
        SharedSource, Surface,
    },
    io::SourceReader,
};
use std::{
    io::{self, Read},
    process::{Child, ChildStdout, Command, Stdio},
    thread::{self, JoinHandle},
};

pub(super) struct Ffmpeg;

impl MovieDecoderFactory for Ffmpeg {
    fn open(&self, source: SharedSource) -> Result<Box<dyn MovieDecoder>> {
        let (metadata, rate) = probe(source.clone())?;
        Ok(Box::new(Decoder {
            source,
            metadata,
            rate,
            time: 0.,
            video: Track::Pending,
            audio: if metadata.sample_rate == 0 {
                Track::Ended
            } else {
                Track::Pending
            },
            frames: 0,
            samples: 0,
            audio_bytes: Vec::new(),
        }))
    }
}

fn diagnostic(operation: &str, error: impl std::fmt::Display) -> Error {
    Error::new(0, format!("{operation}: {error}"))
}

struct Pipe {
    child: Child,
    output: ChildStdout,
    input: Option<JoinHandle<io::Result<()>>>,
    tool: &'static str,
}

impl Pipe {
    fn start(tool: &'static str, command: &mut Command, source: SharedSource) -> Result<Self> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| diagnostic(tool, e))?;
        let mut stdin = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let mut pipe = Self {
            child,
            output,
            input: None,
            tool,
        };
        pipe.input = Some(
            thread::Builder::new()
                .spawn(
                    move || match io::copy(&mut SourceReader::from_shared(source), &mut stdin) {
                        Ok(_) => Ok(()),
                        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
                        Err(e) => Err(e),
                    },
                )
                .map_err(|e| diagnostic("movie input thread", e))?,
        );
        Ok(pipe)
    }

    fn read_up_to(&mut self, bytes: &mut [u8]) -> Result<usize> {
        let mut count = 0;
        while count < bytes.len() {
            match self.output.read(&mut bytes[count..]) {
                Ok(0) => break,
                Ok(n) => count += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(diagnostic(self.tool, e)),
            }
        }
        Ok(count)
    }

    fn finish(mut self) -> Result<()> {
        let status = self.child.wait().map_err(|e| diagnostic(self.tool, e))?;
        if let Some(input) = self.input.take() {
            input
                .join()
                .map_err(|_| Error::new(0, "Movie input thread ended unexpectedly"))?
                .map_err(|e| diagnostic("movie input", e))?;
        }
        if status.success() {
            Ok(())
        } else {
            Err(diagnostic(self.tool, status))
        }
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        if let Some(input) = self.input.take() {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = input.join();
        } else {
            // Also reap a child if starting its input thread failed.
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn probe(source: SharedSource) -> Result<(MovieMetadata, [u32; 2])> {
    let mut pipe = Pipe::start(
        "ffprobe",
        Command::new("ffprobe").args([
            "-v", "error", "-protocol_whitelist", "pipe", "-i", "pipe:0",
            "-show_entries", "stream=codec_type,width,height,avg_frame_rate,r_frame_rate,sample_rate:format=duration",
            "-of", "xml",
        ]),
        source,
    )?;
    let mut xml = String::new();
    const MAX_METADATA: u64 = 1024 * 1024;
    pipe.output
        .by_ref()
        .take(MAX_METADATA + 1)
        .read_to_string(&mut xml)
        .map_err(|e| diagnostic("ffprobe", e))?;
    if xml.len() as u64 > MAX_METADATA {
        return Err(Error::new(0, "Movie metadata exceeds the size limit"));
    }
    pipe.finish()?;
    read_metadata(&xml)
}

fn read_metadata(xml: &str) -> Result<(MovieMetadata, [u32; 2])> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| diagnostic("ffprobe metadata", e))?;
    let root = doc.root_element();
    let stream = |kind| {
        root.children()
            .find(|n| n.has_tag_name("streams"))?
            .children()
            .find(|n| n.has_tag_name("stream") && n.attribute("codec_type") == Some(kind))
    };
    let video = stream("video").ok_or_else(|| Error::new(0, "Movie has no video stream"))?;
    let number = |node: roxmltree::Node<'_, '_>, key| {
        node.attribute(key)
            .and_then(|n| n.parse::<u32>().ok())
            .filter(|&n| n > 0)
            .ok_or_else(|| Error::new(0, format!("Invalid movie {key}")))
    };
    let size = [number(video, "width")?, number(video, "height")?];
    let ratio = |text: &str| {
        let (numerator, denominator) = text.split_once('/')?;
        let pair = [
            numerator.parse::<u32>().ok()?,
            denominator.parse::<u32>().ok()?,
        ];
        (!pair.contains(&0)).then_some(pair)
    };
    let rate = video
        .attribute("avg_frame_rate")
        .and_then(ratio)
        .or_else(|| video.attribute("r_frame_rate").and_then(ratio))
        .ok_or_else(|| Error::new(0, "Movie has no valid frame rate"))?;
    let sample_rate = stream("audio")
        .map(|audio| number(audio, "sample_rate"))
        .transpose()?
        .unwrap_or(0);
    let duration = root
        .children()
        .find(|n| n.has_tag_name("format"))
        .and_then(|n| n.attribute("duration"))
        .and_then(|n| n.parse::<f64>().ok())
        .filter(|&n| n.is_finite() && n >= 0.)
        .unwrap_or(0.);
    Ok((
        MovieMetadata {
            size,
            frame_rate: f64::from(rate[0]) / f64::from(rate[1]),
            sample_rate,
            duration,
        },
        rate,
    ))
}

enum Track {
    Pending,
    Decoding(Pipe),
    Ended,
}

impl Track {
    fn read(&mut self, dest: &mut [u8]) -> Result<usize> {
        let Self::Decoding(pipe) = self else {
            return Ok(0);
        };
        let count = pipe.read_up_to(dest)?;
        if count < dest.len() {
            let Self::Decoding(pipe) = std::mem::replace(self, Self::Ended) else {
                unreachable!()
            };
            pipe.finish()?;
        }
        Ok(count)
    }
}

struct Decoder {
    source: SharedSource,
    metadata: MovieMetadata,
    rate: [u32; 2],
    time: f64,
    video: Track,
    audio: Track,
    frames: u64,
    samples: u64,
    audio_bytes: Vec<u8>,
}

impl Decoder {
    fn process(&self, args: &[&str]) -> Result<Track> {
        Pipe::start(
            "ffmpeg",
            Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-nostdin",
                    "-xerror",
                    "-protocol_whitelist",
                    "pipe",
                    "-i",
                    "pipe:0",
                ])
                .args(args)
                .arg("pipe:1"),
            self.source.clone(),
        )
        .map(Track::Decoding)
    }
}

impl MovieDecoder for Decoder {
    fn metadata(&self) -> MovieMetadata {
        self.metadata
    }

    fn time(&self) -> f64 {
        self.time
    }

    fn rewind(&mut self) -> Result<()> {
        self.time = 0.;
        self.frames = 0;
        self.samples = 0;
        self.video = Track::Pending;
        self.audio = if self.metadata.sample_rate == 0 {
            Track::Ended
        } else {
            Track::Pending
        };
        Ok(())
    }

    fn advance(&mut self, seconds: f64) -> Result<MovieDecoded> {
        if !seconds.is_finite() || !(0.0..=1.0).contains(&seconds) {
            return Err(Error::new(
                0,
                "Movie decode step must be between zero and one second",
            ));
        }
        let [num, den] = self.rate;
        if matches!(self.video, Track::Pending) {
            let filter = format!("fps={num}/{den}:start_time=0");
            self.video = self.process(&[
                "-map",
                "0:v:0",
                "-vf",
                &filter,
                "-pix_fmt",
                "bgra",
                "-threads:v",
                "1",
                "-f",
                "rawvideo",
            ])?;
        }
        if matches!(self.audio, Track::Pending) {
            self.audio = self.process(&[
                "-map",
                "0:a:0",
                "-af",
                "aresample=async=1:first_pts=0",
                "-ac",
                "2",
                "-f",
                "f32le",
            ])?;
        }

        // Both filters align their outputs to the start of the same input timeline.
        self.time += seconds;
        let mut video = None;
        while !matches!(self.video, Track::Ended) {
            let time = self.frames as f64 * f64::from(den) / f64::from(num);
            if time > self.time + 1e-9 {
                break;
            }
            let mut surface = Surface::new(self.metadata.size)?;
            let count = self.video.read(surface.pixels_mut())?;
            if count == 0 {
                break;
            }
            if count != surface.pixels().len() {
                return Err(Error::new(0, "FFmpeg produced a partial movie frame"));
            }
            video = Some(MovieFrame { time, surface });
            self.frames += 1;
        }
        let mut audio = None;
        if !matches!(self.audio, Track::Ended) {
            let rate = f64::from(self.metadata.sample_rate);
            let end = (self.time * rate + 1e-6).floor() as u64;
            let frames = end.saturating_sub(self.samples);
            let bytes = usize::try_from(frames)
                .ok()
                .and_then(|n| n.checked_mul(8))
                .filter(|&n| n <= 8 * 1024 * 1024)
                .ok_or_else(|| Error::new(0, "Movie PCM decode step exceeds the size limit"))?;
            self.audio_bytes.resize(bytes, 0);
            let count = self.audio.read(&mut self.audio_bytes)?;
            if !count.is_multiple_of(8) {
                return Err(Error::new(0, "FFmpeg produced a partial stereo sample"));
            }
            if count != 0 {
                let time = self.samples as f64 / rate;
                self.samples += (count / 8) as u64;
                let samples = self.audio_bytes[..count]
                    .chunks_exact(4)
                    .map(|sample| {
                        let n = f32::from_le_bytes(sample.try_into().unwrap());
                        if n.is_finite() { n.clamp(-1., 1.) } else { 0. }
                    })
                    .collect();
                audio = Some(MovieAudio { time, samples });
            }
        }
        let ended = matches!(self.video, Track::Ended) && matches!(self.audio, Track::Ended);
        Ok(MovieDecoded {
            video,
            audio,
            ended,
        })
    }
}
