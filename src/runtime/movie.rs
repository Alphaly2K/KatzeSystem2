use super::{ObjectFactory, ObjectHost, Resources, TaskObject};
use crate::graphics::{DrawList, Rect, Surface, TextureFilter};
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::movie::Player;
use crate::{Error, Result, read};
use std::collections::BTreeMap;
use std::sync::Arc;

pub(super) struct Movie {
    handle: u32,
    player: Player,
    frame: Option<Arc<Surface>>,
    rectangle: Option<Rect>,
    exclusive: bool,
    pub surface_size: [u32; 2],
    tick: u32,
    owner: Option<(u32, bool)>,
    audio_task: Option<u32>,
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
            commands: Vec::new(),
            filter,
        };
        output.push_screen_quad(bounds, None, 0xff00_0000, None);
        if let Some(frame) = &self.frame {
            output.push_screen_quad(
                self.rectangle.unwrap_or_else(|| {
                    Rect::from_xywh(
                        0,
                        0,
                        self.surface_size[0] as i32,
                        self.surface_size[1] as i32,
                    )
                }),
                Some(frame),
                u32::MAX,
                None,
            );
        }
        Ok(Some(output))
    }
}

impl Resources {
    pub(super) fn advance_movies(&mut self, now: u32) -> Result<()> {
        if let Some(movie) = &mut self.movie {
            movie.advance(now)?;
        }
        for movie in self.plane_movies.values_mut() {
            movie.advance(now)?;
        }
        Ok(())
    }

    pub(super) fn update_plane_movie_frames(&mut self) -> Result<()> {
        for movie in self.plane_movies.values_mut() {
            if let Some(frame) = movie.player.take_video()? {
                let frame = Arc::new(frame.surface);
                self.compositor
                    .drawables
                    .set_image(movie.owner.unwrap().0, Some(frame.clone()));
                movie.frame = Some(frame);
            }
        }
        Ok(())
    }

    fn remove_plane_movie(
        &mut self,
        objects: &mut BTreeMap<u32, Box<dyn TaskObject>>,
        handle: u32,
    ) -> bool {
        let Some(movie) = self.plane_movies.remove(&handle) else {
            return false;
        };
        if let Some(sound) = movie.audio_task.and_then(|id| objects.get_mut(&id)) {
            if let Some(state) = sound.audio_state_mut() {
                state.set_movie_track(handle, None);
            }
        }
        self.compositor
            .drawables
            .set_image(movie.owner.unwrap().0, None);
        true
    }

    pub(super) fn release_plane_movies(
        &mut self,
        objects: &mut BTreeMap<u32, Box<dyn TaskObject>>,
        owner: u32,
    ) {
        let handles: Vec<_> = self
            .plane_movies
            .iter()
            .filter(|(_, movie)| movie.owner.is_some_and(|value| value.0 == owner))
            .map(|(&handle, _)| handle)
            .collect();
        for handle in handles {
            self.remove_plane_movie(objects, handle);
        }
    }

    pub(super) fn plane_movie_handle(&self, owner: u32, effect: bool) -> u32 {
        self.plane_movies
            .values()
            .find(|movie| movie.owner == Some((owner, effect)))
            .map_or(0, |movie| movie.handle)
    }

    pub fn movie_frame(
        &mut self,
        size: [u32; 2],
        filter: TextureFilter,
    ) -> Result<Option<DrawList>> {
        self.update_plane_movie_frames()?;
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
        let layer = self.resources.plane_movies.contains_key(&handle);
        if index == 268 {
            if layer {
                self.resources.remove_plane_movie(self.objects, handle);
            } else {
                let movie = self
                    .resources
                    .movie
                    .as_mut()
                    .filter(|movie| movie.handle == handle)
                    .ok_or_else(|| Error::new(vm.pc, "电影句柄不存在"))?;
                movie.player.stop()?;
                self.set_movie_audio(None)?;
                self.resources.movie = None;
            }
            self.resources.compositor.drawing.enabled = true;
            let input = self.command_target(0, 34, [0; 3])?;
            self.command_target(input, 144, [0, 1, 0])?;
            return Ok(NativeReturn::Value(1));
        }
        if layer && index == 266 {
            self.connect_plane_movie_audio(handle)?;
        }
        let movie = if layer {
            self.resources.plane_movies.get_mut(&handle)
        } else {
            self.resources
                .movie
                .as_mut()
                .filter(|movie| movie.handle == handle)
        }
        .ok_or_else(|| Error::new(vm.pc, "电影句柄不存在"))?;
        let result = match index {
            266 => {
                let result = movie.player.play(read::u32_at(arguments, 4)?)?;
                if result {
                    movie.frame = None;
                    movie.tick = self.host.system_time_ms;
                    if let Some((owner, _)) = movie.owner {
                        self.resources.compositor.drawables.set_image(owner, None);
                    }
                }
                u32::from(result)
            }
            267 => u32::from(movie.player.stop()?),
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

    fn connect_plane_movie_audio(&mut self, handle: u32) -> Result<()> {
        let movie = &self.resources.plane_movies[&handle];
        if movie.player.metadata().sample_rate == 0 {
            return Ok(());
        }
        let output = movie.player.audio_output();
        let sound = self.command_target(0, 35, [0; 3])?;
        let state = self
            .objects
            .get_mut(&sound)
            .and_then(|object| object.audio_state_mut())
            .ok_or_else(|| Error::new(0, "电影播放需要可达的声音任务"))?;
        state.set_movie_track(handle, Some(output));
        self.resources
            .plane_movies
            .get_mut(&handle)
            .unwrap()
            .audio_task = Some(sound);
        Ok(())
    }

    pub(super) fn create_plane_movie(
        &mut self,
        owner: u32,
        source: crate::io::SharedSource,
        effect: bool,
    ) -> Result<[u32; 2]> {
        let player = Player::new(source)?;
        let size = player.metadata().size;
        let handle = self.host.handle_allocator().allocate()?;
        self.resources.plane_movies.insert(
            handle,
            Movie {
                handle,
                player,
                frame: None,
                rectangle: None,
                exclusive: false,
                surface_size: self.host.surface_size,
                tick: self.host.system_time_ms,
                owner: Some((owner, effect)),
                audio_task: None,
            },
        );
        Ok(size)
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
                owner: None,
                audio_task: None,
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
