use super::{ImageList, Images, accumulate};
use crate::{Error, Result, anm::ImageAnimation};

pub(super) struct Pass {
    keys: Vec<u32>,
    index: usize,
    now: u32,
    restart: bool,
    started: bool,
}

impl ImageList {
    pub fn animation(&self, key: u32) -> Option<&ImageAnimation> {
        self.entries.get(&key).map(|entry| &entry.animation)
    }

    /// 已有条目装载 ANM；失败移除旧时间线，保留播放控制和旧显示缓存。
    pub fn load_animation(&mut self, key: u32, bytes: &[u8]) -> Result<bool> {
        self.check_animation_pass()?;
        let Some(entry) = self.entries.get_mut(&key) else {
            return Ok(false);
        };
        let max_id = entry
            .frames
            .last_key_value()
            .map_or(0, |(&id, _)| id as i32);
        entry.animation.load(bytes, max_id)?;
        Ok(true)
    }

    /// 设置动画时间基点，即使当前没有 ANM 也保留该值。
    pub fn set_animation_time(&mut self, key: u32, time: u32) -> Result<bool> {
        self.check_animation_pass()?;
        let Some(entry) = self.entries.get_mut(&key) else {
            return Ok(false);
        };
        entry.animation.time = time;
        if let Some(timeline) = &mut entry.animation.timeline {
            timeline.origin = time;
        }
        Ok(true)
    }

    /// 保存原始播放开关；切换本身不产生脏区或推进时间线。
    pub fn set_animation_playing(&mut self, key: u32, playing: i32) -> Result<bool> {
        self.check_animation_pass()?;
        let Some(entry) = self.entries.get_mut(&key) else {
            return Ok(false);
        };
        entry.animation.playing = playing;
        Ok(true)
    }

    pub fn animations_pending(&self) -> bool {
        self.animation_pass.is_some()
    }

    /// 强制重启一个条目的动画，保留原播放开关；缺失条目直接完成。
    pub fn restart_animation(
        &mut self,
        key: u32,
        budget: usize,
        images: &mut Images,
    ) -> Result<bool> {
        self.restart_inner(key, budget, images, None)
    }

    pub fn restart_animation_with_random(
        &mut self,
        key: u32,
        budget: usize,
        images: &mut Images,
        random: &mut impl FnMut() -> u32,
    ) -> Result<bool> {
        self.restart_inner(key, budget, images, Some(random))
    }

    fn restart_inner(
        &mut self,
        key: u32,
        budget: usize,
        images: &mut Images,
        random: Option<&mut dyn FnMut() -> u32>,
    ) -> Result<bool> {
        if let Some(pass) = &self.animation_pass {
            if !pass.restart || pass.keys != [key] {
                return Err(Error::new(0, "图像列表正在执行其他动画更新"));
            }
        } else {
            if !self.entries.contains_key(&key) {
                return Ok(true);
            }
            self.animation_pass = Some(Pass {
                keys: vec![key],
                index: 0,
                now: u32::MAX,
                restart: true,
                started: false,
            });
        }
        self.advance_animations(u32::MAX, budget, images, random)
    }

    pub fn update_animations(
        &mut self,
        now: u32,
        budget: usize,
        images: &mut Images,
    ) -> Result<bool> {
        self.advance_animations(now, budget, images, None)
    }

    /// 每条目的预算独立；续跑保留本轮时刻、游标及随机副作用。
    pub fn update_animations_with_random(
        &mut self,
        now: u32,
        budget: usize,
        images: &mut Images,
        random: &mut impl FnMut() -> u32,
    ) -> Result<bool> {
        self.advance_animations(now, budget, images, Some(random))
    }

    fn advance_animations(
        &mut self,
        now: u32,
        budget: usize,
        images: &mut Images,
        mut random: Option<&mut dyn FnMut() -> u32>,
    ) -> Result<bool> {
        if self.animation_pass.is_none() {
            let mut keys: Vec<_> = self.entries.keys().copied().collect();
            keys.sort_unstable_by_key(|&key| key as i32);
            self.animation_pass = Some(Pass {
                keys,
                index: 0,
                now,
                restart: false,
                started: false,
            });
        }
        let mut pass = self.animation_pass.take().unwrap();
        while let Some(&key) = pass.keys.get(pass.index) {
            let entry = self.entries.get_mut(&key).unwrap();
            entry.history = accumulate(
                entry.history,
                images.list_bounds(
                    entry.image,
                    entry.properties.position,
                    entry.properties.base_point,
                )?,
                entry.clip,
            );
            let result = if pass.restart && !pass.started {
                pass.started = true;
                match random.as_mut() {
                    Some(random) => entry.animation.restart_with_random(
                        budget,
                        entry.frames.contains_key(&10000),
                        random,
                    ),
                    None => entry
                        .animation
                        .restart(budget, entry.frames.contains_key(&10000)),
                }
            } else {
                match random.as_mut() {
                    Some(random) => entry.animation.update_with_random(
                        pass.now,
                        budget,
                        entry.frames.contains_key(&10000),
                        random,
                    ),
                    None => {
                        entry
                            .animation
                            .update(pass.now, budget, entry.frames.contains_key(&10000))
                    }
                }
            };
            if !result? {
                self.animation_pass = Some(pass);
                return Ok(false);
            }
            let frame_change = entry.animation.frame_change();
            if let Some(id) = frame_change {
                self.select_frame(key, id as u32, images)?;
            }
            let entry = self.entries.get_mut(&key).unwrap();
            if pass.restart || (frame_change.is_some() && entry.visible()) {
                let previous = entry.history;
                entry.history = accumulate(
                    entry.history,
                    images.list_bounds(
                        entry.image,
                        entry.properties.position,
                        entry.properties.base_point,
                    )?,
                    entry.clip,
                );
                let next = entry.history;
                self.mark(previous);
                self.mark(next);
            }
            pass.index += 1;
        }
        Ok(true)
    }

    pub(super) fn check_animation_pass(&self) -> Result<()> {
        if self.animation_pass.is_some() {
            Err(Error::new(0, "图像列表动画推进尚未完成"))
        } else {
            Ok(())
        }
    }
}
