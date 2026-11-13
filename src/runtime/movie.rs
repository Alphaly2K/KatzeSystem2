use super::{ObjectFactory, ObjectHost, Resources};
use crate::graphics::{
    DrawCommand, DrawList, FrameTexture, ProjectedVertex, Rect, Surface, TextureFilter,
    TextureSource,
};
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::movie::Player;
use crate::{Error, Result, read};
use std::sync::Arc;

pub(super) struct Movie {
    handle: u32,
    player: Player,
    frame: Option<Arc<Surface>>,
    rectangle: Option<Rect>,
    exclusive: bool,
    pub surface_size: [u32; 2],
    tick: u32,
}

impl Movie {
    pub fn advance(&mut self, now: u32) -> Result<()> {
        let elapsed = now.wrapping_sub(self.tick);
        self.tick = now;
        self.player.advance(elapsed as f64 / 1000.)
    }

    fn compose(&mut self, size: [u32; 2], filter: TextureFilter) -> Result<Option<DrawList>> {
        if !self.exclusive {
            return Ok(None);
        }
        if let Some(frame) = self.player.take_video()? {
            self.frame = Some(Arc::new(frame.surface));
        }
        let bounds = Rect::from_xywh(0, 0, size[0] as i32, size[1] as i32);
        let mut output = DrawList {
            passes: Vec::new(),
            commands: Vec::new(),
            filter,
        };
        quad(&mut output, bounds, None, 0xff00_0000);
        if let Some(frame) = &self.frame {
            quad(
                &mut output,
                self.rectangle.unwrap_or_else(|| {
                    Rect::from_xywh(
                        0,
                        0,
                        self.surface_size[0] as i32,
                        self.surface_size[1] as i32,
                    )
                }),
                Some(frame.clone()),
                u32::MAX,
            );
        }
        Ok(Some(output))
    }
}

fn quad(output: &mut DrawList, rectangle: Rect, image: Option<Arc<Surface>>, color: u32) {
    if rectangle.right <= rectangle.left || rectangle.bottom <= rectangle.top {
        return;
    }
    let positions = [
        [rectangle.left as f32, rectangle.top as f32, 0.],
        [rectangle.right as f32, rectangle.top as f32, 0.],
        [rectangle.left as f32, rectangle.bottom as f32, 0.],
        [rectangle.right as f32, rectangle.bottom as f32, 0.],
    ];
    let uv = [[0., 0.], [1., 0.], [0., 1.], [1., 1.]];
    for indices in [[0, 1, 2], [2, 1, 3]] {
        output.commands.push(DrawCommand {
            vertices: indices.map(|i| ProjectedVertex {
                position: positions[i],
                rhw: 1.,
                color,
            }),
            texture: image.as_ref().map(|image| FrameTexture {
                source: TextureSource::Image(image.clone()),
                uv: indices.map(|i| uv[i]),
            }),
            clip: None,
            blend_mode: 0,
        });
    }
}

impl Resources {
    pub fn movie_frame(
        &mut self,
        size: [u32; 2],
        filter: TextureFilter,
    ) -> Result<Option<DrawList>> {
        match &mut self.movie {
            Some(movie) => movie.compose(size, filter),
            None => Ok(None),
        }
    }
}

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn movie_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let count = match index {
            266 | 417 | 997 => 8,
            788 => 20,
            _ => 4,
        };
        if arguments.len() != count {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let handle = read::u32_at(arguments, 0)?;
        if index == 265 {
            return self.load_movie(handle, vm);
        }
        let value = matches!(index, 266..=269 | 998);
        if handle == 0 {
            return Ok(if value {
                NativeReturn::Value(0)
            } else {
                NativeReturn::Continue
            });
        }
        let movie = self
            .resources
            .movie
            .as_mut()
            .filter(|movie| movie.handle == handle)
            .ok_or_else(|| Error::new(vm.pc, "电影句柄不存在"))?;
        let result = match index {
            266 => {
                let result = movie.player.play(read::u32_at(arguments, 4)?)?;
                if result {
                    movie.frame = None;
                    movie.tick = self.host.system_time_ms;
                }
                u32::from(result)
            }
            267 => u32::from(movie.player.stop()?),
            268 => {
                movie.player.stop()?;
                self.set_movie_audio(None)?;
                self.resources.movie = None;
                self.resources.compositor.drawing.enabled = true;
                let input = self.command_target(0, 34, [0; 3])?;
                self.command_target(input, 144, [0, 1, 0])?;
                1
            }
            269 => u32::from(movie.player.is_playing()?),
            // 电影沿宿主窗口呈现，现有活动窗口即为影片的前台目标。
            270 => 0,
            417 => {
                let requested = read::u32_at(arguments, 4)?;
                let volume = if let Some(settings) = &self.host.system_settings {
                    let word = |offset| {
                        u32::from_le_bytes(settings.sound[offset..offset + 4].try_into().unwrap())
                    };
                    if word(0x15e8) != 0 || word(0x15ec) != 0 {
                        0
                    } else {
                        requested.wrapping_mul(word(0)) / 100
                    }
                } else {
                    requested
                };
                movie.player.set_volume(volume as i32);
                0
            }
            788 => {
                movie.rectangle = Some(Rect::from_xywh(
                    read::u32_at(arguments, 4)? as i32,
                    read::u32_at(arguments, 8)? as i32,
                    read::u32_at(arguments, 12)? as i32,
                    read::u32_at(arguments, 16)? as i32,
                ));
                0
            }
            997 => {
                let normal = read::u32_at(arguments, 4)? != 0;
                movie.exclusive = !normal;
                self.resources.compositor.drawing.enabled = normal;
                0
            }
            998 => (movie.player.metadata().duration * 1000.) as u32,
            _ => unreachable!(),
        };
        Ok(if value {
            NativeReturn::Value(result)
        } else {
            NativeReturn::Continue
        })
    }

    fn set_movie_audio(&mut self, output: Option<crate::movie::AudioOutput>) -> Result<()> {
        let sound = self.command_target(0, 35, [0; 3])?;
        let state = self
            .objects
            .get_mut(&sound)
            .and_then(|object| object.audio_state_mut())
            .ok_or_else(|| Error::new(0, "电影播放需要可达的声音任务"))?;
        state.set_movie_output(output);
        Ok(())
    }

    fn load_movie(&mut self, address: u32, vm: &mut Interpreter) -> Result<NativeReturn> {
        if self
            .host
            .platform_settings
            .as_ref()
            .is_some_and(|settings| settings.etc_disablemovie != 0)
        {
            return Ok(NativeReturn::Value(0));
        }
        let name = format_string(vm, address, 0)?;
        let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
        if self.resources.movie.is_some() {
            return Err(Error::new(vm.pc, "装载电影前须先释放现有电影"));
        }
        let services = self
            .resources
            .services
            .as_deref_mut()
            .ok_or_else(|| Error::new(vm.pc, "运行时未配置电影资源服务"))?;
        let name = services.resolve(b"movie", name)?;
        let input = self.command_target(0, 34, [0; 3])?;
        self.command_target(input, 144, [0; 3])?;
        self.resources.compositor.drawing.enabled = false;
        let result = (|| {
            let source = self
                .resources
                .services
                .as_deref_mut()
                .unwrap()
                .open(&name, crate::resource::ResourceMode::LooseFirst)?;
            let Some(source) = source else {
                return Ok(None);
            };
            let player = Player::new(source)?;
            let handle = self.host.handle_allocator().allocate()?;
            self.set_movie_audio(Some(player.audio_output()))?;
            Ok(Some(Movie {
                handle,
                player,
                frame: None,
                rectangle: None,
                exclusive: true,
                surface_size: self.host.surface_size,
                tick: self.host.system_time_ms,
            }))
        })();
        match result {
            Ok(Some(movie)) => {
                let handle = movie.handle;
                self.resources.movie = Some(movie);
                Ok(NativeReturn::Value(handle))
            }
            result => {
                self.resources.compositor.drawing.enabled = true;
                self.command_target(input, 144, [0, 1, 0])?;
                result.map(|_| NativeReturn::Value(0))
            }
        }
    }
}
