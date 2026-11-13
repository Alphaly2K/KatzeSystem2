use std::ffi::{c_int, c_void};

pub enum Player {}
pub enum Buffer {}

#[repr(C)]
pub struct Plane {
    pub width: u32,
    pub height: u32,
    pub data: *mut u8,
}

#[repr(C)]
pub struct Frame {
    pub time: f64,
    pub width: u32,
    pub height: u32,
    pub y: Plane,
    pub cr: Plane,
    pub cb: Plane,
}

#[repr(C)]
pub struct Samples {
    pub time: f64,
    pub count: u32,
    pub interleaved: [f32; 2304],
}

unsafe extern "C" {
    pub fn plm_buffer_create_with_callbacks(
        load: unsafe extern "C" fn(*mut Buffer, *mut c_void),
        seek: unsafe extern "C" fn(*mut Buffer, usize, *mut c_void),
        tell: unsafe extern "C" fn(*mut Buffer, *mut c_void) -> usize,
        length: usize,
        user: *mut c_void,
    ) -> *mut Buffer;
    pub fn plm_buffer_write(buffer: *mut Buffer, bytes: *mut u8, length: usize) -> usize;
    pub fn katze_movie_buffer_end(buffer: *mut Buffer);
    pub fn plm_create_with_buffer(buffer: *mut Buffer, destroy: c_int) -> *mut Player;
    pub fn plm_destroy(player: *mut Player);
    pub fn plm_probe(player: *mut Player, bytes: usize) -> c_int;
    pub fn katze_movie_rewind(player: *mut Player);
    pub fn plm_has_headers(player: *mut Player) -> c_int;
    pub fn plm_get_width(player: *mut Player) -> c_int;
    pub fn plm_get_height(player: *mut Player) -> c_int;
    pub fn plm_get_framerate(player: *mut Player) -> f64;
    pub fn plm_get_samplerate(player: *mut Player) -> c_int;
    pub fn plm_get_duration(player: *mut Player) -> f64;
    pub fn plm_get_time(player: *mut Player) -> f64;
    pub fn plm_has_ended(player: *mut Player) -> c_int;
    pub fn plm_set_video_decode_callback(
        player: *mut Player,
        callback: unsafe extern "C" fn(*mut Player, *mut Frame, *mut c_void),
        user: *mut c_void,
    );
    pub fn plm_set_audio_decode_callback(
        player: *mut Player,
        callback: unsafe extern "C" fn(*mut Player, *mut Samples, *mut c_void),
        user: *mut c_void,
    );
    pub fn plm_decode(player: *mut Player, seconds: f64);
    pub fn plm_frame_to_bgra(frame: *mut Frame, pixels: *mut u8, stride: c_int);
}
