//! MPEG-1 视频与 MP2／MP3 立体声的流式解码；输入不依赖平台文件路径。

mod ffi;
mod playback;
pub use playback::{AudioOutput, Player};

use crate::{Error, Result, graphics::Surface, io::SharedSource, read};
use std::{ffi::c_void, panic::AssertUnwindSafe, ptr::NonNull};

/// 顶端起始、不透明 BGRA 视频帧，时间以秒计。
pub struct Frame {
    pub time: f64,
    pub surface: Surface,
}

/// 从 time 开始的连续交错立体声采样。
pub struct Audio {
    pub time: f64,
    pub samples: Vec<f32>,
}

/// 本次推进的最后一幅画面及全部音频；没有新画面时继续显示上一帧。
pub struct Decoded {
    pub video: Option<Frame>,
    pub audio: Option<Audio>,
    pub ended: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Metadata {
    pub size: [u32; 2],
    pub frame_rate: f64,
    /// 零表示没有音轨；存在时音频输出始终为两声道。
    pub sample_rate: u32,
    pub duration: f64,
}

struct Input {
    source: SharedSource,
    length: usize,
    position: usize,
    error: Option<Error>,
    buffer: [u8; 32768],
    video: Option<Frame>,
    audio: Option<Audio>,
}

/// 独占解码状态，可移动到工作线程；推进时同步读取 SharedSource。
/// 同时消费视频和音频，不将完整电影载入内存。当前接受偶数宽高的 MPEG-1。
pub struct Decoder {
    player: NonNull<ffi::Player>,
    input: Box<Input>,
    metadata: Metadata,
}

// C 状态及回调上下文由此实例独占，库没有线程局部或全局可变解码状态。
unsafe impl Send for Decoder {}

impl Decoder {
    pub fn new(source: SharedSource) -> Result<Self> {
        let length = usize::try_from(source.len()?)
            .ok()
            .filter(|&length| length > 0 && length <= i32::MAX as usize)
            .ok_or_else(|| Error::new(0, "电影来源为空或超过解码器位置范围"))?;
        let mut input = Box::new(Input {
            source,
            length,
            position: 0,
            error: None,
            buffer: [0; 32768],
            video: None,
            audio: None,
        });
        let user = (&mut *input as *mut Input).cast();
        // 输入上下文地址在 C 解码器释放前保持稳定；buffer 的所有权移交给 player。
        let player = unsafe {
            let buffer = ffi::plm_buffer_create_with_callbacks(load, seek, tell, length, user);
            NonNull::new(ffi::plm_create_with_buffer(buffer, 1))
        }
        .ok_or_else(|| Error::new(0, "电影解码器创建失败"))?;
        let mut decoder = Self {
            player,
            input,
            metadata: Metadata {
                size: [0; 2],
                frame_rate: 0.,
                sample_rate: 0,
                duration: 0.,
            },
        };
        // 容器头的 audio_bound 可能为零；实际 PES 扫描仍能发现 MP2 音轨。
        let valid = unsafe {
            ffi::katze_movie_rewind(player.as_ptr());
            ffi::plm_probe(player.as_ptr(), length.min(1024 * 1024)) != 0
                && ffi::plm_has_headers(player.as_ptr()) != 0
        };
        decoder.check_error()?;
        if !valid {
            return Err(Error::new(0, "电影缺少可解码的 MPEG-1／MP2／MP3 流头"));
        }
        let metadata = unsafe {
            Metadata {
                size: [
                    ffi::plm_get_width(player.as_ptr()) as u32,
                    ffi::plm_get_height(player.as_ptr()) as u32,
                ],
                frame_rate: ffi::plm_get_framerate(player.as_ptr()),
                sample_rate: ffi::plm_get_samplerate(player.as_ptr()) as u32,
                duration: ffi::plm_get_duration(player.as_ptr()),
            }
        };
        decoder.check_error()?;
        if metadata
            .size
            .iter()
            .any(|&value| value == 0 || value % 2 != 0)
            || !metadata.frame_rate.is_finite()
            || metadata.frame_rate <= 0.
            || !metadata.duration.is_finite()
            || metadata.duration < 0.
        {
            return Err(Error::new(0, "电影视频尺寸、帧率或时长无效"));
        }
        Surface::new(metadata.size)?;
        decoder.metadata = metadata;
        unsafe {
            ffi::katze_movie_rewind(player.as_ptr());
            ffi::plm_set_video_decode_callback(player.as_ptr(), video, user);
            ffi::plm_set_audio_decode_callback(player.as_ptr(), audio, user);
        }
        Ok(decoder)
    }

    pub fn metadata(&self) -> Metadata {
        self.metadata
    }

    pub fn time(&self) -> f64 {
        unsafe { ffi::plm_get_time(self.player.as_ptr()) }
    }

    /// 回到流起点，清除尚未取走的输出；读取故障后的实例仍返回原诊断。
    pub fn rewind(&mut self) -> Result<()> {
        self.check_error()?;
        self.input.video = None;
        self.input.audio = None;
        unsafe {
            ffi::katze_movie_rewind(self.player.as_ptr());
        }
        self.check_error()
    }

    /// 每次推进零至一秒；较大的时钟变化由调用方分批推进。
    /// 解码错误后应丢弃本实例，避免继续使用不完整输出。
    pub fn advance(&mut self, seconds: f64) -> Result<Decoded> {
        self.check_error()?;
        if !seconds.is_finite() || !(0.0..=1.0).contains(&seconds) {
            return Err(Error::new(0, "电影推进间隔须为零至一秒"));
        }
        unsafe {
            ffi::plm_decode(self.player.as_ptr(), seconds);
        }
        self.check_error()?;
        Ok(Decoded {
            video: self.input.video.take(),
            audio: self.input.audio.take(),
            ended: unsafe { ffi::plm_has_ended(self.player.as_ptr()) != 0 },
        })
    }

    fn check_error(&self) -> Result<()> {
        self.input.error.clone().map_or(Ok(()), Err)
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            ffi::plm_destroy(self.player.as_ptr());
        }
    }
}

// 所有回调均同步发生于解码调用；错误及宿主 panic 在 C 边界内转换为诊断。
unsafe fn callback(user: *mut c_void, operation: impl FnOnce(&mut Input) -> Result<()>) {
    let input = unsafe { &mut *user.cast::<Input>() };
    if input.error.is_some() {
        return;
    }
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| operation(input)));
    input.error = match result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error),
        Err(_) => Some(Error::new(input.position, "电影读取或解码回调发生 panic")),
    };
}

unsafe extern "C" fn load(buffer: *mut ffi::Buffer, user: *mut c_void) {
    unsafe {
        callback(user, |input| {
            let capacity = input.buffer.len().min(input.length - input.position);
            if capacity == 0 {
                ffi::katze_movie_buffer_end(buffer);
                return Ok(());
            }
            let mut count = 0;
            while count < capacity {
                let read = input.source.read_at(
                    (input.position + count) as u64,
                    &mut input.buffer[count..capacity],
                )?;
                if read == 0 || read > capacity - count {
                    return Err(Error::new(
                        input.position + count,
                        "电影来源提前结束或返回量超过缓冲区",
                    ));
                }
                count += read;
            }
            if ffi::plm_buffer_write(buffer, input.buffer.as_mut_ptr(), count) != count {
                return Err(Error::new(input.position, "电影输入缓冲写入失败"));
            }
            input.position += count;
            Ok(())
        });
    }
    if unsafe { &*user.cast::<Input>() }.error.is_some() {
        unsafe {
            ffi::katze_movie_buffer_end(buffer);
        }
    }
}

unsafe extern "C" fn seek(_: *mut ffi::Buffer, position: usize, user: *mut c_void) {
    unsafe {
        callback(user, |input| {
            if position > input.length {
                return Err(Error::new(position, "电影读取位置超出来源范围"));
            }
            input.position = position;
            Ok(())
        });
    }
}

unsafe extern "C" fn tell(_: *mut ffi::Buffer, user: *mut c_void) -> usize {
    unsafe { &*user.cast::<Input>() }.position
}

unsafe extern "C" fn video(_: *mut ffi::Player, frame: *mut ffi::Frame, user: *mut c_void) {
    unsafe {
        callback(user, |input| {
            let size = [(*frame).width, (*frame).height];
            if size.iter().any(|&value| value == 0 || value % 2 != 0) {
                return Err(Error::new(0, "电影视频帧需要非零偶数宽高"));
            }
            let output = match input.video.as_mut() {
                Some(output) if output.surface.size() == size => output,
                _ => {
                    let mut surface = Surface::new(size)?;
                    surface.pixels_mut().fill(255);
                    input.video.insert(Frame {
                        time: (*frame).time,
                        surface,
                    })
                }
            };
            ffi::plm_frame_to_bgra(
                frame,
                output.surface.pixels_mut().as_mut_ptr(),
                (size[0] * 4) as i32,
            );
            output.time = (*frame).time;
            Ok(())
        });
    }
}

unsafe extern "C" fn audio(_: *mut ffi::Player, samples: *mut ffi::Samples, user: *mut c_void) {
    unsafe {
        callback(user, |input| {
            let samples = &*samples;
            let count = (samples.count as usize)
                .checked_mul(2)
                .filter(|&count| count <= samples.interleaved.len())
                .ok_or_else(|| Error::new(0, "电影音频采样帧数量无效"))?;
            let audio = input.audio.get_or_insert_with(|| Audio {
                time: samples.time,
                samples: Vec::new(),
            });
            if audio.samples.len() + count > read::MAX_BYTES / 4 {
                return Err(Error::new(0, "电影单批音频超过容量上限"));
            }
            audio
                .samples
                .extend_from_slice(&samples.interleaved[..count]);
            Ok(())
        });
    }
}
